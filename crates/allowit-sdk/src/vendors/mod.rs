//! Mechanical compiled registrations. Native semantics remain in vendor libraries.
//!
//! A vendor integrates with one line in [`registered!`], gated by its Cargo feature, and an adapter
//! module that reexports its provider. Every dispatch below goes through that list, so no function
//! in this file names a vendor.
#[cfg(feature = "stellar")]
pub mod stellar;
use crate::{
    error::{Error, Result},
    routing,
};
pub(crate) use allowit_core::vendor::{KeyProvider, OwnerProvider, PrivateSigner, RuntimeProvider};

/// The compiled vendors, in build order. A vendor integrates with one line here.
macro_rules! registered {
    ($then:ident! { $($args:tt)* }) => {
        $then! { [
            #[cfg(feature = "stellar")] stellar,
        ] $($args)* }
    };
}

/// Runs `$body` with `$v` bound to the adapter module of the vendor that owns `$family`.
macro_rules! by_family {
    ([$(#[$cfg:meta] $vendor:ident,)*] $family:expr, $v:ident => $body:expr, $none:expr) => {
        match $family {
            $(#[$cfg] self::$vendor::FAMILY => {
                use self::$vendor as $v;
                $body
            })*
            _ => $none,
        }
    };
}
macro_rules! dispatch {
    ($family:expr, $v:ident => $body:expr) => {
        registered!(by_family! { $family, $v => $body, Err(unavailable()) })
    };
    ($family:expr, $v:ident => $body:expr, else $none:expr) => {
        registered!(by_family! { $family, $v => $body, $none })
    };
}

/// The networks of every compiled vendor.
macro_rules! all_networks {
    ([$(#[$cfg:meta] $vendor:ident,)*]) => {{
        #[allow(unused_mut)]
        let mut networks = Vec::new();
        $(#[$cfg] networks.extend(self::$vendor::networks());)*
        networks
    }};
}

/// The result of the first vendor that accepts an input which names no network.
macro_rules! first_accepting {
    ([$(#[$cfg:meta] $vendor:ident,)*] $v:ident => $body:expr) => {{
        #[allow(unused_mut)]
        let mut result = Err(unavailable());
        $(#[$cfg] if result.is_err() {
            use self::$vendor as $v;
            result = $body;
        })*
        result
    }};
}

fn unavailable() -> Error {
    Error::unsupported("The selected build has no matching vendor capability")
}
pub(crate) fn networks() -> Vec<routing::RailInfo> {
    registered!(all_networks! {})
}
pub(crate) fn keys(network: &str) -> Result<&'static dyn KeyProvider> {
    dispatch!(routing::family(network)?, v => Ok(&v::KEYS))
}
pub(crate) fn owner(network: &str) -> Result<&'static dyn OwnerProvider> {
    dispatch!(routing::family(network)?, v => Ok(&v::owner::OWNER))
}

#[cfg_attr(
    not(feature = "default"),
    allow(dead_code, reason = "a build may compile no vendor")
)]
fn open_provider_store(root: &std::path::Path, namespace: &str) -> Result<crate::Store> {
    crate::Store::open(&root.join(namespace))
}
pub(crate) fn owner_store(_root: &std::path::Path, network: &str) -> Result<crate::Store> {
    dispatch!(routing::family(network)?, v => open_provider_store(_root, v::OWNER_STORE_NAMESPACE))
}
pub(crate) fn generate(
    _backend: &str,
    _origin: &str,
    _signer: &crate::wallet::LocalSigner,
    request: &crate::generation::GenerateRequest,
    _clock: &mut dyn FnMut() -> Result<u64>,
) -> Result<crate::generation::GeneratedDraft> {
    dispatch!(routing::family(&request.network)?, v => {
        v::generate(_backend, _origin, _signer, &request.into(), _clock)
    })
}
pub(crate) fn formats(family: routing::Family) -> Result<&'static crate::input::Formats> {
    dispatch!(family, v => Ok(&v::formats::FORMATS))
}
pub(crate) type ProfileBinding = Box<dyn allowit_core::vendor::profiles::ProfileBinding>;
pub(crate) fn decode_profile(
    network: &str,
    _bytes: &[u8],
) -> Result<(crate::profiles::Description, ProfileBinding)> {
    dispatch!(routing::family(network)?, v => v::profiles::decode(network, _bytes))
}
pub(crate) fn execution_tokens(network: &str) -> Result<&'static [crate::input::asset::Token]> {
    let network = routing::normalize(network)?;
    dispatch!(routing::family(network)?, v => Ok(v::tokens_for(network)))
}
pub(crate) fn check_execution(
    network: &str,
    _op: &str,
    _to: &str,
    _minimum: Option<&crate::input::asset::Amount>,
    _args: bool,
) -> Result<()> {
    dispatch!(routing::family(network)?, v => v::request::check(_op, _to, _minimum, _args))
}
pub(crate) fn build_execution(
    p: &crate::profiles::Profile,
    _root: &std::path::Path,
    _rpc: Option<&str>,
    _r: &crate::execution::Request,
) -> Result<serde_json::Value> {
    dispatch!(routing::family(&p.description().network)?, v => v::execution::build(
        &p.inner,
        &|| open_provider_store(_root, v::EXECUTION_STORE_NAMESPACE),
        _rpc,
        _r,
    ))
}
pub(crate) fn execute(
    p: crate::profiles::Profile,
    _s: crate::wallet::LocalSigner,
    _root: &std::path::Path,
    _rpc: Option<String>,
    _r: &crate::execution::Request,
) -> Result<crate::execution::Outcome> {
    dispatch!(routing::family(&p.description().network)?, v => v::execution::execute(
        p.inner,
        std::sync::Arc::new(_s),
        &|| open_provider_store(_root, v::EXECUTION_STORE_NAMESPACE),
        _rpc,
        _r,
    ))
}
pub(crate) fn execution_status(
    p: crate::profiles::Profile,
    _s: crate::wallet::LocalSigner,
    _root: &std::path::Path,
    _rpc: Option<String>,
    _id: &str,
    _wait: std::time::Duration,
) -> Result<crate::execution::Outcome> {
    dispatch!(routing::family(&p.description().network)?, v => v::execution::status(
        p.inner,
        std::sync::Arc::new(_s),
        &|| open_provider_store(_root, v::EXECUTION_STORE_NAMESPACE),
        _rpc,
        _id,
        _wait,
    ))
}
pub(crate) fn capabilities(network: &str) -> Result<crate::execution::Capabilities> {
    dispatch!(routing::family(network)?, v => Ok(v::capabilities(network)))
}
pub(crate) fn fund_key(network: &str, _address: &str, _endpoint: Option<&str>) -> Result<String> {
    dispatch!(routing::family(network)?, v => v::fund_key(network, _address, _endpoint))
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
    dispatch!(routing::family(network)?, v => v::execution::retained(
        &|| open_provider_store(_root, v::EXECUTION_STORE_NAMESPACE),
        network,
        _wallet,
        _id,
    ))
}
/// The vendor of a retained operation is the one whose wallet format accepts its wallet.
pub(crate) fn recover(
    original: crate::execution::Recovery,
    _signer: crate::wallet::LocalSigner,
    _backend: &str,
    _root: &std::path::Path,
    _id: &str,
    _wait: std::time::Duration,
    _request: Option<&crate::execution::Request>,
) -> Result<crate::execution::Outcome> {
    let wallet = original.binding().wallet;
    let family = routing::all()
        .iter()
        .map(|rail| rail.family)
        .find(|family| crate::input::address::check_wallet(*family, &wallet).is_ok())
        .ok_or_else(unavailable)?;
    dispatch!(family, v => v::execution::recover(
        original,
        std::sync::Arc::new(_signer),
        _backend,
        &|| open_provider_store(_root, v::EXECUTION_STORE_NAMESPACE),
        _id,
        _wait,
        _request,
    ))
}

pub(crate) fn runtime(network: &str, _endpoint: &str) -> Result<Box<dyn RuntimeProvider>> {
    dispatch!(routing::family(network)?, v => {
        Ok(Box::new(v::server::Provider::new(network, _endpoint)?))
    })
}
pub(crate) fn runtime_with_transport(
    network: &str,
    _transport: std::sync::Arc<dyn crate::runtime::Transport>,
) -> Result<Box<dyn RuntimeProvider>> {
    dispatch!(routing::family(network)?, v => {
        Ok(Box::new(v::server::Provider::with_transport(network, _transport)?))
    })
}
pub(crate) fn runtime_policy(
    i: crate::runtime::PolicyInstallation,
) -> Result<allowit_core::vendor::runtime::Installation> {
    dispatch!(routing::family(&i.description.network)?, v => v::server::policy(i))
}
/// A stored packet names its network inside the vendor's own document, so each vendor tries it.
pub(crate) fn runtime_restore(
    _k: crate::runtime::Kind,
    _v: &serde_json::value::RawValue,
) -> Result<crate::runtime::Packet> {
    registered!(first_accepting! { v => v::server::restore(_k, _v.to_owned()) })
}

pub(crate) fn owner_configuration(
    network: &str,
    _document: &[u8],
) -> Result<allowit_core::vendor::runtime::owner::Configuration> {
    dispatch!(routing::family(network)?, v => v::server_owner::configuration(network, _document))
}
pub(crate) fn owner_envelope_hash(
    network: &str,
    _payload: &[u8],
    _require_signature: bool,
) -> Result<[u8; 32]> {
    dispatch!(routing::family(network)?, v => {
        v::server_owner::envelope_hash(network, _payload, _require_signature)
    })
}
pub(crate) fn owner_validate_signed(
    network: &str,
    _original: &[u8],
    _signed: &[u8],
) -> Result<[u8; 32]> {
    dispatch!(routing::family(network)?, v => {
        v::server_owner::validate_signed(network, _original, _signed)
    })
}
pub(crate) fn owner_with_policy(
    c: &allowit_core::vendor::runtime::owner::Configuration,
    _i: crate::runtime::owner::PolicyIdentity,
) -> Result<allowit_core::vendor::runtime::owner::Configuration> {
    dispatch!(routing::family(&c.network)?, v => v::server_owner::with_policy(c, _i))
}

pub(crate) fn owner_code_hash(network: &str, _wasm: &[u8]) -> Result<[u8; 32]> {
    dispatch!(routing::family(network)?, v => v::server_owner::code_hash(_wasm))
}
pub(crate) fn owner_connect(
    network: &str,
    _endpoint: &str,
) -> Result<std::sync::Arc<dyn crate::runtime::Transport>> {
    dispatch!(routing::family(network)?, v => v::server_owner::connect(_endpoint))
}

pub(crate) fn owner_fee_bounds(network: &str) -> Result<crate::runtime::owner::FeeBounds> {
    dispatch!(routing::family(network)?, v => Ok(v::server_owner::fee_bounds()))
}
pub(crate) fn owner_explorer(network: &str, _kind: &str, _id: &str) -> Option<String> {
    dispatch!(routing::family(network).ok()?, v => {
        v::server_owner::explorer_link(network, _kind, _id)
    }, else None)
}
pub(crate) fn owner_wallet_address(
    network: &str,
    _owner: &str,
    _instance: &[u8; 32],
) -> Result<String> {
    dispatch!(routing::family(network)?, v => {
        v::server_owner::wallet_address(network, _owner, _instance)
    })
}

pub(crate) fn owner_custody_encoding(
    network: &str,
    _c: crate::runtime::owner::Custody,
) -> Result<serde_json::Value> {
    dispatch!(routing::family(network)?, v => v::server_owner::custody_encoding(_c))
}
pub(crate) fn owner_tier_encoding(
    network: &str,
    _t: Option<crate::runtime::Tier>,
) -> Result<serde_json::Value> {
    dispatch!(routing::family(network)?, v => v::server_owner::tier_encoding(_t))
}

pub(crate) fn runtime_request_proof_protocol(
    network: &str,
) -> Result<crate::request_proof::Protocol> {
    dispatch!(routing::family(network)?, v => Ok(v::server::request_proof_protocol()))
}

pub(crate) fn runtime_signer_identity(network: &str, _key: &[u8; 32]) -> Result<String> {
    dispatch!(routing::family(network)?, v => v::server::signer_identity(_key))
}

pub(crate) fn runtime_request_id(p: &crate::runtime::Packet) -> Result<[u8; 32]> {
    dispatch!(routing::family(p.network())?, v => v::server::request_id(p))
}

pub(crate) fn owner_source_consent<P: serde::Serialize>(
    _input: &crate::runtime::owner::SourceConsent<'_, P>,
) -> Result<(serde_json::Value, String)> {
    dispatch!(routing::family(&_input.configuration.network)?, v => {
        v::server_owner::source_consent(&allowit_core::vendor::runtime::owner::SourceConsent {
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
        })
    })
}
