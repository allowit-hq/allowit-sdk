//! Shared execution input and profile-bound orchestration. Callers control keys and storage.
use crate::{
    error::Result,
    input::asset::{Amount, Token},
    profiles::Profile,
    vendors,
    wallet::LocalSigner,
};
pub use allowit_core::vendor::execution::{Binding, Capabilities, Outcome, Recovery, Request};
use std::{path::Path, time::Duration};
pub fn tokens(network: &str) -> Result<&'static [Token]> {
    vendors::execution_tokens(network)
}
pub fn check(
    network: &str,
    op: &str,
    to: &str,
    minimum: Option<&Amount>,
    args: bool,
) -> Result<()> {
    vendors::check_execution(network, op, to, minimum, args)
}
pub fn build(
    profile: &Profile,
    root: &Path,
    rpc: Option<&str>,
    r: &Request,
) -> Result<serde_json::Value> {
    vendors::build_execution(profile, root, rpc, r)
}
pub fn execute(
    profile: Profile,
    signer: LocalSigner,
    root: &Path,
    rpc: Option<String>,
    r: &Request,
) -> Result<Outcome> {
    profile.require_authority()?;
    vendors::execute(profile, signer, root, rpc, r)
}
pub fn status(
    profile: Profile,
    signer: LocalSigner,
    root: &Path,
    rpc: Option<String>,
    id: &str,
    wait: Duration,
) -> Result<Outcome> {
    vendors::execution_status(profile, signer, root, rpc, id, wait)
}

pub fn capabilities(network: &str) -> Result<Capabilities> {
    vendors::capabilities(network)
}
pub fn fund_key(network: &str, address: &str, endpoint: Option<&str>) -> Result<String> {
    vendors::fund_key(network, address, endpoint)
}

pub fn retained(
    root: &Path,
    network: &str,
    wallet: Option<&str>,
    id: &str,
) -> Result<Option<Recovery>> {
    vendors::retained(root, network, wallet, id)
}
pub fn recover(
    original: Recovery,
    signer: LocalSigner,
    backend: &str,
    root: &Path,
    id: &str,
    wait: Duration,
    request: Option<&Request>,
) -> Result<Outcome> {
    vendors::recover(original, signer, backend, root, id, wait, request)
}
