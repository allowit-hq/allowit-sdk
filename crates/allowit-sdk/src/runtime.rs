//! Shared runtime orchestration over vendor-owned packets and native implementations.
pub mod owner;
use crate::{config::ServerAuthority, error::Result};
use allowit_core::vendor::runtime as native;
pub use native::{
    Accounting, Action, Debit, Evaluator, Expense, Lifetime, OwnerAction, OwnerInfo, PacketInfo,
    ParamsUpdate, PolicyInstallation, Poll, Problem, ReceiptInfo, Resources, Signature, Signer,
    Snapshot, Status, Terms, Tier, Transport, Validity,
};
pub(crate) use native::{Kind, Packet};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
pub struct Installation(pub(crate) native::Installation);
impl std::ops::Deref for Installation {
    type Target = native::Installation;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Installation {
    pub fn description(&self) -> &crate::profiles::Description {
        self.0.description()
    }
    pub fn native_document(&self) -> &[u8] {
        self.0.native_document()
    }
    pub fn policy_wallet(input: PolicyInstallation) -> Result<Self> {
        crate::plugins::for_network(&input.description.network)?
            .install(input)
            .map(Self)
    }
}
/// A retained packet names the rail plugin that restores it.
#[derive(Serialize)]
struct Stored<'a> {
    rail: &'static str,
    packet: &'a RawValue,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Restored {
    rail: String,
    packet: Box<RawValue>,
}
macro_rules! carrier {
    ($name:ident,$kind:ident) => {
        #[derive(Clone, Debug)]
        pub struct $name(pub(crate) Packet);
        impl $name {
            pub fn bytes(&self) -> &[u8] {
                self.0.document().get().as_bytes()
            }
            pub fn network(&self) -> &str {
                self.0.network()
            }
        }
        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(
                &self,
                s: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                let rail = crate::plugins::for_network(self.0.network())
                    .map_err(serde::ser::Error::custom)?;
                Stored {
                    rail: rail.id(),
                    packet: self.0.document(),
                }
                .serialize(s)
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(
                d: D,
            ) -> std::result::Result<Self, D::Error> {
                let stored = Restored::deserialize(d)?;
                crate::plugins::by_id(&stored.rail)
                    .and_then(|rail| rail.restore(Kind::$kind, stored.packet))
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
        crate::plugins::for_network(self.0.network())?.request_id(&self.0)
    }
}
impl PartialEq for RequestPacket {
    fn eq(&self, other: &Self) -> bool {
        native::RequestPacket(self.0.clone()) == native::RequestPacket(other.0.clone())
    }
}

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
impl OwnerPacket {
    pub fn info(&self) -> Result<OwnerInfo> {
        self.0.provider().owner_info(&self.0)
    }
}
impl Prepared {
    pub fn info(&self) -> Result<PacketInfo> {
        self.0.provider().info(&self.0, Kind::Prepared)
    }
    pub fn metadata(&self) -> Result<PreparedMetadata> {
        let m = self.0.provider().prepared_metadata(&self.0)?;
        Ok(PreparedMetadata {
            configuration: owner::Configuration(m.configuration),
            request: RequestPacket(m.request.0),
            operation_id: m.operation_id,
            created_at: m.created_at,
            expires_at: m.expires_at,
            transaction_hash: m.transaction_hash,
            debit_units: m.debit_units,
            request_hash: m.request_hash,
            debit: m.debit,
        })
    }
}
impl Signed {
    pub fn validate_signatures(&self) -> Result<()> {
        self.0.provider().validate_signed(&self.0)
    }
    pub fn preparation(&self) -> Result<Prepared> {
        self.0
            .provider()
            .signed_preparation(&self.0)
            .map(|p| Prepared(p.0))
    }
    pub fn info(&self) -> Result<PacketInfo> {
        self.0.provider().info(&self.0, Kind::Signed)
    }
}
impl Receipt {
    pub fn verify_operation(&self, signed: &Signed) -> Result<()> {
        self.0.provider().verify_receipt(&self.0, &signed.0)
    }
    pub fn info(&self) -> Result<ReceiptInfo> {
        self.0.provider().receipt_info(&self.0)
    }
}
pub trait OperationStore {
    fn load(&self, id: [u8; 32]) -> Result<Option<(Signed, Status)>>;
    fn record(&mut self, id: [u8; 32], signed: &Signed, status: Status) -> Result<()>;
}
pub struct ObservedAuthorization {
    pub signed: Signed,
    pub receipt: Receipt,
    pub status: Status,
}
pub struct Runtime {
    pub(crate) provider: Box<dyn crate::plugins::RuntimeProvider>,
}
impl Runtime {
    pub fn discover_snapshot(
        &self,
        configuration: &owner::Configuration,
        accounting: Option<&Accounting>,
    ) -> Result<Snapshot> {
        self.provider
            .discover_snapshot(&configuration.0, accounting)
    }
    pub fn current_height(&self) -> Result<u64> {
        self.provider.current_height()
    }
    pub fn observe_hash(&self, hash: &[u8; 32]) -> Result<Option<Receipt>> {
        self.provider
            .observe_hash(hash)
            .map(|p| p.map(|p| Receipt(p.0)))
    }

    pub fn with_transport(network: &str, transport: std::sync::Arc<dyn Transport>) -> Result<Self> {
        let network = crate::routing::normalize(network)?;
        crate::plugins::for_network(network)?
            .runtime(network, transport)
            .map(|provider| Self { provider })
    }
    pub fn new(network: &str, endpoint: &str) -> Result<Self> {
        let network = crate::routing::normalize(network)?;
        let rail = crate::plugins::for_network(network)?;
        rail.runtime(network, rail.connect(network, endpoint)?)
            .map(|provider| Self { provider })
    }
    pub fn import(&self, authority: &ServerAuthority, bytes: &[u8]) -> Result<Installation> {
        self.provider.import(&authority.0, bytes).map(Installation)
    }
    pub fn request(
        &self,
        i: &Installation,
        id: [u8; 32],
        action: Action,
        accounting: Option<&Accounting>,
    ) -> Result<RequestPacket> {
        self.provider
            .request(&i.0, id, action, accounting)
            .map(|p| RequestPacket(p.0))
    }
    pub fn read_request(&self, i: &Installation, bytes: &[u8]) -> Result<RequestPacket> {
        self.provider
            .read_request(&i.0, bytes)
            .map(|p| RequestPacket(p.0))
    }
    pub fn snapshot(&self, i: &Installation, a: Option<&Accounting>) -> Result<Snapshot> {
        self.provider.snapshot(&i.0, a)
    }
    pub fn action(&self, i: &Installation, r: &RequestPacket) -> Result<Action> {
        self.provider
            .action(&i.0, &native::RequestPacket(r.0.clone()))
    }
    pub fn resources(&self, i: &Installation) -> Result<Resources> {
        self.provider.resources(&i.0)
    }
    pub fn redeemable(&self, i: &Installation, vault: &str, shares: i128) -> Result<i128> {
        self.provider.redeemable(&i.0, vault, shares)
    }
    pub fn prepare(
        &self,
        i: &Installation,
        r: &RequestPacket,
        terms: &Terms,
        accounting: Option<&Accounting>,
        validity: Validity,
    ) -> Result<Prepared> {
        self.provider
            .prepare(
                &i.0,
                &native::RequestPacket(r.0.clone()),
                terms,
                accounting,
                validity,
            )
            .map(|p| Prepared(p.0))
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
            .authorize_preparation(&i.0, &native::Prepared(p.0.clone()), signer, evaluator, now)
            .map(|p| Prepared(p.0))
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
            .finalize(
                &i.0,
                &native::Prepared(p.0.clone()),
                signer,
                evaluator,
                now,
                Box::new(|p| retain(&Prepared(p.0.clone()))),
            )
            .map(|p| Prepared(p.0))
    }
    pub fn finish_authorization(
        &self,
        i: &Installation,
        p: &Prepared,
        retain: impl FnOnce(&Prepared) -> Result<()>,
    ) -> Result<Prepared> {
        self.provider
            .finish(
                &i.0,
                &native::Prepared(p.0.clone()),
                Box::new(|p| retain(&Prepared(p.0.clone()))),
            )
            .map(|p| Prepared(p.0))
    }
    pub fn revalidate(&self, i: &Installation, p: &Prepared, now: u64) -> Result<Prepared> {
        self.provider
            .revalidate(&i.0, &native::Prepared(p.0.clone()), now)
            .map(|p| Prepared(p.0))
    }
    pub fn sign(
        &self,
        i: &Installation,
        p: &Prepared,
        s: &dyn Signer,
        now: u64,
    ) -> Result<Signature> {
        self.provider
            .sign(&i.0, &native::Prepared(p.0.clone()), s, now)
    }
    pub fn authorize(
        &self,
        i: &Installation,
        p: &Prepared,
        s: &[Signature],
        now: u64,
    ) -> Result<Signed> {
        self.provider
            .authorize(&i.0, &native::Prepared(p.0.clone()), s, now)
            .map(|p| Signed(p.0))
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
                let observed = self.provider.status(&native::Signed(original.0.clone()))?;
                if observed != Status::SubmissionUnknown
                    || self
                        .provider
                        .validate_signed(&native::Signed(p.0.clone()), now)
                        .is_err()
                {
                    store.record(id, &original, observed.clone())?;
                    return Ok(observed);
                }
            }
        }
        self.provider
            .validate_signed(&native::Signed(p.0.clone()), now)?;
        store.record(id, p, Status::SubmissionUnknown)?;
        let status = self.provider.broadcast(&native::Signed(p.0.clone()), now)?;
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
        let status = self.provider.status(&native::Signed(original.0.clone()))?;
        store.record(id, &original, status.clone())?;
        Ok(status)
    }
    pub fn observe(&self, p: &Signed) -> Result<Option<Receipt>> {
        self.provider
            .observe(&native::Signed(p.0.clone()))
            .map(|p| p.map(|p| Receipt(p.0)))
    }
    pub fn observe_authorizations(
        &self,
        packets: &[Prepared],
    ) -> Result<Vec<ObservedAuthorization>> {
        self.provider
            .observe_authorizations(
                &packets
                    .iter()
                    .map(|p| native::Prepared(p.0.clone()))
                    .collect::<Vec<_>>(),
            )
            .map(|packets| {
                packets
                    .into_iter()
                    .map(|p| ObservedAuthorization {
                        signed: Signed(p.signed.0),
                        receipt: Receipt(p.receipt.0),
                        status: p.status,
                    })
                    .collect()
            })
    }
    pub fn prepare_owner(
        &self,
        i: &Installation,
        action: OwnerAction,
        fee_limit: u64,
        now: u64,
    ) -> Result<OwnerPacket> {
        self.provider
            .prepare_owner(&i.0, action, fee_limit, now)
            .map(|p| OwnerPacket(p.0))
    }
    /// Prepare the owner step that changes installed params on an active wallet. The rail
    /// writes the next configuration revision and keeps the recorded usage and the request
    /// nonce. The packet takes the path of every owner packet.
    pub fn prepare_params_update(
        &self,
        i: &Installation,
        update: &ParamsUpdate,
        fee_limit: u64,
        now: u64,
    ) -> Result<OwnerPacket> {
        self.provider
            .prepare_params_update(&i.0, update, fee_limit, now)
            .map(|p| OwnerPacket(p.0))
    }
    pub fn read_owner_signed(&self, p: &OwnerPacket, signed_payload: &[u8]) -> Result<OwnerSigned> {
        self.provider
            .read_owner_signed(&native::OwnerPacket(p.0.clone()), signed_payload)
            .map(|p| OwnerSigned(p.0))
    }
    pub fn submit_owner(
        &self,
        p: &OwnerSigned,
        retain: impl FnOnce(&OwnerSigned) -> Result<()>,
        poll: Poll,
        now: u64,
    ) -> Result<Receipt> {
        self.provider
            .submit_owner(
                &native::OwnerSigned(p.0.clone()),
                Box::new(|p| retain(&OwnerSigned(p.0.clone()))),
                poll,
                now,
            )
            .map(|p| Receipt(p.0))
    }
    pub fn observe_owner(&self, p: &OwnerSigned) -> Result<Option<Receipt>> {
        self.provider
            .observe_owner(&native::OwnerSigned(p.0.clone()))
            .map(|p| p.map(|p| Receipt(p.0)))
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
    crate::request_proof::verify(
        crate::plugins::for_network(network)?.request_proof_protocol(),
        proof,
        now,
        path,
        canonical_body,
    )
}

/// Encode the public part of a caller-owned signing key for the selected vendor.
pub fn signer_identity(network: &str, public_key: &[u8; 32]) -> Result<String> {
    crate::plugins::for_network(network)?.signer_identity(public_key)
}

impl Runtime {
    /// Return diagnostics with one operation, without retaining evidence for a later call.
    /// The installation selects the expected wallet; it does not prove a fault's origin.
    pub fn diagnose<T>(
        &self,
        installation: &Installation,
        operation: impl FnOnce(&Runtime) -> Result<T>,
    ) -> std::result::Result<T, Problem> {
        let session = self
            .provider
            .diagnostic_session(&installation.0)
            .map_err(|error| self.provider.problem(&error))?;
        match session {
            Some(session) => operation(&Runtime {
                provider: session.provider,
            })
            .map_err(|error| (session.classify)(&error)),
            None => operation(self).map_err(|error| self.provider.problem(&error)),
        }
    }
    pub fn problem(&self, error: &crate::error::Error) -> Problem {
        self.provider.problem(error)
    }
}
