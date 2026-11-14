//! Owner orchestration through the selected compiled capability. No terminal I/O or native DTOs.
use crate::{
    error::{Error, Result},
    plugins::{for_network, owner_store},
    routing,
    wallet::LocalSigner,
};
use std::path::Path;

pub struct Context<'a> {
    pub network: &'a str,
    pub backend: &'a str,
    pub authority: &'a crate::config::ServerAuthority,
    pub origin: &'a str,
    pub rpc_url: &'a str,
    pub root: &'a Path,
    pub signer: &'a LocalSigner,
    pub clock: &'a dyn Fn() -> Result<u64>,
}
impl Context<'_> {
    pub(crate) fn provider_context<'a>(
        &'a self,
        store: &'a dyn Fn() -> Result<crate::Store>,
    ) -> allowit_core::vendor::owner::Context<'a> {
        allowit_core::vendor::owner::Context {
            network: self.network,
            backend: self.backend,
            authority: &self.authority.0,
            origin: self.origin,
            rpc_url: self.rpc_url,
            store,
            signer: self.signer,
            clock: self.clock,
        }
    }
    fn check(&self) -> Result<()> {
        if self.authority.origin() != crate::config::parse_origin(self.backend)?
            || self.authority.network() != routing::normalize(self.network)?
        {
            return Err(Error::config(
                "Owner context differs from independently configured server authority",
            ));
        }
        if routing::normalize(self.network)? != self.signer.network() {
            return Err(Error::config(
                "Owner signer belongs to another selected rail",
            ));
        }
        Ok(())
    }
}
pub use allowit_core::vendor::owner::{
    Activate, Comparison, Counters, Deploy, EditParams, HostPlacement, NativePlacement,
    PairedExport, Revise, Setup, StateReadback, UpdateParams, Verification,
};
pub use allowit_core::vendor::runtime::ParamsUpdate;
pub fn deploy(cx: &Context<'_>, request: Deploy<'_>) -> Result<serde_json::Value> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    for_network(cx.network)?
        .owner()
        .deploy(&cx.provider_context(&store), request)
}
pub fn fund(
    cx: &Context<'_>,
    document: &[u8],
    id: &str,
    amount: &str,
    fee_limit: &str,
) -> Result<serde_json::Value> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    for_network(cx.network)?.owner().fund(
        &cx.provider_context(&store),
        document,
        id,
        amount,
        fee_limit,
    )
}
pub fn activate(cx: &Context<'_>, request: Activate<'_>) -> Result<serde_json::Value> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    for_network(cx.network)?
        .owner()
        .activate(&cx.provider_context(&store), request)
}
pub fn export(
    cx: &Context<'_>,
    document: &[u8],
    activation_id: &str,
) -> Result<crate::profiles::Profile> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    let raw = for_network(cx.network)?.owner().export(
        &cx.provider_context(&store),
        document,
        activation_id,
    )?;
    crate::profiles::Cache::open(cx.root)?
        .install(cx.network, cx.backend, raw)?
        .with_authority(cx.authority)
}
pub fn setup(cx: &Context<'_>, request: Setup<'_>) -> Result<serde_json::Value> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    for_network(cx.network)?
        .owner()
        .setup(&cx.provider_context(&store), request)
}
pub fn status(root: &Path, network: &str, rpc_url: &str, id: &str) -> Result<serde_json::Value> {
    let store = owner_store(root, network)?;
    for_network(network)?
        .owner()
        .status(&store, network, rpc_url, id)
}
/// Change typed params of an unsigned draft through the backend. The reply is the next
/// revision of the draft. The backend refuses a value outside the param's type or range.
pub fn edit_params(cx: &Context<'_>, request: EditParams<'_>) -> Result<serde_json::Value> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    for_network(cx.network)?
        .owner()
        .edit_params(&cx.provider_context(&store), request)
}
/// Revise an unsigned draft in the owner's words. The reply is the next revision.
pub fn revise(cx: &Context<'_>, request: Revise<'_>) -> Result<serde_json::Value> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    for_network(cx.network)?
        .owner()
        .revise(&cx.provider_context(&store), request)
}
/// Change installed params on an active wallet with one owner signature. The workflow is
/// resume-safe under its request ID, as `deploy` and `fund` are. The outcome selects the
/// updated installation.
pub fn update_params(cx: &Context<'_>, request: UpdateParams<'_>) -> Result<serde_json::Value> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    for_network(cx.network)?
        .owner()
        .update_params(&cx.provider_context(&store), request)
}
/// Compare the owner's installation document with the ledger and read the wallet state back.
/// `Verification::matches` is true only when every comparison matches.
pub fn verify(cx: &Context<'_>, document: &[u8]) -> Result<Verification> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    for_network(cx.network)?
        .owner()
        .verify(&cx.provider_context(&store), document)
}
/// The host and native placements of a draft or installed policy document: what the server
/// evaluates and what the wallet enforces.
pub fn paired_export(network: &str, document: &[u8]) -> Result<PairedExport> {
    let network = routing::normalize(network)?;
    for_network(network)?
        .owner()
        .paired_export(network, document)
}
/// The typed values of the owner's text edits for a draft, or for the outcome of a deployment,
/// funding or params update, of the rail of `network`, with each change as the owner reads it.
/// An outcome holds its policy and installation under `selection`; its installed params are
/// the ones that are edited. `edits` maps a param key to text in human units, such as `3.5`
/// for an amount or `2026-12-31` for an expiry. The checks are those of
/// [`crate::parameters::set_all`]: type, range, a future expiry and limits that fit each
/// other. The backend checks the values again.
pub fn typed_changes(
    network: &str,
    document: &serde_json::Value,
    edits: &std::collections::BTreeMap<String, String>,
    now: i64,
) -> Result<(
    std::collections::BTreeMap<String, crate::PolicyValue>,
    Vec<crate::parameters::Change>,
)> {
    let network = routing::normalize(network)?;
    let rail = for_network(network)?;
    // The document itself, the selection of an outcome, or the policy of that selection:
    // the first one whose params the rail reads.
    let selection = document.get("selection");
    let policy = selection.and_then(|selection| selection.get("policy"));
    let mut read = rail.draft_params(network, document);
    for candidate in [selection, policy].into_iter().flatten() {
        if read.is_ok() {
            break;
        }
        read = rail.draft_params(network, candidate);
    }
    let read = read?;
    let code = [Some(document), policy, selection]
        .into_iter()
        .flatten()
        .find_map(|part| part.get("code").and_then(serde_json::Value::as_str))
        .ok_or_else(|| Error::usage("The document holds no policy source"))?;
    let known = crate::parameters::known_assets(network)?;
    let draft = crate::parameters::Parameters {
        params: read.params.iter().cloned().collect(),
        kinds: Default::default(),
        asset: crate::parameters::Asset {
            address: known
                .iter()
                .find(|asset| asset.symbol == read.asset.symbol)
                .map(|asset| asset.address.clone())
                .unwrap_or_default(),
            symbol: read.asset.symbol.clone(),
            decimals: u32::from(read.asset.decimals),
        },
        known,
        unresolved: Vec::new(),
    };
    let (next, changes) =
        crate::parameters::set_all(&draft, code, edits, now).map_err(Error::usage)?;
    let typed = changes
        .iter()
        .map(|change| {
            let value = next
                .params
                .get(&change.key)
                .cloned()
                .ok_or_else(|| Error::config("The edit dropped a param"))?;
            serde_json::from_value(value)
                .map(|value| (change.key.clone(), value))
                .map_err(|_| Error::config("The edit produced an untyped value"))
        })
        .collect::<Result<_>>()?;
    Ok((typed, changes))
}
