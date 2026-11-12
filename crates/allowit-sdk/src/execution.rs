//! Shared execution input and profile-bound orchestration. Callers control keys and storage.
use crate::{
    error::{Error, Result, quoted},
    input::asset::{Amount, Token},
    plugins::{self, execution_store, for_network},
    profiles::Profile,
    wallet::LocalSigner,
};
pub use allowit_core::vendor::execution::{
    Binding, Capabilities, EXPLORER_KEY, FEE_CHARGED_KEY, LimitReport, Limits, Outcome, Quantity,
    Recovery, Request, Settlement, explorer, fee_charged,
};
use std::{path::Path, sync::Arc, time::Duration};

fn rail_for_operation(network: &str, op: &str) -> Result<&'static dyn plugins::RailPlugin> {
    let rail = for_network(network)?;
    if !rail.capabilities(network).operations.contains(&op) {
        return Err(Error::unsupported(format!(
            "Operation {} is not supported on {network}",
            quoted(op)
        )));
    }
    Ok(rail)
}

pub fn tokens(network: &str) -> Result<&'static [Token]> {
    let network = crate::routing::normalize(network)?;
    Ok(for_network(network)?.tokens(network))
}
pub fn check(
    network: &str,
    op: &str,
    to: &str,
    minimum: Option<&Amount>,
    args: bool,
) -> Result<()> {
    let network = crate::routing::normalize(network)?;
    rail_for_operation(network, op)?
        .execution()
        .check(network, op, to, minimum, args)
}
pub fn build(
    profile: &Profile,
    root: &Path,
    rpc: Option<&str>,
    r: &Request,
) -> Result<serde_json::Value> {
    let network = &profile.description().network;
    let rail = rail_for_operation(network, &r.op)?;
    if !rail.capabilities(network).build_only {
        return Err(Error::unsupported(format!(
            "Building execution requests is not supported on {network}"
        )));
    }
    rail.execution()
        .build(&profile.inner, &|| execution_store(root, rail), rpc, r)
}
pub fn execute(
    profile: Profile,
    signer: LocalSigner,
    root: &Path,
    rpc: Option<String>,
    r: &Request,
) -> Result<Outcome> {
    profile.require_authority()?;
    let rail = rail_for_operation(&profile.description().network, &r.op)?;
    rail.execution().execute(
        profile.inner,
        Arc::new(signer),
        &|| execution_store(root, rail),
        rpc,
        r,
    )
}
pub fn status(
    profile: Profile,
    signer: LocalSigner,
    root: &Path,
    rpc: Option<String>,
    id: &str,
    wait: Duration,
) -> Result<Outcome> {
    let rail = for_network(&profile.description().network)?;
    rail.execution().status(
        profile.inner,
        Arc::new(signer),
        &|| execution_store(root, rail),
        rpc,
        id,
        wait,
    )
}
/// A limit on the amount of one operation, such as the limit of each payment. The policy checks
/// it against one request, so it keeps no counter and has no spent or remaining amount.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentLimit {
    /// The policy param that holds the limit, such as `payment_limit`.
    pub param: String,
    /// What one operation may move, in the installed asset.
    pub limit: Quantity,
}
/// The limits of a wallet: the counted spending limits and the limits on one operation.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletLimits {
    /// What each counted limit allows, what the policy has spent and what remains, as the
    /// backend reports them. `None` means that the backend reported no counters: an unread
    /// counter is never shown as unused.
    pub limits: Option<Vec<LimitReport>>,
    /// The limits on one operation, from the params of the verified wallet profile.
    pub payment_limits: Vec<PaymentLimit>,
}
impl WalletLimits {
    /// The key of the per-payment limits in a document, beside [`Limits::KEY`].
    pub const PAYMENT_KEY: &'static str = "paymentLimits";
    /// The per-payment limits that a document carries, if it has them.
    pub fn payment_of(document: &serde_json::Value) -> Option<Vec<PaymentLimit>> {
        serde_json::from_value(document.get(Self::PAYMENT_KEY)?.clone()).ok()
    }
}
/// The canonical params that limit one operation. Each is an amount of the installed asset.
const PER_PAYMENT: &[&str] = &[allowit_core::PAYMENT_LIMIT];
/// The per-payment limits among the params of a draft, a policy, a public wallet profile or an
/// installation document of the rail of `network`. The rail reads its own document and names
/// the installed asset; a document without such a param has none.
pub fn payment_limits_of(network: &str, document: &serde_json::Value) -> Result<Vec<PaymentLimit>> {
    let network = crate::routing::normalize(network)?;
    let read = for_network(network)?.draft_params(network, document)?;
    Ok(read
        .params
        .iter()
        .filter(|(key, _)| PER_PAYMENT.contains(&key.as_str()))
        .filter_map(|(key, value)| {
            // The `PolicyValue` JSON of an integer: `{"Int": "5"}` or `{"Int": 5}`.
            let units = match value.get("Int")? {
                serde_json::Value::String(text) => text.parse::<i128>().ok()?,
                number => i128::from(number.as_i64()?),
            };
            Some(PaymentLimit {
                param: key.clone(),
                limit: Quantity::new(units, read.asset.clone()),
            })
        })
        .collect())
}
/// The per-payment limits of an imported wallet profile. See [`payment_limits_of`].
pub fn payment_limits(profile: &Profile) -> Result<Vec<PaymentLimit>> {
    payment_limits_of(&profile.description().network, &profile.document()?)
}
/// The limits of the profile's wallet: the counted limits as [`limits`] reads them from the
/// backend, and the per-payment limits of the profile. A wallet that has executed nothing
/// reports both. A profile whose params the rail cannot read has no per-payment limit.
pub fn wallet_limits(
    profile: Profile,
    signer: LocalSigner,
    root: &Path,
    rpc: Option<String>,
) -> Result<WalletLimits> {
    let payment_limits = payment_limits(&profile).unwrap_or_default();
    Ok(WalletLimits {
        limits: limits(profile, signer, root, rpc)?,
        payment_limits,
    })
}
/// The spending limits of the profile's wallet as the backend reports them: what each limit
/// allows, what the policy has spent and what remains. `None` means that the backend reported
/// no counters for this wallet. The read needs the agent key, as a status read does.
pub fn limits(
    profile: Profile,
    signer: LocalSigner,
    root: &Path,
    rpc: Option<String>,
) -> Result<Option<Vec<LimitReport>>> {
    let rail = for_network(&profile.description().network)?;
    rail.execution().limits(
        profile.inner,
        Arc::new(signer),
        &|| execution_store(root, rail),
        rpc,
    )
}

pub fn capabilities(network: &str) -> Result<Capabilities> {
    let network = crate::routing::normalize(network)?;
    Ok(for_network(network)?.capabilities(network))
}
pub fn fund_key(network: &str, address: &str, endpoint: Option<&str>) -> Result<String> {
    let network = crate::routing::normalize(network)?;
    for_network(network)?.fund_key(network, address, endpoint)
}

pub fn retained(
    root: &Path,
    network: &str,
    wallet: Option<&str>,
    id: &str,
) -> Result<Option<Recovery>> {
    let network = crate::routing::normalize(network)?;
    let rail = for_network(network)?;
    rail.execution()
        .retained(&|| execution_store(root, rail), network, wallet, id)
}
/// Recover a retained operation on the rail of the signer's network.
pub fn recover(
    original: Recovery,
    signer: LocalSigner,
    backend: &str,
    root: &Path,
    id: &str,
    wait: Duration,
    request: Option<&Request>,
) -> Result<Outcome> {
    let rail = plugins::for_network(signer.network())?;
    rail.execution().recover(
        original,
        Arc::new(signer),
        backend,
        &|| execution_store(root, rail),
        id,
        wait,
        request,
    )
}
