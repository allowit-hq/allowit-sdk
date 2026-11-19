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
/// Whether a journal status leaves a send pending: the journal holds the transaction past
/// `Prepared`, and the network may still include it.
fn pending(status: &Status) -> bool {
    matches!(status, Status::Submitted | Status::SubmissionUnknown)
}
/// The outcome of a code transaction from its receipt, which is final whatever the journal can
/// record of it: a confirmed transaction returns its receipt, and one that the ledger failed is
/// an error that names it and the fee that its source paid.
fn code_receipt(
    store: &mut dyn OperationStore,
    id: [u8; 32],
    signed: &Signed,
    receipt: Receipt,
) -> Result<Option<Receipt>> {
    // Only a receipt of this transaction is its outcome.
    receipt.verify_operation(signed)?;
    let info = receipt.info()?;
    let _ = store.record(id, signed, info.status.clone());
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
/// What a journal holds under an operation ID: the retained packet and its status.
type Journaled = Option<(Signed, Status)>;
/// Lowercase hexadecimal of `bytes`.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
/// A journal held in memory, for a caller that keeps no durable one: it retains each signed
/// packet under its operation ID, refuses other bytes under the same ID, and lasts as long as
/// the value.
#[derive(Default)]
pub struct MemoryOperations(std::collections::BTreeMap<[u8; 32], (Signed, Status)>);
impl MemoryOperations {
    /// Whether the journal holds a transaction as pending: past `Prepared` and without a final
    /// status, so the network may still include it. A caller that frees the sending account of
    /// a code transaction only when this is false never frees one that may still be charged.
    pub fn pending(&self) -> bool {
        self.0.values().any(|(_, status)| pending(status))
    }
}
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
    /// Upload the wallet code `wasm` for an installation from the server's sending accounts,
    /// which pay for it, with the network fee bounded by `max_fee` in the rail's native units.
    /// `None` when the code is live. The rail prepares the transaction and signs it with a
    /// sending account (`prepare_code_within`), and this sends it as a code transaction. Nothing
    /// restores code that lapsed.
    ///
    /// A new preparation is a new transaction: its sending account, sequence and time bound give
    /// it another hash, and so another operation ID, than an earlier one. This call therefore
    /// finds no transaction of an earlier call that may still apply. A caller whose journal lasts
    /// one call, as the server's does, can send a second transaction for the same code while the
    /// first may still apply: a code transaction gives no authority, the server pays each within
    /// the fee ceiling, and the sending account of a pending one stays held until it resolves or
    /// its hold ends. A send can apply only within the transaction's time bound
    /// (`Signed::info().expires_at`).
    ///
    /// The journal `store` decides. Its packet is the transaction: when it holds one under the
    /// same operation ID, this follows that original and its status, not the new preparation.
    /// A journal packet that names another operation ID, transaction hash or network stops the
    /// call, at the first read before any send or lookup, and at a later read before its status
    /// counts. The read just before each send decides: a transaction that the journal holds past
    /// `Prepared`, also one that another caller of a shared journal sent, is looked up by its
    /// retained packet, never sent again. A receipt counts only when the rail verifies it as this
    /// transaction's. The clock of each send counts from the call's start, so
    /// the preparation's reads of the network take part of the transaction's time bound.
    /// `submit` retains the transaction before every send, so a transaction is pending once the
    /// journal holds it past `Prepared` (`Submitted` or `SubmissionUnknown`): the network may
    /// still include it. A send that the network did not accept (`Prepared`) goes out again, the
    /// first one included, only while the rail says that it is still needed (`code_needed`),
    /// since another transaction can do its work meanwhile; after the last attempt's refused
    /// send, a transaction that is no longer needed needs nothing. A send that failed is pending
    /// when the journal holds it as pending, and also when the journal cannot be read: it is
    /// looked up, never sent again. Each attempt within `poll` after a send reads the receipt,
    /// which gives the final status and the fee in one lookup. An error within the poll moves on
    /// to the next attempt.
    ///
    /// The receipt is that of a confirmed transaction. One that the ledger included and failed
    /// is an error whose details name its `transactionHash` and the fee that its source paid,
    /// `nativeUnits`. Without a receipt, an error names the transaction's own `transactionHash`
    /// when the journal holds it as pending; it can be looked up again with `observe`.
    pub fn make_code_live(
        &self,
        store: &mut dyn OperationStore,
        i: &Installation,
        wasm: &[u8],
        max_fee: u64,
        poll: Poll,
        now: u64,
    ) -> Result<Option<Receipt>> {
        let started = std::time::Instant::now();
        let Some(prepared) = self
            .provider
            .prepare_code_within(&i.0, wasm, max_fee, now)?
        else {
            return Ok(None);
        };
        self.send_code(store, Signed(prepared.0), poll, (now, started))
    }
    /// Renew the live wallet code `code` that the server uploaded and that wallets share, such as
    /// the code of a fixed tier, from the server's sending accounts, which pay for it, with the
    /// network fee bounded by `max_fee`, once the rail says that it is due
    /// (`prepare_code_renewal`). `None` before. The renewal is a code transaction that takes the
    /// path of `make_code_live`, with its journal. Code that lapsed is not renewed: nothing
    /// restores it.
    pub fn renew_code(
        &self,
        store: &mut dyn OperationStore,
        code: &[u8; 32],
        max_fee: u64,
        poll: Poll,
        now: u64,
    ) -> Result<Option<Receipt>> {
        let started = std::time::Instant::now();
        let Some(prepared) = self.provider.prepare_code_renewal(code, max_fee, now)? else {
            return Ok(None);
        };
        self.send_code(store, Signed(prepared.0), poll, (now, started))
    }
    /// Send the code transaction `prepared` through the journal `store` and follow it to its
    /// receipt within `poll`. The clock counts from `start`, the time `now` at which the call
    /// began and the instant that matched it. See [`Self::make_code_live`].
    fn send_code(
        &self,
        store: &mut dyn OperationStore,
        prepared: Signed,
        poll: Poll,
        start: (u64, std::time::Instant),
    ) -> Result<Option<Receipt>> {
        let expected = prepared.info()?;
        let id = expected.operation_id;
        let network = prepared.network().to_owned();
        // The journal's packet is followed, and its status taken, only when it is this
        // transaction: at the first read and at every read after it.
        let this = |found: Journaled| -> Result<Journaled> {
            match found {
                Some((original, status)) => {
                    let held = original.info()?;
                    if held.operation_id != id
                        || held.transaction_hash != expected.transaction_hash
                        || original.network() != network
                    {
                        return Err(crate::error::Error::config(
                            "The journal holds another transaction under the code transaction's operation ID",
                        ));
                    }
                    Ok(Some((original, status)))
                }
                None => Ok(None),
            }
        };
        let (mut signed, mut status) =
            this(store.load(id)?)?.unwrap_or((prepared, Status::Prepared));
        let hash = expected.transaction_hash;
        let (now, started) = start;
        let clock = || now.saturating_add(started.elapsed().as_secs());
        let mut last = None;
        for attempt in 0..poll.attempts.max(1) {
            if attempt > 0 {
                std::thread::sleep(poll.interval);
            }
            if status == Status::Prepared {
                // Nothing is pending.
                match self.provider.code_needed(&native::Signed(signed.0.clone())) {
                    Ok(true) => {}
                    Ok(false) => return Ok(None),
                    Err(error) => {
                        last = Some(error);
                        continue;
                    }
                }
                status = match self.send_unless_journaled(store, &mut signed, id, &this, clock()) {
                    Ok(status) => status,
                    // The journal tells whether the send that failed may be pending, and holds
                    // the packet that the call follows. A journal that cannot be read leaves it
                    // in doubt: it is looked up, never sent again.
                    Err(error) => {
                        last = Some(error);
                        match store.load(id) {
                            Ok(found) => match this(found)? {
                                Some((original, status)) => {
                                    signed = original;
                                    status
                                }
                                None => Status::Prepared,
                            },
                            Err(_) => Status::SubmissionUnknown,
                        }
                    }
                };
                if status == Status::Prepared {
                    // The network accepted no send. After the last one, live code needs nothing.
                    if attempt + 1 == poll.attempts.max(1)
                        && matches!(
                            self.provider.code_needed(&native::Signed(signed.0.clone())),
                            Ok(false)
                        )
                    {
                        return Ok(None);
                    }
                    continue;
                }
            }
            match self.observe(&signed) {
                Ok(Some(receipt)) => return code_receipt(store, id, &signed, receipt),
                Ok(None) => {}
                Err(error) => last = Some(error),
            }
        }
        // A journal that cannot be read now keeps what this call knew of the send.
        let journal = match store.load(id) {
            Ok(found) => this(found)?.map(|(_, status)| status),
            Err(_) => Some(status.clone()),
        };
        match journal {
            Some(status) if pending(&status) => {
                Err(crate::error::Error::uncertain(format!(
                    "The wallet code transaction {} has no receipt yet; look it up again before any replacement",
                    hex(&hash)
                ))
                .with_details(serde_json::json!({ "transactionHash": hex(&hash) })))
            }
            _ => Err(last.unwrap_or_else(|| {
                crate::error::Error::config(
                    "The network did not accept the wallet code transaction: nothing is pending",
                )
            })),
        }
    }
    /// Send the code transaction `signed` once, unless the journal's read just before the send,
    /// whose packet `this` checks, holds it already: its retained packet becomes the one that the
    /// call follows, and a status past `Prepared` is looked up, never sent again, also when
    /// another caller of a shared journal sent it.
    fn send_unless_journaled(
        &self,
        store: &mut dyn OperationStore,
        signed: &mut Signed,
        id: [u8; 32],
        this: &dyn Fn(Journaled) -> Result<Journaled>,
        now: u64,
    ) -> Result<Status> {
        if let Some((original, status)) = this(store.load(id)?)? {
            *signed = original;
            if status != Status::Prepared {
                return Ok(status);
            }
        }
        self.provider
            .validate_signed(&native::Signed(signed.0.clone()), now)?;
        store.record(id, signed, Status::SubmissionUnknown)?;
        let status = self
            .provider
            .broadcast(&native::Signed(signed.0.clone()), now)?;
        if status != Status::SubmissionUnknown {
            store.record(id, signed, status.clone())?;
        }
        Ok(status)
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
    const NETWORK: &str = "test:network";
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
        /// Lookups that fail, as an unreachable RPC does, before the others answer.
        failing_lookups: usize,
        /// The code checks, counted from one, that fail, as an unreachable RPC does.
        failing_checks: Vec<usize>,
        checks: usize,
        lookups: usize,
        /// Each preparation signs new bytes of the same transaction.
        preparations: usize,
        /// The renewals that the rail prepared.
        renewals: usize,
        /// The packets that the lookups asked about.
        observed: Vec<Vec<u8>>,
        /// The ledger's final status of the transaction, once it has one.
        outcome: Option<Status>,
        /// The transaction that the receipts settle, when it is another one.
        receipt_hash: Option<u64>,
        /// How long each preparation takes, as the rail's reads of the network do.
        preparation_time: std::time::Duration,
        /// The time that each send carried.
        sent_at: Vec<u64>,
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
        /// The operation ID and transaction hash that the packet names (`id`, `hash`), by
        /// default those of the one code transaction.
        fn info(&self, p: &Packet, _: Kind) -> Result<PacketInfo> {
            let document: serde_json::Value = serde_json::from_str(p.document().get()).unwrap();
            let byte = |name: &str, default: u8| {
                [document[name].as_u64().map_or(default, |b| b as u8); 32]
            };
            Ok(PacketInfo {
                operation_id: byte("id", 7),
                transaction_hash: byte("hash", 8),
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
        /// A receipt settles the transaction that its `hash` names, by default the one code
        /// transaction.
        fn verify_receipt(&self, r: &Packet, p: &Packet) -> Result<()> {
            let hash = |packet: &Packet| {
                let document: serde_json::Value =
                    serde_json::from_str(packet.document().get()).unwrap();
                document["hash"].as_u64().unwrap_or(8)
            };
            if hash(r) != hash(p) {
                return Err(crate::error::Error::config(
                    "The receipt settles another transaction",
                ));
            }
            Ok(())
        }
        fn receipt_info(&self, p: &Packet) -> Result<ReceiptInfo> {
            let document: serde_json::Value = serde_json::from_str(p.document().get()).unwrap();
            Ok(ReceiptInfo {
                transaction_hash: [8; 32],
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
    impl Rail {
        /// A code transaction, signed: new bytes for each preparation. Live code needs none.
        fn prepare(&self) -> Result<Option<native::Signed>> {
            let delay = self.0.lock().unwrap().preparation_time;
            std::thread::sleep(delay);
            let mut chain = self.0.lock().unwrap();
            if chain.live {
                return Ok(None);
            }
            chain.preparations += 1;
            Ok(Some(native::Signed(native::Packet::new(
                NETWORK,
                &serde_json::json!({ "code": chain.preparations }),
                &PACKETS,
            )?)))
        }
    }
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
            chain.sent_at.push(now);
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
            // A lookup that has not found the transaction does not establish that it is safe
            // to send it again: uncertain.
            if chain.pending_lookups > 0 {
                chain.pending_lookups -= 1;
                return Ok(Status::SubmissionUnknown);
            }
            Ok(chain.outcome.clone().unwrap_or(Status::Submitted))
        }
        fn observe(&self, p: &native::Signed) -> Result<Option<native::Receipt>> {
            let mut chain = self.0.lock().unwrap();
            chain.lookups += 1;
            chain
                .observed
                .push(p.0.document().get().as_bytes().to_vec());
            if chain.failing_lookups > 0 {
                chain.failing_lookups -= 1;
                return Err(crate::error::Error::config("The RPC is unreachable"));
            }
            if chain.pending_lookups > 0 {
                chain.pending_lookups -= 1;
                return Ok(None);
            }
            chain
                .outcome
                .clone()
                .map(|status| {
                    let mut receipt = serde_json::json!({ "status": status });
                    if let Some(hash) = chain.receipt_hash {
                        receipt["hash"] = hash.into();
                    }
                    native::Packet::new(NETWORK, &receipt, &PACKETS).map(native::Receipt)
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
            wasm: &[u8],
            max_fee: u64,
            now: u64,
        ) -> Result<Option<native::Signed>> {
            self.prepare()
        }
        fn prepare_code_renewal(
            &self,
            code: &[u8; 32],
            max_fee: u64,
            now: u64,
        ) -> Result<Option<native::Signed>> {
            self.0.lock().unwrap().renewals += 1;
            self.prepare()
        }
        fn code_needed(&self, p: &native::Signed) -> Result<bool> {
            let mut chain = self.0.lock().unwrap();
            chain.checks += 1;
            if chain.failing_checks.contains(&chain.checks) {
                return Err(crate::error::Error::config("The RPC is unreachable"));
            }
            Ok(!chain.live)
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
    /// A journal whose `n`th write fails, as a full disk does, and keeps what came before. With
    /// `unreadable`, every read after that failure fails too, as a lost store does.
    struct Journal {
        inner: MemoryOperations,
        writes: usize,
        failing_write: Option<usize>,
        unreadable: bool,
        failed: std::cell::Cell<bool>,
    }
    impl Journal {
        fn failing_at(n: usize) -> Self {
            Self {
                inner: MemoryOperations::default(),
                writes: 0,
                failing_write: Some(n),
                unreadable: false,
                failed: std::cell::Cell::new(false),
            }
        }
        fn lost_at(n: usize) -> Self {
            Self {
                unreadable: true,
                ..Self::failing_at(n)
            }
        }
    }
    impl OperationStore for Journal {
        fn load(&self, id: [u8; 32]) -> Result<Option<(Signed, Status)>> {
            if self.unreadable && self.failed.get() {
                return Err(crate::error::Error::config("The journal is unreachable"));
            }
            self.inner.load(id)
        }
        fn record(&mut self, id: [u8; 32], signed: &Signed, status: Status) -> Result<()> {
            self.writes += 1;
            if self.failing_write == Some(self.writes) {
                self.failed.set(true);
                return Err(crate::error::Error::config("The journal is full"));
            }
            self.inner.record(id, signed, status)
        }
    }
    fn run_on(
        chain: Arc<Mutex<Chain>>,
        store: &mut dyn OperationStore,
        attempts: u32,
    ) -> Result<Option<Receipt>> {
        let runtime = Runtime {
            provider: Box::new(Rail(chain)),
        };
        let poll = Poll {
            attempts,
            interval: std::time::Duration::ZERO,
        };
        runtime.make_code_live(store, &installation(), b"wasm", 10_000, poll, 1)
    }
    fn run(chain: Chain) -> (Result<Option<Receipt>>, Arc<Mutex<Chain>>, MemoryOperations) {
        let chain = Arc::new(Mutex::new(chain));
        let mut store = MemoryOperations::default();
        let result = run_on(chain.clone(), &mut store, 4);
        (result, chain, store)
    }
    /// The error names the transaction by its own hash, not by its operation ID.
    fn names_transaction(error: &crate::error::Error) -> bool {
        error.detail("transactionHash") == Some(hex(&[8; 32]).as_str())
    }
    fn status(store: &dyn OperationStore) -> Option<Status> {
        store.load([7; 32]).unwrap().map(|(_, status)| status)
    }
    /// A renewal of shared code takes the path of an upload: the rail prepares it once it is due,
    /// and it is sent through the journal and followed to its receipt.
    #[test]
    fn a_due_renewal_is_sent_and_followed_to_its_receipt() {
        let chain = Arc::new(Mutex::new(Chain {
            pending_lookups: 1,
            outcome: Some(Status::Confirmed),
            ..Chain::default()
        }));
        let runtime = Runtime {
            provider: Box::new(Rail(chain.clone())),
        };
        let poll = Poll {
            attempts: 4,
            interval: std::time::Duration::ZERO,
        };
        let mut store = MemoryOperations::default();
        let receipt = runtime
            .renew_code(&mut store, &[3; 32], 10_000, poll, 1)
            .unwrap()
            .unwrap();
        assert_eq!(receipt.info().unwrap().status, Status::Confirmed);
        {
            let chain = chain.lock().unwrap();
            assert_eq!((chain.renewals, chain.sends), (1, 1));
        }
        assert_eq!(status(&store), Some(Status::Confirmed));
        // Not due: nothing is prepared or sent.
        chain.lock().unwrap().live = true;
        let poll = Poll {
            attempts: 4,
            interval: std::time::Duration::ZERO,
        };
        assert!(
            runtime
                .renew_code(&mut store, &[3; 32], 10_000, poll, 1)
                .unwrap()
                .is_none()
        );
        assert_eq!(chain.lock().unwrap().sends, 1);
    }
    /// The journal's packet under the operation ID is followed only when it is the transaction
    /// that the rail prepared: another one is neither sent nor looked up.
    #[test]
    fn a_journal_that_holds_another_transaction_under_the_id_stops_the_call() {
        for foreign in [
            serde_json::json!({ "id": 9 }),
            serde_json::json!({ "hash": 9 }),
            serde_json::json!({}),
        ] {
            let network = if foreign == serde_json::json!({}) {
                "other:network"
            } else {
                NETWORK
            };
            let packet = Signed(native::Packet::new(network, &foreign, &PACKETS).unwrap());
            let mut store = MemoryOperations::default();
            store
                .record([7; 32], &packet, Status::SubmissionUnknown)
                .unwrap();
            let chain = Arc::new(Mutex::new(Chain {
                outcome: Some(Status::Confirmed),
                ..Chain::default()
            }));
            let refused = run_on(chain.clone(), &mut store, 4).unwrap_err();
            assert!(
                refused.message.contains("another transaction"),
                "{refused:?}"
            );
            let chain = chain.lock().unwrap();
            assert_eq!((chain.sends, chain.lookups), (0, 0), "{foreign}");
        }
    }
    /// A shared journal that holds nothing at the first read and another transaction under the
    /// operation ID later: the call never takes that transaction's status for its own.
    #[test]
    fn a_journal_that_shows_another_transaction_later_stops_the_call() {
        /// Shows the foreign packet from the `from`th read on.
        struct Shared {
            loads: std::cell::Cell<usize>,
            from: usize,
            foreign: Signed,
        }
        impl OperationStore for Shared {
            fn load(&self, _: [u8; 32]) -> Result<Option<(Signed, Status)>> {
                self.loads.set(self.loads.get() + 1);
                Ok((self.loads.get() >= self.from)
                    .then(|| (self.foreign.clone(), Status::Submitted)))
            }
            fn record(&mut self, _: [u8; 32], _: &Signed, _: Status) -> Result<()> {
                Ok(())
            }
        }
        // From the read of `submit`, which refuses it, or from the last read, after this call's
        // own send.
        for from in [2, 3] {
            let mut store = Shared {
                loads: std::cell::Cell::new(0),
                from,
                foreign: Signed(
                    native::Packet::new(NETWORK, &serde_json::json!({ "hash": 9 }), &PACKETS)
                        .unwrap(),
                ),
            };
            let chain = Arc::new(Mutex::new(Chain {
                pending_lookups: 10,
                ..Chain::default()
            }));
            let refused = run_on(chain.clone(), &mut store, 1).unwrap_err();
            assert!(!names_transaction(&refused), "{from}: {refused:?}");
            assert!(
                refused.message.contains("another transaction"),
                "{from}: {refused:?}"
            );
            if from == 2 {
                let chain = chain.lock().unwrap();
                assert_eq!((chain.sends, chain.lookups), (0, 0));
            }
        }
    }
    /// A receipt of another transaction is no outcome of this one: the journal does not record
    /// its status, and the call returns no receipt.
    #[test]
    fn a_receipt_of_another_transaction_is_refused() {
        let (result, _, store) = run(Chain {
            outcome: Some(Status::Confirmed),
            receipt_hash: Some(9),
            ..Chain::default()
        });
        assert!(result.is_err());
        assert_ne!(status(&store), Some(Status::Confirmed));
    }
    /// A journal shared with another caller that sent the same transaction in the meantime: the
    /// read just before the send finds it pending, so the call only looks it up, and follows the
    /// retained packet, also when its signed bytes differ from this preparation's.
    #[test]
    fn a_send_that_another_caller_journaled_is_looked_up_not_sent_again() {
        /// Holds nothing at the first read, then `retained` as `Submitted`.
        struct Shared {
            loads: std::cell::Cell<usize>,
            retained: Signed,
            recorded: Vec<Status>,
        }
        impl OperationStore for Shared {
            fn load(&self, _: [u8; 32]) -> Result<Option<(Signed, Status)>> {
                self.loads.set(self.loads.get() + 1);
                Ok((self.loads.get() > 1).then(|| {
                    (
                        self.retained.clone(),
                        self.recorded.last().cloned().unwrap_or(Status::Submitted),
                    )
                }))
            }
            fn record(&mut self, _: [u8; 32], signed: &Signed, status: Status) -> Result<()> {
                assert_eq!(signed.bytes(), self.retained.bytes());
                self.recorded.push(status);
                Ok(())
            }
        }
        for retained in [
            // The same bytes as this preparation's: the first one.
            serde_json::json!({ "code": 1 }),
            // The same transaction with other signed bytes.
            serde_json::json!({ "code": 99 }),
        ] {
            let mut store = Shared {
                loads: std::cell::Cell::new(0),
                retained: Signed(native::Packet::new(NETWORK, &retained, &PACKETS).unwrap()),
                recorded: Vec::new(),
            };
            let chain = Arc::new(Mutex::new(Chain {
                // The first send has no receipt yet.
                pending_lookups: 2,
                outcome: Some(Status::Confirmed),
                ..Chain::default()
            }));
            assert!(
                run_on(chain.clone(), &mut store, 4).unwrap().is_some(),
                "{retained}"
            );
            let chain = chain.lock().unwrap();
            assert_eq!(chain.sends, 0, "{retained}");
            assert!(
                chain
                    .observed
                    .iter()
                    .all(|bytes| bytes == store.retained.bytes()),
                "{retained}"
            );
        }
    }
    /// A send carries the time of the call, which the preparation's reads of the network take
    /// part of: the clock starts when the call does.
    #[test]
    fn the_send_counts_the_preparation_in_its_clock() {
        let chain = Arc::new(Mutex::new(Chain {
            preparation_time: std::time::Duration::from_millis(1_100),
            outcome: Some(Status::Confirmed),
            ..Chain::default()
        }));
        let mut store = MemoryOperations::default();
        assert!(run_on(chain.clone(), &mut store, 1).unwrap().is_some());
        assert!(
            chain.lock().unwrap().sent_at[0] >= 2,
            "{:?}",
            chain.lock().unwrap().sent_at
        );
        let runtime = Runtime {
            provider: Box::new(Rail(chain.clone())),
        };
        let poll = Poll {
            attempts: 1,
            interval: std::time::Duration::ZERO,
        };
        let mut store = MemoryOperations::default();
        runtime
            .renew_code(&mut store, &[3; 32], 10_000, poll, 1)
            .unwrap();
        assert!(
            chain.lock().unwrap().sent_at[1] >= 2,
            "{:?}",
            chain.lock().unwrap().sent_at
        );
    }
    #[test]
    fn live_code_needs_no_transaction() {
        let (result, chain, store) = run(Chain {
            live: true,
            ..Chain::default()
        });
        assert!(result.unwrap().is_none());
        assert_eq!(chain.lock().unwrap().sends, 0);
        assert_eq!(status(&store), None);
        assert!(!store.pending());
    }
    #[test]
    fn a_confirmed_upload_is_retained_and_returned() {
        let (result, chain, store) = run(Chain {
            pending_lookups: 1,
            outcome: Some(Status::Confirmed),
            ..Chain::default()
        });
        let receipt = result.unwrap().unwrap();
        assert_eq!(receipt.info().unwrap().status, Status::Confirmed);
        assert_eq!(chain.lock().unwrap().sends, 1);
        assert_eq!(status(&store), Some(Status::Confirmed));
        assert!(!store.pending());
    }
    #[test]
    fn a_send_accepted_in_the_last_attempt_is_looked_up_in_it() {
        let chain = Arc::new(Mutex::new(Chain {
            outcome: Some(Status::Confirmed),
            ..Chain::default()
        }));
        let mut store = MemoryOperations::default();
        assert!(run_on(chain.clone(), &mut store, 1).unwrap().is_some());
        assert_eq!(chain.lock().unwrap().sends, 1);
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
        assert_eq!(status(&store), Some(Status::Confirmed));
        // The network accepts none of the sends: the journal holds nothing pending, and the
        // error names no transaction.
        let (result, chain, store) = run(Chain {
            answers: vec![Status::Prepared; 4],
            ..Chain::default()
        });
        let refused = result.unwrap_err();
        assert!(!names_transaction(&refused), "{refused:?}");
        assert_eq!(chain.lock().unwrap().sends, 4);
        assert_eq!(status(&store), Some(Status::Prepared));
        assert!(!store.pending());
    }
    #[test]
    fn a_check_that_fails_before_any_send_sends_nothing() {
        let (result, chain, store) = run(Chain {
            failing_checks: vec![1, 2, 3, 4],
            outcome: Some(Status::Confirmed),
            ..Chain::default()
        });
        let refused = result.unwrap_err();
        assert!(!names_transaction(&refused));
        assert_eq!(chain.lock().unwrap().sends, 0);
        assert_eq!(status(&store), None);
        assert!(!store.pending());
    }
    #[test]
    fn a_check_that_fails_after_a_send_that_was_not_accepted_stays_within_the_poll() {
        let (result, chain, _) = run(Chain {
            answers: vec![Status::Prepared],
            failing_checks: vec![2],
            outcome: Some(Status::Confirmed),
            ..Chain::default()
        });
        assert!(result.unwrap().is_some());
        assert_eq!(chain.lock().unwrap().sends, 2);
    }
    #[test]
    fn a_transaction_that_the_journal_holds_is_followed_by_its_retained_packet() {
        let chain = Arc::new(Mutex::new(Chain {
            answers: vec![Status::SubmissionUnknown],
            pending_lookups: 10,
            ..Chain::default()
        }));
        let mut store = MemoryOperations::default();
        assert!(run_on(chain.clone(), &mut store, 4).is_err());
        let (original, retained) = store.load([7; 32]).unwrap().unwrap();
        assert_eq!(retained, Status::SubmissionUnknown);
        // A later call prepares new bytes of the same transaction. The journal's packet is the
        // one that the network may hold: it is looked up, not sent again, and its status is
        // recorded.
        {
            let mut chain = chain.lock().unwrap();
            chain.pending_lookups = 1;
            chain.outcome = Some(Status::Confirmed);
            chain.sends = 0;
            chain.observed.clear();
        }
        assert!(run_on(chain.clone(), &mut store, 4).unwrap().is_some());
        let chain = chain.lock().unwrap();
        assert_eq!(chain.sends, 0);
        assert_eq!(chain.preparations, 2);
        assert!(chain.observed.iter().all(|bytes| bytes == original.bytes()));
        assert_eq!(status(&store), Some(Status::Confirmed));
    }
    #[test]
    fn a_send_that_the_journal_could_not_record_as_accepted_is_pending() {
        // The journal retains the transaction before the send (first write); the network
        // accepts it, and the journal cannot record that (second write).
        let chain = Arc::new(Mutex::new(Chain {
            pending_lookups: 10,
            ..Chain::default()
        }));
        let mut journal = Journal::failing_at(2);
        let pending = run_on(chain.clone(), &mut journal, 4).unwrap_err();
        assert!(names_transaction(&pending), "{pending:?}");
        assert!(pending.detail("nativeUnits").is_none());
        assert_eq!(status(&journal), Some(Status::SubmissionUnknown));
        assert!(journal.inner.pending());
        assert_eq!(chain.lock().unwrap().sends, 1);
    }
    #[test]
    fn a_send_stays_in_doubt_while_its_journal_cannot_be_read() {
        // The network accepts the send, and the journal can neither record that (second write)
        // nor be read afterwards: the transaction is looked up, never sent again.
        let chain = Arc::new(Mutex::new(Chain {
            pending_lookups: 1,
            outcome: Some(Status::Confirmed),
            ..Chain::default()
        }));
        let mut journal = Journal::lost_at(2);
        assert!(run_on(chain.clone(), &mut journal, 4).unwrap().is_some());
        assert_eq!(chain.lock().unwrap().sends, 1);
        // Without a receipt within the poll, the error names it as pending.
        let chain = Arc::new(Mutex::new(Chain {
            pending_lookups: 10,
            ..Chain::default()
        }));
        let mut journal = Journal::lost_at(2);
        let pending = run_on(chain.clone(), &mut journal, 4).unwrap_err();
        assert!(names_transaction(&pending), "{pending:?}");
        assert_eq!(chain.lock().unwrap().sends, 1);
    }
    #[test]
    fn live_code_after_the_last_refused_send_needs_nothing() {
        // Another transaction makes the code live during the one send that the poll allows,
        // which the network does not accept.
        let chain = Arc::new(Mutex::new(Chain {
            answers: vec![Status::Prepared],
            live_at_send: Some(1),
            ..Chain::default()
        }));
        let mut store = MemoryOperations::default();
        assert!(run_on(chain.clone(), &mut store, 1).unwrap().is_none());
        assert_eq!(chain.lock().unwrap().sends, 1);
        assert!(!store.pending());
    }
    #[test]
    fn a_failed_transaction_keeps_its_fee_when_the_journal_cannot_record_it() {
        // Writes: retained before the send, accepted, then the failure, which the journal
        // cannot record.
        let chain = Arc::new(Mutex::new(Chain {
            outcome: Some(Status::Failed),
            ..Chain::default()
        }));
        let mut journal = Journal::failing_at(3);
        let failed = run_on(chain, &mut journal, 4).unwrap_err();
        assert!(failed.message.contains("failed on the ledger"));
        assert!(names_transaction(&failed));
        assert_eq!(failed.detail("nativeUnits"), Some("250"));
    }
    #[test]
    fn lookups_that_fail_stay_within_the_poll() {
        let (result, chain, store) = run(Chain {
            failing_lookups: 2,
            outcome: Some(Status::Failed),
            ..Chain::default()
        });
        let failed = result.unwrap_err();
        assert!(failed.message.contains("failed on the ledger"));
        assert!(names_transaction(&failed));
        assert_eq!(failed.detail("nativeUnits"), Some("250"));
        assert_eq!(chain.lock().unwrap().lookups, 3);
        assert_eq!(status(&store), Some(Status::Failed));
        assert!(!store.pending());
    }
    #[test]
    fn a_transaction_without_a_receipt_is_pending_and_named() {
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
        assert!(names_transaction(&uncertain));
        assert!(uncertain.detail("nativeUnits").is_none());
        assert_eq!(chain.lock().unwrap().sends, 1);
        assert_eq!(status(&store), Some(Status::Submitted));
        assert!(store.pending());
    }
}
