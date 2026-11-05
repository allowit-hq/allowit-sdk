//! Owner orchestration through the selected compiled capability. No terminal I/O or native DTOs.
use crate::{
    error::{Error, Result},
    routing, vendors,
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
pub struct Deploy<'a> {
    pub draft: &'a [u8],
    pub agent: &'a str,
    pub server: &'a str,
    pub executable: &'a [u8],
    pub request_id: &'a str,
    pub fee_limit: &'a str,
}
pub struct Activate<'a> {
    pub document: &'a [u8],
    pub agent: Option<&'a str>,
    pub server: Option<&'a str>,
    pub request_id: &'a str,
}
pub struct Setup<'a> {
    pub agent: &'a str,
    pub server: &'a str,
    pub request_id: &'a str,
    pub fee_per_operation: &'a str,
}
pub fn deploy(cx: &Context<'_>, request: Deploy<'_>) -> Result<serde_json::Value> {
    cx.check()?;
    vendors::owner(cx.network)?.deploy(cx, request)
}
pub fn fund(
    cx: &Context<'_>,
    document: &[u8],
    id: &str,
    amount: &str,
    fee_limit: &str,
) -> Result<serde_json::Value> {
    cx.check()?;
    vendors::owner(cx.network)?.fund(cx, document, id, amount, fee_limit)
}
pub fn activate(cx: &Context<'_>, request: Activate<'_>) -> Result<serde_json::Value> {
    cx.check()?;
    vendors::owner(cx.network)?.activate(cx, request)
}
pub fn export(
    cx: &Context<'_>,
    document: &[u8],
    activation_id: &str,
) -> Result<crate::profiles::Profile> {
    cx.check()?;
    vendors::owner(cx.network)?.export(cx, document, activation_id)
}
pub fn setup(cx: &Context<'_>, request: Setup<'_>) -> Result<serde_json::Value> {
    cx.check()?;
    vendors::owner(cx.network)?.setup(cx, request)
}
pub fn status(root: &Path, network: &str, rpc_url: &str, id: &str) -> Result<serde_json::Value> {
    vendors::owner(network)?.status(root, network, rpc_url, id)
}
