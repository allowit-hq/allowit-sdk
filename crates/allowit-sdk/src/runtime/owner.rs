//! Public installation metadata. Decoding describes retained data and grants no signing authority.
use crate::{error::Result, runtime::Tier};

pub use allowit_core::vendor::plugin::{LinkKind, OwnerStep};
pub use allowit_core::vendor::runtime::owner::{
    ConfigurationInfo, Custody, FeeBounds, PolicyIdentity,
};
/// Literal retained configuration with read-only common metadata. Serialization keeps original native bytes.
#[derive(Clone, Debug)]
pub struct Configuration(pub(crate) allowit_core::vendor::runtime::owner::Configuration);
impl std::ops::Deref for Configuration {
    type Target = ConfigurationInfo;
    fn deref(&self) -> &Self::Target {
        self.0.info()
    }
}
impl PartialEq for Configuration {
    fn eq(&self, other: &Self) -> bool {
        self.0.document().get() == other.0.document().get()
    }
}
impl Eq for Configuration {}
impl Configuration {
    pub fn bytes(&self) -> &[u8] {
        self.0.document().get().as_bytes()
    }
    pub fn policy(input: crate::runtime::PolicyInstallation) -> Result<Self> {
        let installation = crate::runtime::Installation::policy_wallet(input)?;
        configuration(
            &installation.description().network,
            installation.native_document(),
        )
    }
}
impl serde::Serialize for Configuration {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        self.0.document().serialize(s)
    }
}
impl<'de> serde::Deserialize<'de> for Configuration {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = <Box<serde_json::value::RawValue> as serde::Deserialize>::deserialize(d)?;
        #[derive(serde::Deserialize)]
        struct Identity {
            network: String,
        }
        let identity: Identity =
            serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
        configuration(&identity.network, raw.get().as_bytes()).map_err(serde::de::Error::custom)
    }
}
/// Describe a literal retained configuration. Source consent and live authority are separate checks.
pub fn configuration(network: &str, document: &[u8]) -> Result<Configuration> {
    let network = crate::routing::normalize(network)?;
    crate::plugins::for_network(network)?
        .owner_runtime()
        .configuration(network, document)
        .map(Configuration)
}
/// Bind owner packet identity without granting permission to sign or broadcast it.
pub fn envelope_hash(network: &str, payload: &[u8], require_signature: bool) -> Result<[u8; 32]> {
    let network = crate::routing::normalize(network)?;
    crate::plugins::for_network(network)?
        .owner_runtime()
        .envelope_hash(network, payload, require_signature)
}
/// Validate an external signature against a literal retained original owner packet.
/// Current authority, expiry and settlement remain separate operations.
pub fn validate_signed(network: &str, original: &[u8], signed: &[u8]) -> Result<[u8; 32]> {
    let network = crate::routing::normalize(network)?;
    crate::plugins::for_network(network)?
        .owner_runtime()
        .validate_signed(network, original, signed)
}
impl Configuration {
    pub fn with_policy(&self, identity: PolicyIdentity) -> Result<Self> {
        crate::plugins::for_network(&self.network)?
            .owner_runtime()
            .with_policy(&self.0, identity)
            .map(Configuration)
    }
}
impl crate::runtime::Installation {
    /// Restore an already admitted opaque configuration for read/owner operations.
    /// Native delegated signing still requires independently pinned source consent through Runtime::import.
    pub fn from_configuration(c: &Configuration) -> Self {
        Self(allowit_core::vendor::runtime::Installation {
            description: crate::profiles::Description {
                network: c.network.clone(),
                wallet: c.wallet.clone(),
                owner: c.owner.clone(),
                agent: c.agent.clone(),
                server: c.server.clone(),
                policy_id: String::new(),
                asset: c.asset.clone(),
                asset_symbol: c.asset_symbol.clone(),
                source: c.source.clone(),
                source_hash: c.source_hash.iter().map(|b| format!("{b:02x}")).collect(),
                module_digest: c.module_digest.iter().map(|b| format!("{b:02x}")).collect(),
                params: c.params.clone(),
            },
            config: c.0.document().to_owned(),
            export: None,
            authority: None,
        })
    }
}
/// Check native deployment code before exposing an owner preparation.
pub fn wallet_code_hash(network: &str, wasm: &[u8]) -> Result<[u8; 32]> {
    crate::plugins::for_network(network)?
        .owner_runtime()
        .code_hash(wasm)
}
/// Connect the selected native protocol through the shared bounded transport.
pub fn connect(
    network: &str,
    endpoint: &str,
) -> Result<std::sync::Arc<dyn crate::runtime::Transport>> {
    let network = crate::routing::normalize(network)?;
    crate::plugins::for_network(network)?.connect(network, endpoint)
}
pub fn fee_bounds(network: &str) -> Result<FeeBounds> {
    Ok(crate::plugins::for_network(network)?
        .owner_runtime()
        .fee_bounds())
}
pub fn explorer_link(network: &str, kind: LinkKind, id: &str) -> Option<String> {
    let network = crate::routing::normalize(network).ok()?;
    crate::plugins::for_network(network)
        .ok()?
        .explorer(network, kind, id)
}
pub fn wallet_address(network: &str, owner: &str, instance: &[u8; 32]) -> Result<String> {
    let network = crate::routing::normalize(network)?;
    crate::plugins::for_network(network)?
        .owner_runtime()
        .wallet_address(network, owner, instance)
}

pub fn custody_encoding(network: &str, custody: Custody) -> Result<serde_json::Value> {
    crate::plugins::for_network(network)?
        .owner_runtime()
        .custody_encoding(custody)
}
pub fn tier_encoding(network: &str, tier: Option<Tier>) -> Result<serde_json::Value> {
    crate::plugins::for_network(network)?
        .owner_runtime()
        .tier_encoding(tier)
}

/// Literal source-consent inputs. The native codec owns field names and the signing domain.
/// Encoding these values does not approve a policy or establish wallet authority.
pub struct SourceConsent<'a, P: serde::Serialize> {
    pub configuration: &'a Configuration,
    pub policy_id: &'a str,
    pub revision: u32,
    pub source: &'a str,
    pub source_hash: &'a str,
    pub ir_hash: &'a str,
    pub module_digest: &'a str,
    pub language: &'a str,
    pub tier: &'a str,
    pub rules: &'a [String],
    pub params: &'a P,
    pub params_hash: &'a str,
}
pub fn source_consent<P: serde::Serialize>(
    input: &SourceConsent<'_, P>,
) -> Result<(serde_json::Value, String)> {
    crate::plugins::source_consent(input)
}

/// The rail-neutral step that the owner signs for a prepared owner packet.
pub fn step(packet: &crate::runtime::OwnerPacket) -> Result<OwnerStep> {
    let network = packet.network();
    crate::plugins::for_network(network)?.owner_runtime().step(
        network,
        &allowit_core::vendor::runtime::OwnerPacket(packet.0.clone()),
    )
}
