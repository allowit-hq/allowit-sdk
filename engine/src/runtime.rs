//! Local rail communication for server callers. Private admission and key authority remain with the caller.
pub mod owner;
use crate::{config::ServerAuthority, error::Result, profiles::Description};
use allowit_sdk::policy::PolicyValue;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::{collections::BTreeMap, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tier {
    Simple,
    #[serde(rename = "common")]
    Standard,
    Custom,
}
pub struct PolicyInstallation {
    pub description: Description,
    pub policy_instance: [u8; 32],
    pub tier: Tier,
    pub asset: crate::input::asset::Asset,
    pub max_fee_units: u64,
}
/// Immutable common identity with private native encoding. Construction grants no signing authority.
pub struct Installation {
    pub(crate) description: Description,
    pub(crate) config: Box<RawValue>,
    #[cfg_attr(not(feature = "stellar"), allow(dead_code))]
    pub(crate) export: Option<Box<RawValue>>,
    #[cfg_attr(not(feature = "stellar"), allow(dead_code))]
    pub(crate) authority: Option<ServerAuthority>,
}
impl Installation {
    pub fn policy_wallet(input: PolicyInstallation) -> Result<Self> {
        crate::vendors::runtime_policy(input)
    }
    pub fn description(&self) -> &Description {
        &self.description
    }
    /// Preserve the existing native installation record when the caller persists its own state.
    pub fn native_document(&self) -> &[u8] {
        self.config.get().as_bytes()
    }
}
#[derive(Clone, Debug)]
pub enum Action {
    Transfer {
        recipient: String,
        units: i128,
    },
    Deposit {
        vault: String,
        assets: i128,
        min_shares: i128,
    },
    Redeem {
        vault: String,
        shares: i128,
        min_assets: i128,
    },
}
#[derive(Clone, Debug, Default)]
pub struct Terms {
    pub args: BTreeMap<String, PolicyValue>,
    pub expense: Option<Expense>,
}
#[derive(Clone, Debug)]
pub struct Expense {
    pub gas_units: i128,
    pub conversion_units: i128,
    pub spent_units: i128,
    pub max_fee_units: u64,
    pub observed_height: u64,
    pub day: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Accounting {
    pub nonce: u64,
    #[serde(alias = "revision")]
    pub deployment_version: u64,
    pub spent_daily: i128,
    pub spent_total: i128,
    pub spent_day: u64,
    pub calls: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct Validity {
    pub now: u64,
    pub expires_at: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Prepared,
    Submitted,
    SubmissionUnknown,
    Confirmed,
    Failed,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signature {
    #[serde(rename = "signer")]
    pub identity: String,
    #[serde(rename = "signature")]
    pub bytes: Vec<u8>,
}
/// The caller owns the private key and decides when this capability is available.
pub trait Signer {
    fn identity(&self) -> &str;
    fn sign_hash(&self, digest: &[u8; 32]) -> Result<Vec<u8>>;
}
/// Existing portable Core evaluator. The server supplies only its admitted compiled policy.
pub type Evaluator<'a> = &'a dyn Fn(&allowit::AllowitContext) -> allowit::Result;

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Prepared,
    Signed,
    Receipt,
    Owner,
    OwnerSigned,
    Request,
}
#[derive(Clone, Debug)]
pub(crate) struct Packet {
    pub network: String,
    pub document: Box<RawValue>,
}
impl Packet {
    #[cfg_attr(not(feature = "stellar"), allow(dead_code))]
    pub(crate) fn new(network: &str, value: &impl Serialize) -> Result<Self> {
        let document = RawValue::from_string(
            serde_json::to_string(value)
                .map_err(|_| crate::error::Error::config("Cannot encode native packet"))?,
        )
        .map_err(|_| crate::error::Error::config("Cannot retain native packet"))?;
        Ok(Self {
            network: crate::routing::normalize(network)?.into(),
            document,
        })
    }
}
macro_rules! carrier {
    ($name:ident,$kind:ident) => {
        #[derive(Clone, Debug)]
        pub struct $name(pub(crate) Packet);
        impl $name {
            pub fn bytes(&self) -> &[u8] {
                self.0.document.get().as_bytes()
            }
            pub fn network(&self) -> &str {
                &self.0.network
            }
        }
        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(
                &self,
                s: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                self.0.document.serialize(s)
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(
                d: D,
            ) -> std::result::Result<Self, D::Error> {
                let raw = Box::<RawValue>::deserialize(d)?;
                crate::vendors::runtime_restore(Kind::$kind, raw)
                    .map(Self)
                    .map_err(serde::de::Error::custom)
            }
        }
    };
}
carrier!(Prepared, Prepared);
carrier!(Signed, Signed);
carrier!(Receipt, Receipt);
carrier!(OwnerPacket, Owner);
carrier!(OwnerSigned, OwnerSigned);
carrier!(RequestPacket, Request);
impl RequestPacket {
    pub fn operation_id(&self) -> Result<[u8; 32]> {
        crate::vendors::runtime_request_id(&self.0)
    }
}
impl PartialEq for RequestPacket {
    fn eq(&self, other: &Self) -> bool {
        self.network() == other.network()
            && serde_json::from_slice::<serde_json::Value>(self.bytes()).ok()
                == serde_json::from_slice::<serde_json::Value>(other.bytes()).ok()
    }
}

/// Native owner-signing input and common immutable transaction identity.
#[derive(Clone, Debug)]
pub struct OwnerInfo {
    pub action: String,
    pub signing_payload: Vec<u8>,
    pub transaction_hash: [u8; 32],
    pub expires_at: u64,
}
impl OwnerPacket {
    pub fn info(&self) -> Result<OwnerInfo> {
        crate::vendors::runtime_owner_info(&self.0)
    }
}
#[derive(Clone, Debug)]
pub struct PacketInfo {
    pub operation_id: [u8; 32],
    pub transaction_hash: [u8; 32],
    pub wallet: String,
    pub expires_at: u64,
}
/// Read-only common projection of a validated retained operation.
pub struct PreparedMetadata {
    pub configuration: owner::Configuration,
    pub request: RequestPacket,
    pub operation_id: [u8; 32],
    pub created_at: u64,
    pub expires_at: u64,
    pub transaction_hash: [u8; 32],
    pub debit_units: i128,
    pub request_hash: [u8; 32],
    pub debit: Debit,
}
#[derive(Clone, Debug)]
pub struct Debit {
    pub units: i128,
    pub token: String,
    pub recipient: String,
    pub kind: String,
    pub display_amount: String,
    pub display_token: String,
}
impl Prepared {
    pub fn metadata(&self) -> Result<PreparedMetadata> {
        crate::vendors::runtime_prepared_metadata(&self.0)
    }
    pub fn info(&self) -> Result<PacketInfo> {
        crate::vendors::runtime_info(&self.0, Kind::Prepared)
    }
}
impl Signed {
    pub fn validate_signatures(&self) -> Result<()> {
        crate::vendors::runtime_validate_signed(&self.0)
    }
    pub fn preparation(&self) -> Result<Prepared> {
        crate::vendors::runtime_signed_preparation(&self.0)
    }
    pub fn info(&self) -> Result<PacketInfo> {
        crate::vendors::runtime_info(&self.0, Kind::Signed)
    }
}
/// Values observed by the selected native implementation. This is not a complete economic-state commitment.
#[derive(Clone)]
pub struct Snapshot {
    pub accounting: Accounting,
    pub observed_height: u64,
    pub paused: bool,
    pub params: BTreeMap<String, PolicyValue>,
    pub state: BTreeMap<String, PolicyValue>,
    pub(crate) evidence: Packet,
}
impl Snapshot {
    pub fn verify_consent(&self, message: &str, signatures: &[Signature]) -> Result<()> {
        crate::vendors::runtime_verify_consent(self, message, signatures)
    }
    pub fn signer_weight(&self, identity: &str) -> Result<u32> {
        crate::vendors::runtime_signer_weight(self, identity)
    }
    pub fn authority_document(&self) -> Result<serde_json::Value> {
        crate::vendors::runtime_authority_document(self)
    }
    /// Compare only native known-state evidence. Current balance, lifetime and validity checks stay separate.
    pub fn same_known_state(&self, other: &Self) -> Result<bool> {
        crate::vendors::runtime_same_snapshot(self, other)
    }
}
#[derive(Clone, Debug)]
pub struct Lifetime {
    pub observed_height: u64,
    pub instance_until: u64,
    pub code_until: u64,
    pub extension_target: u64,
    pub needs_renewal: bool,
}
#[derive(Clone, Debug)]
pub struct Resources {
    pub code_present: bool,
    pub wallet_present: bool,
    pub lifetime: Option<Lifetime>,
}
#[derive(Clone, Debug)]
pub struct ReceiptInfo {
    pub transaction_hash: [u8; 32],
    pub status: Status,
    pub observed_height: u64,
}
impl Receipt {
    pub fn verify_operation(&self, signed: &Signed) -> Result<()> {
        crate::vendors::runtime_verify_receipt(&self.0, &signed.0)
    }
    pub fn info(&self) -> Result<ReceiptInfo> {
        crate::vendors::runtime_receipt_info(&self.0)
    }
}
/// Atomically reject a different signed packet under an existing operation ID.
pub trait OperationStore {
    fn load(&self, id: [u8; 32]) -> Result<Option<(Signed, Status)>>;
    fn record(&mut self, id: [u8; 32], signed: &Signed, status: Status) -> Result<()>;
}
pub struct ObservedAuthorization {
    pub signed: Signed,
    pub receipt: Receipt,
    pub status: Status,
}
#[derive(Clone, Debug)]
pub enum OwnerAction {
    Upload {
        wasm: Vec<u8>,
    },
    Create {
        wasm: Vec<u8>,
    },
    Fund {
        units: i128,
    },
    Extend,
    SetPause {
        paused: bool,
    },
    Recover {
        units: Option<i128>,
        recipient: Option<String>,
    },
}
#[derive(Clone, Copy, Debug)]
pub struct Poll {
    pub attempts: u32,
    pub interval: Duration,
}
impl Default for Poll {
    fn default() -> Self {
        Self {
            attempts: 30,
            interval: Duration::from_secs(2),
        }
    }
}

/// Caller-owned bounded RPC transport. Native methods and packet interpretation stay in the vendor.
pub trait Transport: Send + Sync {
    fn call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value>;
}

pub struct Runtime {
    pub(crate) provider: Box<dyn crate::vendors::RuntimeProvider>,
}
impl Runtime {
    pub fn discover_snapshot(
        &self,
        configuration: &owner::Configuration,
        accounting: Option<&Accounting>,
    ) -> Result<Snapshot> {
        self.provider.discover_snapshot(configuration, accounting)
    }
    pub fn current_height(&self) -> Result<u64> {
        self.provider.current_height()
    }
    pub fn observe_hash(&self, hash: &[u8; 32]) -> Result<Option<Receipt>> {
        self.provider.observe_hash(hash)
    }

    pub fn with_transport(network: &str, transport: std::sync::Arc<dyn Transport>) -> Result<Self> {
        crate::vendors::runtime_with_transport(network, transport).map(|provider| Self { provider })
    }
    pub fn new(network: &str, endpoint: &str) -> Result<Self> {
        crate::vendors::runtime(network, endpoint).map(|provider| Self { provider })
    }
    pub fn import(&self, authority: &ServerAuthority, bytes: &[u8]) -> Result<Installation> {
        self.provider.import(authority, bytes)
    }
    pub fn request(
        &self,
        i: &Installation,
        id: [u8; 32],
        action: Action,
        accounting: Option<&Accounting>,
    ) -> Result<RequestPacket> {
        self.provider.request(i, id, action, accounting)
    }
    pub fn read_request(&self, i: &Installation, bytes: &[u8]) -> Result<RequestPacket> {
        self.provider.read_request(i, bytes)
    }
    pub fn snapshot(&self, i: &Installation, a: Option<&Accounting>) -> Result<Snapshot> {
        self.provider.snapshot(i, a)
    }
    pub fn action(&self, i: &Installation, r: &RequestPacket) -> Result<Action> {
        self.provider.action(i, r)
    }
    pub fn resources(&self, i: &Installation) -> Result<Resources> {
        self.provider.resources(i)
    }
    pub fn redeemable(&self, i: &Installation, vault: &str, shares: i128) -> Result<i128> {
        self.provider.redeemable(i, vault, shares)
    }
    pub fn prepare(
        &self,
        i: &Installation,
        r: &RequestPacket,
        terms: &Terms,
        accounting: Option<&Accounting>,
        validity: Validity,
    ) -> Result<Prepared> {
        self.provider.prepare(i, r, terms, accounting, validity)
    }
    /// Obtain service authorization without exposing it to RPC. Caller must retain it before finish_authorization.
    pub fn authorize_preparation(
        &self,
        i: &Installation,
        p: &Prepared,
        signer: &dyn Signer,
        evaluator: Option<Evaluator<'_>>,
        now: u64,
    ) -> Result<Prepared> {
        self.provider
            .authorize_preparation(i, p, signer, evaluator, now)
    }
    pub fn finalize_authorization(
        &self,
        i: &Installation,
        p: &Prepared,
        signer: &dyn Signer,
        evaluator: Option<Evaluator<'_>>,
        now: u64,
        retain: impl FnOnce(&Prepared) -> Result<()>,
    ) -> Result<Prepared> {
        self.provider
            .finalize(i, p, signer, evaluator, now, Box::new(retain))
    }
    pub fn finish_authorization(
        &self,
        i: &Installation,
        p: &Prepared,
        retain: impl FnOnce(&Prepared) -> Result<()>,
    ) -> Result<Prepared> {
        self.provider.finish(i, p, Box::new(retain))
    }
    pub fn revalidate(&self, i: &Installation, p: &Prepared, now: u64) -> Result<Prepared> {
        self.provider.revalidate(i, p, now)
    }
    pub fn sign(
        &self,
        i: &Installation,
        p: &Prepared,
        s: &dyn Signer,
        now: u64,
    ) -> Result<Signature> {
        self.provider.sign(i, p, s, now)
    }
    pub fn authorize(
        &self,
        i: &Installation,
        p: &Prepared,
        s: &[Signature],
        now: u64,
    ) -> Result<Signed> {
        self.provider.authorize(i, p, s, now)
    }
    /// Retain the original packet before broadcast. Retries observe that identity first.
    pub fn submit(&self, store: &mut dyn OperationStore, p: &Signed, now: u64) -> Result<Status> {
        let id = p.info()?.operation_id;
        if let Some((original, previous)) = store.load(id)? {
            if original.bytes() != p.bytes() || original.network() != p.network() {
                return Err(crate::error::Error::config(
                    "Operation ID already binds different signed bytes",
                ));
            }
            if previous != Status::Prepared {
                let observed = self.provider.status(&original)?;
                if observed != Status::SubmissionUnknown
                    || self.provider.validate_signed(p, now).is_err()
                {
                    store.record(id, &original, observed.clone())?;
                    return Ok(observed);
                }
            }
        }
        self.provider.validate_signed(p, now)?;
        store.record(id, p, Status::SubmissionUnknown)?;
        let status = self.provider.broadcast(p, now)?;
        if status != Status::SubmissionUnknown {
            store.record(id, p, status.clone())?;
        }
        Ok(status)
    }
    /// Observe only the retained original packet. Cached status grants no settlement authority.
    pub fn recover(&self, store: &mut dyn OperationStore, id: [u8; 32]) -> Result<Status> {
        let (original, _) = store
            .load(id)?
            .ok_or_else(|| crate::error::Error::config("No retained operation"))?;
        if original.info()?.operation_id != id {
            return Err(crate::error::Error::config(
                "Retained operation identity differs",
            ));
        }
        let status = self.provider.status(&original)?;
        store.record(id, &original, status.clone())?;
        Ok(status)
    }
    pub fn observe(&self, p: &Signed) -> Result<Option<Receipt>> {
        self.provider.observe(p)
    }
    pub fn observe_authorizations(
        &self,
        packets: &[Prepared],
    ) -> Result<Vec<ObservedAuthorization>> {
        self.provider.observe_authorizations(packets)
    }
    pub fn prepare_owner(
        &self,
        i: &Installation,
        action: OwnerAction,
        fee_limit: u64,
        now: u64,
    ) -> Result<OwnerPacket> {
        self.provider.prepare_owner(i, action, fee_limit, now)
    }
    pub fn read_owner_signed(&self, p: &OwnerPacket, signed_payload: &[u8]) -> Result<OwnerSigned> {
        self.provider.read_owner_signed(p, signed_payload)
    }
    pub fn submit_owner(
        &self,
        p: &OwnerSigned,
        retain: impl FnOnce(&OwnerSigned) -> Result<()>,
        poll: Poll,
        now: u64,
    ) -> Result<Receipt> {
        self.provider.submit_owner(p, Box::new(retain), poll, now)
    }
    pub fn observe_owner(&self, p: &OwnerSigned) -> Result<Option<Receipt>> {
        self.provider.observe_owner(p)
    }
}

/// Verify a transport identity proof. Authorization for the requested policy remains with the caller.
pub fn verify_request_proof(
    network: &str,
    proof: &str,
    now: u64,
    path: &str,
    canonical_body: &[u8],
) -> Result<String> {
    crate::vendors::runtime_verify_request_proof(network, proof, now, path, canonical_body)
}

/// Encode the public part of a caller-owned signing key for the selected vendor.
pub fn signer_identity(network: &str, public_key: &[u8; 32]) -> Result<String> {
    crate::vendors::runtime_signer_identity(network, public_key)
}
