//! Shared runtime orchestration over vendor-owned packets and native implementations.
pub mod owner;
use crate::{config::ServerAuthority, error::Result};
use allowit_core::vendor::runtime as native;
pub use native::{
    Accounting, Action, Debit, Evaluator, Expense, Lifetime, OwnerAction, OwnerInfo, PacketInfo,
    ParamsUpdate, PolicyInstallation, Poll, Problem, ReceiptInfo, Resources, SendingAccounts,
    Signature, Signer, Snapshot, Status, Terms, Tier, Transport, Usage, Validity,
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
    /// The operation signs a payment authorization that the seller submits: the host sends
    /// nothing, and `transaction_hash` identifies the authorization, not a transaction.
    pub seller_settles: bool,
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
            seller_settles: m.seller_settles,
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
/// Lowercase hexadecimal of `bytes`.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
/// A journal held in memory, for a caller that keeps no durable one: it retains each signed
/// packet under its operation ID, refuses other bytes under the same ID, and lasts as long as
/// the value.
#[derive(Default)]
pub struct MemoryOperations(std::collections::BTreeMap<[u8; 32], (Signed, Status)>);
impl OperationStore for MemoryOperations {
    fn load(&self, id: [u8; 32]) -> Result<Option<(Signed, Status)>> {
        Ok(self.0.get(&id).cloned())
    }
    fn record(&mut self, id: [u8; 32], signed: &Signed, status: Status) -> Result<()> {
        if let Some((original, _)) = self.0.get(&id)
            && (original.bytes() != signed.bytes() || original.network() != signed.network())
        {
            return Err(crate::error::Error::config(
                "Operation ID already binds different signed bytes",
            ));
        }
        self.0.insert(id, (signed.clone(), status));
        Ok(())
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
    /// The server's runtime: its own `sender` accounts send each contract-wallet operation and
    /// code upload, pay the network fee and receive the gas that the wallet repays.
    pub fn with_sender(
        network: &str,
        transport: std::sync::Arc<dyn Transport>,
        sender: std::sync::Arc<dyn native::SendingAccounts>,
    ) -> Result<Self> {
        let network = crate::routing::normalize(network)?;
        crate::plugins::for_network(network)?
            .sending_runtime(network, transport, sender)
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
    /// Whether the prepared operation lapsed: it did not execute, and no envelope that carries
    /// its authorization can execute it any more. `false` means not proven.
    pub fn lapsed(&self, p: &Prepared) -> Result<bool> {
        self.provider.lapsed(&native::Prepared(p.0.clone()))
    }
    /// Whether the owner transaction with hash `hash` lapsed: it never applied and can no longer
    /// apply. The server prepared it at height `since`, valid until `expires_at`. `false` means
    /// not proven.
    pub fn owner_lapsed(&self, hash: &[u8; 32], expires_at: u64, since: u64) -> Result<bool> {
        self.provider.owner_lapsed(hash, expires_at, since)
    }
    /// What the operation adds to its wallet's figures when it executes, as the wallet counts
    /// it. `None` for a rail without such a count.
    pub fn usage(&self, p: &Prepared) -> Result<Option<Usage>> {
        self.provider.usage(&native::Prepared(p.0.clone()))
    }
    /// Make the wallet code of an installation live from the server's sending accounts, which
    /// pay for it: the upload of `wasm`, or without it the restoration of the installation's
    /// lapsed code, with the network fee bounded by `max_fee` in the rail's native units. `None`
    /// when nothing is needed. The rail prepares the transaction and signs it with a sending
    /// account (`prepare_code_within`); this retains it in the journal `store` before its first
    /// send and sends it with `submit`. A send that the network did not accept goes out again,
    /// the first one included, only while the rail says that it is still needed (`code_needed`),
    /// since another transaction can make the code live meanwhile. An accepted or uncertain send
    /// is followed with `recover` within `poll`. The receipt is that of a confirmed
    /// transaction: one that the ledger included and failed is an error whose details name its
    /// `transactionHash` and the fee that its source paid, `nativeUnits`.
    pub fn make_code_live(
        &self,
        store: &mut dyn OperationStore,
        i: &Installation,
        wasm: Option<&[u8]>,
        max_fee: u64,
        poll: Poll,
        now: u64,
    ) -> Result<Option<Receipt>> {
        let Some(prepared) = self
            .provider
            .prepare_code_within(&i.0, wasm, max_fee, now)?
        else {
            return Ok(None);
        };
        let signed = Signed(prepared.0);
        let id = signed.info()?.operation_id;
        let started = std::time::Instant::now();
        let clock = || now.saturating_add(started.elapsed().as_secs());
        let mut status = Status::Prepared;
        for attempt in 0..poll.attempts.max(1) {
            if attempt > 0 {
                std::thread::sleep(poll.interval);
            }
            status = match status {
                // Not sent yet, or not accepted: nothing is pending.
                Status::Prepared => {
                    if !self
                        .provider
                        .code_needed(&native::Signed(signed.0.clone()))?
                    {
                        return Ok(None);
                    }
                    self.submit(store, &signed, clock())?
                }
                _ => self.recover(store, id)?,
            };
            if matches!(status, Status::Confirmed | Status::Failed) {
                break;
            }
        }
        if !matches!(status, Status::Confirmed | Status::Failed) {
            return Err(crate::error::Error::uncertain(format!(
                "The wallet code transaction {} has no final result yet; look it up again before any replacement",
                hex(&id)
            )));
        }
        let receipt = self.observe(&signed)?.ok_or_else(|| {
            crate::error::Error::uncertain(format!(
                "The wallet code transaction {} has no receipt yet; look it up again before any replacement",
                hex(&id)
            ))
        })?;
        let info = receipt.info()?;
        if info.status != Status::Confirmed {
            return Err(crate::error::Error::config(
                "The wallet code transaction failed on the ledger",
            )
            .with_details(serde_json::json!({
                "transactionHash": hex(&info.transaction_hash),
                "nativeUnits": info.fee.native_units.to_string(),
            })));
        }
        Ok(Some(receipt))
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

#[cfg(test)]
mod code_transactions {
    //! `make_code_live` over a rail that prepares a code transaction and a network that
    //! answers its sends and lookups as each case sets.
    use super::*;
    use std::sync::{Arc, Mutex};
    const NETWORK: &str = "stellar:testnet";
    #[derive(Default)]
    struct Chain {
        live: bool,
        /// The answers of the next sends; `Submitted` once they are used up.
        answers: Vec<Status>,
        /// Another transaction makes the code live during this send, counted from one.
        live_at_send: Option<usize>,
        sends: usize,
        /// Lookups that still find the transaction pending.
        pending_lookups: usize,
        /// The ledger's final status of the transaction, once it has one.
        outcome: Option<Status>,
    }
    struct Packets;
    static PACKETS: Packets = Packets;
    impl native::PacketProvider for Packets {
        fn request_id(&self, _: &Packet) -> Result<[u8; 32]> {
            unimplemented!()
        }
        fn owner_info(&self, _: &Packet) -> Result<native::OwnerInfo> {
            unimplemented!()
        }
        fn prepared_metadata(&self, _: &Packet) -> Result<native::PreparedMetadata> {
            unimplemented!()
        }
        fn info(&self, _: &Packet, _: Kind) -> Result<PacketInfo> {
            Ok(PacketInfo {
                operation_id: [7; 32],
                transaction_hash: [7; 32],
                wallet: String::new(),
                expires_at: u64::MAX,
            })
        }
        fn validate_signed(&self, _: &Packet) -> Result<()> {
            Ok(())
        }
        fn signed_preparation(&self, _: &Packet) -> Result<native::Prepared> {
            unimplemented!()
        }
        fn verify_consent(&self, _: &Snapshot, _: &str, _: &[Signature]) -> Result<()> {
            unimplemented!()
        }
        fn signer_weight(&self, _: &Snapshot, _: &str) -> Result<u32> {
            unimplemented!()
        }
        fn authority_document(&self, _: &Snapshot) -> Result<serde_json::Value> {
            unimplemented!()
        }
        fn same_snapshot(&self, _: &Snapshot, _: &Snapshot) -> Result<bool> {
            unimplemented!()
        }
        fn verify_receipt(&self, _: &Packet, _: &Packet) -> Result<()> {
            Ok(())
        }
        fn receipt_info(&self, p: &Packet) -> Result<ReceiptInfo> {
            let document: serde_json::Value = serde_json::from_str(p.document().get()).unwrap();
            Ok(ReceiptInfo {
                transaction_hash: [7; 32],
                status: serde_json::from_value(document["status"].clone()).unwrap(),
                observed_height: 1,
                fee: native::Fee {
                    native_units: 250,
                    charged_units: 0,
                    repaid_native_units: 0,
                },
                debits: Vec::new(),
                settlement_hash: None,
            })
        }
    }
    struct Rail(Arc<Mutex<Chain>>);
    #[allow(unused_variables)]
    impl allowit_core::vendor::RuntimeProvider for Rail {
        fn discover_snapshot(
            &self,
            configuration: &native::owner::Configuration,
            accounting: Option<&native::Accounting>,
        ) -> Result<native::Snapshot> {
            unimplemented!()
        }
        fn current_height(&self) -> Result<u64> {
            unimplemented!()
        }
        fn observe_hash(&self, hash: &[u8; 32]) -> Result<Option<native::Receipt>> {
            unimplemented!()
        }
        fn import(
            &self,
            a: &allowit_core::vendor::config::ServerAuthority,
            b: &[u8],
        ) -> Result<native::Installation> {
            unimplemented!()
        }
        fn request(
            &self,
            i: &native::Installation,
            id: [u8; 32],
            a: native::Action,
            accounting: Option<&native::Accounting>,
        ) -> Result<native::RequestPacket> {
            unimplemented!()
        }
        fn read_request(
            &self,
            i: &native::Installation,
            b: &[u8],
        ) -> Result<native::RequestPacket> {
            unimplemented!()
        }
        fn snapshot(
            &self,
            i: &native::Installation,
            a: Option<&native::Accounting>,
        ) -> Result<native::Snapshot> {
            unimplemented!()
        }
        fn action(
            &self,
            i: &native::Installation,
            r: &native::RequestPacket,
        ) -> Result<native::Action> {
            unimplemented!()
        }
        fn resources(&self, i: &native::Installation) -> Result<native::Resources> {
            unimplemented!()
        }
        fn redeemable(&self, i: &native::Installation, v: &str, shares: i128) -> Result<i128> {
            unimplemented!()
        }
        fn finish<'a>(
            &self,
            i: &native::Installation,
            p: &native::Prepared,
            retain: Box<dyn FnOnce(&native::Prepared) -> Result<()> + 'a>,
        ) -> Result<native::Prepared> {
            unimplemented!()
        }
        fn prepare(
            &self,
            i: &native::Installation,
            r: &native::RequestPacket,
            t: &native::Terms,
            a: Option<&native::Accounting>,
            v: native::Validity,
        ) -> Result<native::Prepared> {
            unimplemented!()
        }
        fn authorize_preparation(
            &self,
            i: &native::Installation,
            p: &native::Prepared,
            signer: &dyn native::Signer,
            evaluator: Option<native::Evaluator<'_>>,
            now: u64,
        ) -> Result<native::Prepared> {
            unimplemented!()
        }
        fn finalize<'a>(
            &self,
            i: &native::Installation,
            p: &native::Prepared,
            s: &dyn native::Signer,
            e: Option<native::Evaluator<'_>>,
            now: u64,
            retain: Box<dyn FnOnce(&native::Prepared) -> Result<()> + 'a>,
        ) -> Result<native::Prepared> {
            unimplemented!()
        }
        fn revalidate(
            &self,
            i: &native::Installation,
            p: &native::Prepared,
            now: u64,
        ) -> Result<native::Prepared> {
            unimplemented!()
        }
        fn sign(
            &self,
            i: &native::Installation,
            p: &native::Prepared,
            s: &dyn native::Signer,
            now: u64,
        ) -> Result<native::Signature> {
            unimplemented!()
        }
        fn authorize(
            &self,
            i: &native::Installation,
            p: &native::Prepared,
            s: &[native::Signature],
            now: u64,
        ) -> Result<native::Signed> {
            unimplemented!()
        }
        fn validate_signed(&self, p: &native::Signed, now: u64) -> Result<()> {
            Ok(())
        }
        fn broadcast(&self, p: &native::Signed, now: u64) -> Result<native::Status> {
            let mut chain = self.0.lock().unwrap();
            chain.sends += 1;
            if chain.live_at_send == Some(chain.sends) {
                chain.live = true;
            }
            Ok(if chain.answers.is_empty() {
                Status::Submitted
            } else {
                chain.answers.remove(0)
            })
        }
        fn status(&self, p: &native::Signed) -> Result<native::Status> {
            let mut chain = self.0.lock().unwrap();
            if chain.pending_lookups > 0 {
                chain.pending_lookups -= 1;
                return Ok(Status::Submitted);
            }
            Ok(chain.outcome.clone().unwrap_or(Status::Submitted))
        }
        fn observe(&self, p: &native::Signed) -> Result<Option<native::Receipt>> {
            let chain = self.0.lock().unwrap();
            chain
                .outcome
                .clone()
                .map(|status| {
                    native::Packet::new(NETWORK, &serde_json::json!({ "status": status }), &PACKETS)
                        .map(native::Receipt)
                })
                .transpose()
        }
        fn observe_authorizations(
            &self,
            p: &[native::Prepared],
        ) -> Result<Vec<native::ObservedAuthorization>> {
            unimplemented!()
        }
        fn prepare_owner(
            &self,
            i: &native::Installation,
            a: native::OwnerAction,
            fee: u64,
            now: u64,
        ) -> Result<native::OwnerPacket> {
            unimplemented!()
        }
        fn prepare_code_within(
            &self,
            i: &native::Installation,
            wasm: Option<&[u8]>,
            max_fee: u64,
            now: u64,
        ) -> Result<Option<native::Signed>> {
            let chain = self.0.lock().unwrap();
            if chain.live {
                return Ok(None);
            }
            Ok(Some(native::Signed(native::Packet::new(
                NETWORK,
                &serde_json::json!({"code": 1}),
                &PACKETS,
            )?)))
        }
        fn code_needed(&self, p: &native::Signed) -> Result<bool> {
            Ok(!self.0.lock().unwrap().live)
        }
        fn read_owner_signed(
            &self,
            p: &native::OwnerPacket,
            signed_payload: &[u8],
        ) -> Result<native::OwnerSigned> {
            unimplemented!()
        }
        fn submit_owner<'a>(
            &self,
            p: &native::OwnerSigned,
            retain: Box<dyn FnOnce(&native::OwnerSigned) -> Result<()> + 'a>,
            poll: native::Poll,
            now: u64,
        ) -> Result<native::Receipt> {
            unimplemented!()
        }
        fn observe_owner(&self, p: &native::OwnerSigned) -> Result<Option<native::Receipt>> {
            unimplemented!()
        }
    }
    fn installation() -> Installation {
        Installation(native::Installation {
            description: crate::profiles::Description {
                network: NETWORK.into(),
                wallet: String::new(),
                agent: String::new(),
                policy_id: String::new(),
                owner: String::new(),
                server: String::new(),
                asset: String::new(),
                asset_symbol: String::new(),
                source: String::new(),
                source_hash: String::new(),
                module_digest: String::new(),
                params: Default::default(),
            },
            config: RawValue::from_string("{}".into()).unwrap(),
            export: None,
            authority: None,
        })
    }
    fn run(chain: Chain) -> (Result<Option<Receipt>>, Arc<Mutex<Chain>>, MemoryOperations) {
        let chain = Arc::new(Mutex::new(chain));
        let runtime = Runtime {
            provider: Box::new(Rail(chain.clone())),
        };
        let mut store = MemoryOperations::default();
        let poll = Poll {
            attempts: 4,
            interval: std::time::Duration::ZERO,
        };
        let result = runtime.make_code_live(&mut store, &installation(), None, 10_000, poll, 1);
        (result, chain, store)
    }
    #[test]
    fn live_code_needs_no_transaction() {
        let (result, chain, store) = run(Chain {
            live: true,
            ..Chain::default()
        });
        assert!(result.unwrap().is_none());
        assert_eq!(chain.lock().unwrap().sends, 0);
        assert!(store.load([7; 32]).unwrap().is_none());
    }
    #[test]
    fn a_confirmed_restoration_is_retained_and_returned() {
        let (result, chain, store) = run(Chain {
            pending_lookups: 1,
            outcome: Some(Status::Confirmed),
            ..Chain::default()
        });
        let receipt = result.unwrap().unwrap();
        assert_eq!(receipt.info().unwrap().status, Status::Confirmed);
        assert_eq!(chain.lock().unwrap().sends, 1);
        assert_eq!(store.load([7; 32]).unwrap().unwrap().1, Status::Confirmed);
    }
    #[test]
    fn a_send_that_was_not_accepted_goes_out_again_only_while_it_is_needed() {
        // Another transaction makes the code live while the first send waits: the transaction
        // is not sent again.
        let (result, chain, _) = run(Chain {
            answers: vec![Status::Prepared],
            live_at_send: Some(1),
            outcome: Some(Status::Confirmed),
            ..Chain::default()
        });
        assert!(result.unwrap().is_none());
        assert_eq!(chain.lock().unwrap().sends, 1);
        // While it is needed, the same bytes go out again.
        let (result, chain, store) = run(Chain {
            answers: vec![Status::Prepared],
            outcome: Some(Status::Confirmed),
            ..Chain::default()
        });
        assert!(result.unwrap().is_some());
        assert_eq!(chain.lock().unwrap().sends, 2);
        assert_eq!(store.load([7; 32]).unwrap().unwrap().1, Status::Confirmed);
    }
    #[test]
    fn a_transaction_that_the_ledger_failed_names_itself_and_its_fee() {
        let (result, _, _) = run(Chain {
            outcome: Some(Status::Failed),
            ..Chain::default()
        });
        let failed = result.unwrap_err();
        assert!(failed.message.contains("failed on the ledger"));
        assert_eq!(
            failed.detail("transactionHash"),
            Some(hex(&[7; 32]).as_str())
        );
        assert_eq!(failed.detail("nativeUnits"), Some("250"));
    }
    #[test]
    fn a_transaction_without_a_final_status_is_uncertain() {
        let (result, chain, store) = run(Chain {
            pending_lookups: 10,
            outcome: Some(Status::Confirmed),
            ..Chain::default()
        });
        let uncertain = result.unwrap_err();
        assert!(
            uncertain.message.contains("look it up again"),
            "{}",
            uncertain.message
        );
        assert_eq!(chain.lock().unwrap().sends, 1);
        assert_eq!(store.load([7; 32]).unwrap().unwrap().1, Status::Submitted);
    }
}
