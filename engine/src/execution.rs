//! Shared execution input and profile-bound orchestration. Callers control keys and storage.
use crate::{
    error::Result,
    input::asset::{Amount, Token},
    profiles::Profile,
    vendors,
    wallet::LocalSigner,
};
use std::{path::Path, time::Duration};
pub struct Request {
    pub op: String,
    pub to: String,
    pub amount: Amount,
    pub token_out: Option<Amount>,
    pub args: Option<serde_json::Value>,
    pub request_id: String,
    pub wait: Duration,
    pub build_only: bool,
    pub wallet: Option<String>,
}
pub struct Outcome {
    pub document: serde_json::Value,
    pub exit_code: i32,
}
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

pub struct Capabilities {
    pub curve: &'static str,
    pub address_is_key: bool,
    pub operations: &'static [&'static str],
    pub build_only: bool,
    pub verbs: &'static [&'static str],
}
pub fn capabilities(network: &str) -> Result<Capabilities> {
    vendors::capabilities(network)
}
pub fn fund_key(network: &str, address: &str, endpoint: Option<&str>) -> Result<String> {
    vendors::fund_key(network, address, endpoint)
}

pub struct Binding {
    pub wallet: String,
    pub policy: String,
}
pub fn status_binding(profile: &Profile, root: &Path, id: &str) -> Result<Binding> {
    vendors::status_binding(profile, root, id)
}
