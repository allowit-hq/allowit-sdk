//! Mechanical compiled registrations. Native semantics remain in vendor libraries.
#[cfg(feature = "stellar")]
mod stellar;
use crate::{
    error::{Error, Result},
    routing,
};
use zeroize::Zeroizing;
pub(crate) trait PrivateSigner: Send + Sync {
    fn address(&self) -> &str;
    fn public_key(&self) -> &str;
    fn key_type(&self) -> &str;
    fn encoded_secret(&self) -> Zeroizing<String>;
    fn sign_hash(&self, digest: &[u8]) -> Result<Vec<u8>>;
}

pub(crate) trait KeyProvider: Sync {
    fn entropy_bytes(&self) -> usize;
    fn derive(&self, entropy: &[u8]) -> Result<Box<dyn PrivateSigner>>;
    fn parse(&self, secret: &str) -> Result<Box<dyn PrivateSigner>>;
    fn validate_identity(&self, address: &str, public_key: &str, key_type: &str) -> Result<()>;
}

pub(crate) trait OwnerProvider {
    fn deploy(
        &self,
        cx: &crate::owner::Context<'_>,
        r: crate::owner::Deploy<'_>,
    ) -> Result<serde_json::Value>;
    fn fund(
        &self,
        cx: &crate::owner::Context<'_>,
        document: &[u8],
        id: &str,
        amount: &str,
        fee: &str,
    ) -> Result<serde_json::Value>;
    fn activate(
        &self,
        cx: &crate::owner::Context<'_>,
        r: crate::owner::Activate<'_>,
    ) -> Result<serde_json::Value>;
    fn export(
        &self,
        cx: &crate::owner::Context<'_>,
        document: &[u8],
        id: &str,
    ) -> Result<crate::profiles::Profile>;
    fn setup(
        &self,
        cx: &crate::owner::Context<'_>,
        r: crate::owner::Setup<'_>,
    ) -> Result<serde_json::Value>;
    fn status(
        &self,
        root: &std::path::Path,
        network: &str,
        rpc: &str,
        id: &str,
    ) -> Result<serde_json::Value>;
}

fn unavailable() -> Error {
    Error::unsupported("The selected build has no matching vendor capability")
}
pub(crate) fn networks() -> Vec<routing::RailInfo> {
    #[cfg(feature = "stellar")]
    {
        stellar::networks()
    }
    #[cfg(not(feature = "stellar"))]
    {
        Vec::new()
    }
}
pub(crate) fn keys(network: &str) -> Result<&'static dyn KeyProvider> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => Ok(&stellar::KEYS),
        _ => Err(unavailable()),
    }
}
pub(crate) fn owner(network: &str) -> Result<&'static dyn OwnerProvider> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => Ok(&stellar::owner::OWNER),
        _ => Err(unavailable()),
    }
}
pub(crate) fn generate(
    _backend: &str,
    _origin: &str,
    _signer: &crate::wallet::LocalSigner,
    request: &crate::generation::GenerateRequest,
    _clock: &mut dyn FnMut() -> Result<u64>,
) -> Result<crate::generation::GeneratedDraft> {
    match routing::family(&request.network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::generate(_backend, _origin, _signer, request, _clock),
        _ => Err(unavailable()),
    }
}
pub(crate) fn formats(family: routing::Family) -> Result<&'static crate::input::Formats> {
    match family {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => Ok(&stellar::formats::FORMATS),
        _ => Err(unavailable()),
    }
}
pub(crate) enum ProfileBinding {
    #[cfg(feature = "stellar")]
    Stellar(crate::stellar::client::Bundle),
}
pub(crate) fn decode_profile(
    network: &str,
    _bytes: &[u8],
) -> Result<(crate::profiles::Description, ProfileBinding)> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::profiles::decode(network, _bytes),
        _ => Err(unavailable()),
    }
}
pub(crate) fn execution_tokens(network: &str) -> Result<&'static [crate::input::asset::Token]> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => Ok(stellar::tokens_for(network)),
        _ => Err(unavailable()),
    }
}
pub(crate) fn check_execution(
    network: &str,
    _op: &str,
    _to: &str,
    _minimum: Option<&crate::input::asset::Amount>,
    _args: bool,
) -> Result<()> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::request::check(_op, _to, _minimum, _args),
        _ => Err(unavailable()),
    }
}
pub(crate) fn build_execution(
    p: &crate::profiles::Profile,
    _root: &std::path::Path,
    _rpc: Option<&str>,
    _r: &crate::execution::Request,
) -> Result<serde_json::Value> {
    match routing::family(&p.description().network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::execution::build(p, _root, _rpc, _r),
        _ => Err(unavailable()),
    }
}
pub(crate) fn execute(
    p: crate::profiles::Profile,
    _s: crate::wallet::LocalSigner,
    _root: &std::path::Path,
    _rpc: Option<String>,
    _r: &crate::execution::Request,
) -> Result<crate::execution::Outcome> {
    match routing::family(&p.description().network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::execution::execute(p, _s, _root, _rpc, _r),
        _ => Err(unavailable()),
    }
}
pub(crate) fn execution_status(
    p: crate::profiles::Profile,
    _s: crate::wallet::LocalSigner,
    _root: &std::path::Path,
    _rpc: Option<String>,
    _id: &str,
    _wait: std::time::Duration,
) -> Result<crate::execution::Outcome> {
    match routing::family(&p.description().network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::execution::status(p, _s, _root, _rpc, _id, _wait),
        _ => Err(unavailable()),
    }
}
pub(crate) fn capabilities(network: &str) -> Result<crate::execution::Capabilities> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => Ok(stellar::capabilities(network)),
        _ => Err(unavailable()),
    }
}
pub(crate) fn fund_key(network: &str, _address: &str, _endpoint: Option<&str>) -> Result<String> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::fund_key(network, _address, _endpoint),
        _ => Err(unavailable()),
    }
}

pub(crate) fn verify_profile_import(_binding: &ProfileBinding, _endpoint: &str) -> Result<()> {
    #[cfg(feature = "stellar")]
    {
        let ProfileBinding::Stellar(b) = _binding;
        b.verify_import(_endpoint)
    }
    #[cfg(not(feature = "stellar"))]
    {
        Err(unavailable())
    }
}
pub(crate) fn status_binding(
    p: &crate::profiles::Profile,
    _root: &std::path::Path,
    _id: &str,
) -> Result<crate::execution::Binding> {
    match routing::family(&p.description().network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::execution::status_binding(p, _root, _id),
        _ => Err(unavailable()),
    }
}
