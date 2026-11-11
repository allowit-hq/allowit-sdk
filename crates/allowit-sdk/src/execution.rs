//! Shared execution input and profile-bound orchestration. Callers control keys and storage.
use crate::{
    error::{Error, Result, quoted},
    input::asset::{Amount, Token},
    plugins::{self, execution_store, for_network},
    profiles::Profile,
    wallet::LocalSigner,
};
pub use allowit_core::vendor::execution::{
    Binding, Capabilities, Outcome, Quantity, Recovery, Request, Settlement,
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
