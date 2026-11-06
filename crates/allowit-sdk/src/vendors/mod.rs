//! Mechanical compiled registrations. Native semantics remain in vendor libraries.
#[cfg(feature = "stellar")]
pub mod stellar;
use crate::{
    error::{Error, Result},
    routing,
};
pub(crate) use allowit_core::vendor::{KeyProvider, OwnerProvider, PrivateSigner, RuntimeProvider};
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

#[cfg(feature = "stellar")]
fn open_provider_store(root: &std::path::Path, namespace: &str) -> Result<crate::Store> {
    crate::Store::open(&root.join(namespace))
}
pub(crate) fn owner_store(root: &std::path::Path, network: &str) -> Result<crate::Store> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => open_provider_store(root, stellar::OWNER_STORE_NAMESPACE),
        _ => {
            let _ = root;
            Err(unavailable())
        }
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
        stellar::FAMILY => stellar::generate(_backend, _origin, _signer, &request.into(), _clock),
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
pub(crate) type ProfileBinding = Box<dyn allowit_core::vendor::profiles::ProfileBinding>;
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
    let network = routing::normalize(network)?;
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
        stellar::FAMILY => stellar::execution::build(
            &p.inner,
            &|| open_provider_store(_root, stellar::EXECUTION_STORE_NAMESPACE),
            _rpc,
            _r,
        ),
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
        stellar::FAMILY => stellar::execution::execute(
            p.inner,
            std::sync::Arc::new(_s),
            &|| open_provider_store(_root, stellar::EXECUTION_STORE_NAMESPACE),
            _rpc,
            _r,
        ),
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
        stellar::FAMILY => stellar::execution::status(
            p.inner,
            std::sync::Arc::new(_s),
            &|| open_provider_store(_root, stellar::EXECUTION_STORE_NAMESPACE),
            _rpc,
            _id,
            _wait,
        ),
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

pub(crate) fn verify_profile_import(binding: &ProfileBinding, endpoint: &str) -> Result<()> {
    binding.verify_import(endpoint)
}
pub(crate) fn verify_server_role(binding: &ProfileBinding, server: &str) -> Result<()> {
    binding.verify_server_role(server)
}
pub(crate) fn retained(
    _root: &std::path::Path,
    network: &str,
    _wallet: Option<&str>,
    _id: &str,
) -> Result<Option<crate::execution::Recovery>> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::execution::retained(
            &|| open_provider_store(_root, stellar::EXECUTION_STORE_NAMESPACE),
            network,
            _wallet,
            _id,
        ),
        _ => Err(unavailable()),
    }
}
pub(crate) fn recover(
    original: crate::execution::Recovery,
    _signer: crate::wallet::LocalSigner,
    _backend: &str,
    _root: &std::path::Path,
    _id: &str,
    _wait: std::time::Duration,
    _request: Option<&crate::execution::Request>,
) -> Result<crate::execution::Outcome> {
    #[cfg(feature = "stellar")]
    {
        stellar::execution::recover(
            original,
            std::sync::Arc::new(_signer),
            _backend,
            &|| open_provider_store(_root, stellar::EXECUTION_STORE_NAMESPACE),
            _id,
            _wait,
            _request,
        )
    }
    #[cfg(not(feature = "stellar"))]
    {
        let _ = original;
        Err(unavailable())
    }
}

pub(crate) fn runtime(network: &str, _endpoint: &str) -> Result<Box<dyn RuntimeProvider>> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => Ok(Box::new(stellar::server::Provider::new(
            network, _endpoint,
        )?)),
        _ => Err(unavailable()),
    }
}
pub(crate) fn runtime_with_transport(
    network: &str,
    _transport: std::sync::Arc<dyn crate::runtime::Transport>,
) -> Result<Box<dyn RuntimeProvider>> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => Ok(Box::new(stellar::server::Provider::with_transport(
            network, _transport,
        )?)),
        _ => Err(unavailable()),
    }
}
pub(crate) fn runtime_policy(
    i: crate::runtime::PolicyInstallation,
) -> Result<allowit_core::vendor::runtime::Installation> {
    match routing::family(&i.description.network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::server::policy(i),
        _ => Err(unavailable()),
    }
}
pub(crate) fn runtime_restore(
    k: crate::runtime::Kind,
    v: Box<serde_json::value::RawValue>,
) -> Result<crate::runtime::Packet> {
    #[cfg(feature = "stellar")]
    {
        stellar::server::restore(k, v)
    }
    #[cfg(not(feature = "stellar"))]
    {
        let _ = (k, v);
        Err(unavailable())
    }
}

pub(crate) fn owner_configuration(
    network: &str,
    _document: &[u8],
) -> Result<allowit_core::vendor::runtime::owner::Configuration> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::server_owner::configuration(network, _document),
        _ => Err(unavailable()),
    }
}
pub(crate) fn owner_envelope_hash(
    network: &str,
    _payload: &[u8],
    _require_signature: bool,
) -> Result<[u8; 32]> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => {
            stellar::server_owner::envelope_hash(network, _payload, _require_signature)
        }
        _ => Err(unavailable()),
    }
}
pub(crate) fn owner_validate_signed(
    network: &str,
    _original: &[u8],
    _signed: &[u8],
) -> Result<[u8; 32]> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::server_owner::validate_signed(network, _original, _signed),
        _ => Err(unavailable()),
    }
}
pub(crate) fn owner_with_policy(
    c: &allowit_core::vendor::runtime::owner::Configuration,
    _i: crate::runtime::owner::PolicyIdentity,
) -> Result<allowit_core::vendor::runtime::owner::Configuration> {
    match routing::family(&c.network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::server_owner::with_policy(c, _i),
        _ => Err(unavailable()),
    }
}

pub(crate) fn owner_code_hash(network: &str, _wasm: &[u8]) -> Result<[u8; 32]> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::server_owner::code_hash(_wasm),
        _ => Err(unavailable()),
    }
}
pub(crate) fn owner_connect(
    network: &str,
    _endpoint: &str,
) -> Result<std::sync::Arc<dyn crate::runtime::Transport>> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::server_owner::connect(_endpoint),
        _ => Err(unavailable()),
    }
}

pub(crate) fn owner_fee_bounds(network: &str) -> Result<crate::runtime::owner::FeeBounds> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => Ok(stellar::server_owner::fee_bounds()),
        _ => Err(unavailable()),
    }
}
pub(crate) fn owner_explorer(network: &str, _kind: &str, _id: &str) -> Option<String> {
    match routing::family(network).ok()? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::server_owner::explorer_link(network, _kind, _id),
        _ => None,
    }
}
pub(crate) fn owner_wallet_address(
    network: &str,
    _owner: &str,
    _instance: &[u8; 32],
) -> Result<String> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::server_owner::wallet_address(network, _owner, _instance),
        _ => Err(unavailable()),
    }
}

pub(crate) fn owner_custody_encoding(
    network: &str,
    _c: crate::runtime::owner::Custody,
) -> Result<serde_json::Value> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::server_owner::custody_encoding(_c),
        _ => Err(unavailable()),
    }
}
pub(crate) fn owner_tier_encoding(
    network: &str,
    _t: Option<crate::runtime::Tier>,
) -> Result<serde_json::Value> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::server_owner::tier_encoding(_t),
        _ => Err(unavailable()),
    }
}

pub(crate) fn runtime_request_proof_protocol(
    network: &str,
) -> Result<crate::request_proof::Protocol> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => Ok(stellar::server::request_proof_protocol()),
        _ => Err(unavailable()),
    }
}

pub(crate) fn runtime_signer_identity(network: &str, _key: &[u8; 32]) -> Result<String> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::server::signer_identity(_key),
        _ => Err(unavailable()),
    }
}

pub(crate) fn runtime_request_id(p: &crate::runtime::Packet) -> Result<[u8; 32]> {
    match routing::family(p.network())? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::server::request_id(p),
        _ => Err(unavailable()),
    }
}

pub(crate) fn owner_source_consent<P: serde::Serialize>(
    _input: &crate::runtime::owner::SourceConsent<'_, P>,
) -> Result<(serde_json::Value, String)> {
    match routing::family(&_input.configuration.network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::server_owner::source_consent(
            &allowit_core::vendor::runtime::owner::SourceConsent {
                configuration: &_input.configuration.0,
                policy_id: _input.policy_id,
                revision: _input.revision,
                source: _input.source,
                source_hash: _input.source_hash,
                ir_hash: _input.ir_hash,
                module_digest: _input.module_digest,
                language: _input.language,
                tier: _input.tier,
                rules: _input.rules,
                params: _input.params,
                params_hash: _input.params_hash,
            },
        ),
        _ => Err(unavailable()),
    }
}
