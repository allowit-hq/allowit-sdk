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
pub use allowit_core::vendor::owner::{Activate, Deploy, Setup};
pub fn deploy(cx: &Context<'_>, request: Deploy<'_>) -> Result<serde_json::Value> {
    cx.check()?;
    let store = || vendors::owner_store(cx.root, cx.network);
    vendors::owner(cx.network)?.deploy(&cx.provider_context(&store), request)
}
pub fn fund(
    cx: &Context<'_>,
    document: &[u8],
    id: &str,
    amount: &str,
    fee_limit: &str,
) -> Result<serde_json::Value> {
    cx.check()?;
    let store = || vendors::owner_store(cx.root, cx.network);
    vendors::owner(cx.network)?.fund(
        &cx.provider_context(&store),
        document,
        id,
        amount,
        fee_limit,
    )
}
pub fn activate(cx: &Context<'_>, request: Activate<'_>) -> Result<serde_json::Value> {
    cx.check()?;
    let store = || vendors::owner_store(cx.root, cx.network);
    vendors::owner(cx.network)?.activate(&cx.provider_context(&store), request)
}
pub fn export(
    cx: &Context<'_>,
    document: &[u8],
    activation_id: &str,
) -> Result<crate::profiles::Profile> {
    cx.check()?;
    let store = || vendors::owner_store(cx.root, cx.network);
    let raw = vendors::owner(cx.network)?.export(
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
    let store = || vendors::owner_store(cx.root, cx.network);
    vendors::owner(cx.network)?.setup(&cx.provider_context(&store), request)
}
pub fn status(root: &Path, network: &str, rpc_url: &str, id: &str) -> Result<serde_json::Value> {
    let store = vendors::owner_store(root, network)?;
    vendors::owner(network)?.status(&store, network, rpc_url, id)
}
