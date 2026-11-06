//! Public installation metadata. Decoding describes retained data and grants no signing authority.
use crate::{error::Result, runtime::Tier};
use allowit_core::policy::PolicyValue;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Custody {
    Owner,
    Policy,
    Retired,
}
#[derive(Clone, Debug)]
pub struct ConfigurationInfo {
    pub network: String,
    pub wallet: String,
    pub owner: String,
    pub agent: String,
    pub server: String,
    pub source: String,
    pub source_hash: [u8; 32],
    pub binding_hash: [u8; 32],
    pub rules: Vec<String>,
    pub is_retired: bool,
    pub module_digest: [u8; 32],
    pub policy_instance: [u8; 32],
    pub asset: String,
    pub asset_symbol: String,
    pub asset_issuer: Option<String>,
    pub max_fee_units: u64,
    pub is_native_asset: bool,
    pub fee_decimals: u8,
    pub payment_asset_decimals: u32,
    pub params: BTreeMap<String, PolicyValue>,
    pub custody: Custody,
    pub custody_encoding: serde_json::Value,
    pub tier_encoding: serde_json::Value,
    pub tier: Option<Tier>,
}
/// Literal retained configuration with read-only common metadata. Serialization keeps original native bytes.
#[derive(Clone, Debug)]
pub struct Configuration {
    pub(crate) info: ConfigurationInfo,
    pub(crate) document: Box<serde_json::value::RawValue>,
}
impl std::ops::Deref for Configuration {
    type Target = ConfigurationInfo;
    fn deref(&self) -> &Self::Target {
        &self.info
    }
}
impl PartialEq for Configuration {
    fn eq(&self, other: &Self) -> bool {
        self.document.get() == other.document.get()
    }
}
impl Eq for Configuration {}
impl Configuration {
    pub fn bytes(&self) -> &[u8] {
        self.document.get().as_bytes()
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
        self.document.serialize(s)
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
    crate::vendors::owner_configuration(network, document)
}
/// Bind owner packet identity without granting permission to sign or broadcast it.
pub fn envelope_hash(network: &str, payload: &[u8], require_signature: bool) -> Result<[u8; 32]> {
    crate::vendors::owner_envelope_hash(network, payload, require_signature)
}
/// Validate an external signature against a literal retained original owner packet.
/// Current authority, expiry and settlement remain separate operations.
pub fn validate_signed(network: &str, original: &[u8], signed: &[u8]) -> Result<[u8; 32]> {
    crate::vendors::owner_validate_signed(network, original, signed)
}
/// Caller-admitted policy identity for a new owner-custody projection. This is not source consent.
pub struct PolicyIdentity {
    pub policy_instance: [u8; 32],
    pub module_digest: [u8; 32],
    pub source: String,
    pub params: BTreeMap<String, PolicyValue>,
    pub asset: String,
    pub asset_symbol: String,
    pub asset_issuer: Option<String>,
    pub agent: String,
    pub server: String,
    pub max_fee_units: u64,
}
impl Configuration {
    pub fn with_policy(&self, identity: PolicyIdentity) -> Result<Self> {
        crate::vendors::owner_with_policy(self, identity)
    }
}
impl crate::runtime::Installation {
    /// Restore an already admitted opaque configuration for read/owner operations.
    /// Native delegated signing still requires independently pinned source consent through Runtime::import.
    pub fn from_configuration(c: &Configuration) -> Self {
        Self {
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
            config: c.document.clone(),
            export: None,
            authority: None,
        }
    }
}
/// Check native deployment code before exposing an owner preparation.
pub fn wallet_code_hash(network: &str, wasm: &[u8]) -> Result<[u8; 32]> {
    crate::vendors::owner_code_hash(network, wasm)
}
/// Connect the selected native protocol through the shared bounded transport.
pub fn connect(
    network: &str,
    endpoint: &str,
) -> Result<std::sync::Arc<dyn crate::runtime::Transport>> {
    crate::vendors::owner_connect(network, endpoint)
}
#[derive(Clone, Debug)]
pub struct FeeBounds {
    pub minimum: u64,
    pub maximum_execution: u64,
    pub default_execution: u64,
    pub default_deployment: u64,
    pub decimals: u8,
    pub native_symbol: String,
}
pub fn fee_bounds(network: &str) -> Result<FeeBounds> {
    crate::vendors::owner_fee_bounds(network)
}
pub fn explorer_link(network: &str, kind: &str, id: &str) -> Option<String> {
    crate::vendors::owner_explorer(network, kind, id)
}
pub fn wallet_address(network: &str, owner: &str, instance: &[u8; 32]) -> Result<String> {
    crate::vendors::owner_wallet_address(network, owner, instance)
}

pub fn custody_encoding(network: &str, custody: Custody) -> Result<serde_json::Value> {
    crate::vendors::owner_custody_encoding(network, custody)
}
pub fn tier_encoding(network: &str, tier: Option<Tier>) -> Result<serde_json::Value> {
    crate::vendors::owner_tier_encoding(network, tier)
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
    crate::vendors::owner_source_consent(input)
}
