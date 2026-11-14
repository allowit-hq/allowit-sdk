//! A settlement service for sellers, for hosts. A seller that takes payment authorizations
//! names a service that checks and submits them. The rail of each network implements the
//! checks, the submission and the read of the settlement; the host serves the routes, keeps
//! the service's key and bounds what a caller can ask. No function here names a vendor.
use crate::{
    error::{Error, Result},
    plugins::{for_network, rails},
    routing::normalize,
};
use allowit_core::vendor::settlement_service::SettlementProvider;
pub use allowit_core::vendor::{
    runtime::Signer,
    settlement_service::{
        Authorization, Limits, MAX_REQUEST_BYTES, Outcome, Settled, Supported, Verified,
    },
};
use serde_json::Value;

fn hook(network: &str) -> Result<(&'static str, &'static dyn SettlementProvider)> {
    let network = normalize(network)?;
    for_network(network)?
        .settlement_service()
        .map(|hook| (network, hook))
        .ok_or_else(|| {
            Error::unsupported(format!("No settlement service is available on {network}"))
        })
}

/// The networks of the registered rails that have a settlement service.
pub fn networks() -> Vec<&'static str> {
    rails()
        .iter()
        .filter(|rail| rail.settlement_service().is_some())
        .flat_map(|rail| rail.networks().iter().map(|network| network.id))
        .collect()
}

/// The kinds of payment that the rail of `network` settles.
pub fn supported(network: &str) -> Result<Supported> {
    let (network, hook) = hook(network)?;
    hook.supported(network)
}

/// Check the authorization of `request` against the ledger at `endpoint`. Nothing is
/// submitted. `signer` is the identity of the service's key.
pub fn verify(
    network: &str,
    endpoint: &str,
    signer: &str,
    request: &Value,
    limits: &Limits,
) -> Result<Verified> {
    let (network, hook) = hook(network)?;
    hook.verify(network, endpoint, signer, request, limits)
}

/// Check the authorization of `request` again, submit it with `signer` and read the
/// settlement from the ledger within `limits.wait`.
pub fn settle(
    network: &str,
    endpoint: &str,
    signer: &dyn Signer,
    request: &Value,
    limits: &Limits,
) -> Result<Settled> {
    let (network, hook) = hook(network)?;
    hook.settle(network, endpoint, signer, request, limits)
}

/// Read the ledger again for a settlement that was pending.
pub fn observe(
    network: &str,
    endpoint: &str,
    request: &Value,
    transaction: &[u8; 32],
) -> Result<Settled> {
    let (network, hook) = hook(network)?;
    hook.observe(network, endpoint, request, transaction)
}
