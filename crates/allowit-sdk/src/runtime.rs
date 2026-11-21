//! Shared runtime orchestration over vendor-owned packets and native implementations.
pub mod owner;
use crate::{config::ServerAuthority, error::Result};
use allowit_core::vendor::runtime as native;
pub use native::{
    Accounting, Action, Debit, Evaluator, Expense, ExpenseBound, FeeCeiling, Lifetime,
    OWNER_TRANSACTION_EXPIRED, OperationOutcome, OwnerAction, OwnerInfo, OwnerSignedInfo,
    PacketInfo, ParamsUpdate, PolicyInstallation, Poll, Problem, ReceiptInfo, Resources,
    STALE_OWNER_OBSERVATION, SendingAccounts, Settlement, Signature, Signer, Snapshot, Status,
    Terms, Tier, Transport, Usage, Validity,
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
impl OwnerSigned {
    /// The step's identity in the rail's own bytes, its action and its time bound.
    pub fn info(&self) -> Result<OwnerSignedInfo> {
        self.0.provider().owner_signed_info(&self.0)
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
    /// When the receipt's operation settled, in Unix seconds, as its rail states it from the
    /// network (Core's `PacketProvider::receipt_settled_at`); `None` when the rail states none.
    pub fn settled_at(&self) -> Result<Option<u64>> {
        self.0.provider().receipt_settled_at(&self.0)
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
/// A journal of signed packets by operation ID, with the status of each.
///
/// A journal has one writer. Its reads and writes are no conditional claim: the SDK engine
/// decides from a read and then writes, so two writers of one journal can both send the same
/// transaction. A host that shares one journal between callers serialises their writers, for
/// example one call at a time per operation ID. A journal that lasts one call, as a server's
/// does, has one writer by construction.
pub trait OperationStore {
    fn load(&self, id: [u8; 32]) -> Result<Option<(Signed, Status)>>;
    fn record(&mut self, id: [u8; 32], signed: &Signed, status: Status) -> Result<()>;
}
/// What a journal holds of one owner-signed step.
#[derive(Clone, Debug)]
pub struct OwnerStepRecord {
    pub step: OwnerSigned,
    /// The wallet of the installation that the step acts on, or `None` for a step that leaves
    /// before its wallet is installed, as a creation.
    pub wallet: Option<String>,
    /// `Prepared`: not sent, and nothing is pending. `SubmissionUnknown`: possibly sent.
    /// `Submitted`: the network accepted it. `Confirmed` or `Failed`: its receipt.
    pub status: Status,
    /// The step reached its send cutoff ([`OwnerSignedInfo::expires_at`]) after it was possibly
    /// sent: it is never sent again, whatever a later clock says, and only its receipt or its
    /// lapse ends it.
    pub closed: bool,
}
/// A journal of owner-signed steps by their identity ([`OwnerSigned::info`]): the owner steps'
/// [`OperationStore`], with its one writer. A record binds a step's identity to its signed
/// bytes and to its installation. A store keeps what is final: it never reopens a closed record
/// and never replaces the status of a receipt.
pub trait OwnerStepStore {
    fn load(&self, identity: &[u8]) -> Result<Option<OwnerStepRecord>>;
    fn record(&mut self, identity: &[u8], record: &OwnerStepRecord) -> Result<()>;
}
/// A journal of owner steps held in memory, for a caller that keeps no durable one.
#[derive(Default)]
pub struct MemoryOwnerSteps(std::collections::BTreeMap<Vec<u8>, OwnerStepRecord>);
impl OwnerStepStore for MemoryOwnerSteps {
    fn load(&self, identity: &[u8]) -> Result<Option<OwnerStepRecord>> {
        Ok(self.0.get(identity).cloned())
    }
    fn record(&mut self, identity: &[u8], record: &OwnerStepRecord) -> Result<()> {
        if let Some(held) = self.0.get(identity) {
            same_owner_step(held, &record.step, record.wallet.as_deref())?;
            let ended = matches!(held.status, Status::Confirmed | Status::Failed);
            if (held.closed && !record.closed) || (ended && record.status != held.status) {
                return Err(crate::error::Error::config(
                    "Owner step record is final: it is not reopened",
                ));
            }
        }
        self.0.insert(identity.to_vec(), record.clone());
        Ok(())
    }
}
/// Refuse other signed bytes or another installation under the identity of the record `held`.
fn same_owner_step(held: &OwnerStepRecord, step: &OwnerSigned, wallet: Option<&str>) -> Result<()> {
    if held.step.bytes() != step.bytes() || held.step.network() != step.network() {
        return Err(crate::error::Error::config(
            "Owner step identity already binds different signed bytes",
        ));
    }
    if held.wallet.as_deref() != wallet {
        return Err(crate::error::Error::config(
            "Owner step identity already binds another installation",
        ));
    }
    Ok(())
}
/// The clock value of a check or a send in a call that began at `started` with the caller's
/// `now`: the second after `now` plus the whole seconds since `started`. `now` names a whole
/// second that the caller read before the call, so the value is not behind the caller's clock
/// when it is computed, for a call that began within that second. It is the value that the rail's
/// check receives, not the moment at which a request leaves: the time between the caller's
/// reading of `now` and the call, and between a check and the departure of its send, is not
/// counted. Core's cutoff is the caller's send rule and no proof of a lapse.
fn later(now: u64, started: std::time::Instant) -> u64 {
    now.saturating_add(1)
        .saturating_add(started.elapsed().as_secs())
}
/// Whether the refusal is the final one of a step at or past its send cutoff.
fn owner_step_expired(e: &crate::error::Error) -> bool {
    e.api_code.as_deref() == Some(OWNER_TRANSACTION_EXPIRED)
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
    /// What the receipt `r` of the operation `p`, carried by the signed envelope `signed`,
    /// changes, as the rail reads it ([`Settlement`]): the outcome, the usage, the operation's own
    /// fee, the counted gas, the next nonce and the gas carry. `accounting` is the server's account of the wallet before
    /// the receipt and `carry` its gas carry. The rail refuses a receipt that is not of `signed`'s
    /// transaction, or, for a payment authorization that another party settles, of that party's
    /// transaction, and a `signed` that does not carry `p`'s operation. A server applies the
    /// operation's usage and nonce with [`crate::accounting::apply_settlement`], charges the fee
    /// of a wallet that counts no gas itself as that function says, and stores the carry as
    /// given, once for each operation and [`Settlement::transaction`].
    pub fn settle_receipt(
        &self,
        p: &Prepared,
        signed: &Signed,
        r: &Receipt,
        accounting: Option<&Accounting>,
        carry: u128,
    ) -> Result<Settlement> {
        self.provider.settle_receipt(
            &native::Prepared(p.0.clone()),
            &native::Signed(signed.0.clone()),
            &native::Receipt(r.0.clone()),
            accounting,
            carry,
        )
    }
    /// The expense bound of `action` on the installation from the fee ceiling `ceiling`, as the
    /// rail values it: the principal, a vault operation's worst-case conversion and the fee
    /// ceiling.
    pub fn expense(
        &self,
        i: &Installation,
        action: &Action,
        ceiling: &FeeCeiling,
    ) -> Result<ExpenseBound> {
        self.provider.expense(&i.0, action, ceiling)
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
    /// `Prepared` is looked up by its retained packet, never sent again; a journal has one writer
    /// ([`OperationStore`]). Live code needs nothing, unless the journal holds a send of this
    /// transaction, whose receipt the call then returns. A receipt counts only when the rail
    /// verifies it as this transaction's. The clock of each send counts from the call's start, so
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
        self.send_code(store, Signed(prepared.0), poll, (now, started), &|| true)
    }
    /// Renew the live wallet code `code` that the server uploaded and that wallets share, such as
    /// the code of a fixed tier, from the server's sending accounts, which pay for it, with the
    /// network fee bounded by `max_fee`, once the rail says that it is due
    /// (`prepare_code_renewal`). `None` before. The renewal is a code transaction that takes the
    /// path of `make_code_live`, with its journal. Code that lapsed is not renewed: nothing
    /// restores it. The caller's `fence` decides just before each send: once it answers `false`,
    /// as for a round that lost its lease to another, nothing more is sent.
    pub fn renew_code(
        &self,
        store: &mut dyn OperationStore,
        code: &[u8; 32],
        max_fee: u64,
        poll: Poll,
        now: u64,
        fence: &dyn Fn() -> bool,
    ) -> Result<Option<Receipt>> {
        let started = std::time::Instant::now();
        let Some(prepared) = self.provider.prepare_code_renewal(code, max_fee, now)? else {
            return Ok(None);
        };
        self.send_code(store, Signed(prepared.0), poll, (now, started), fence)
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
        fence: &dyn Fn() -> bool,
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
        // A fence that refused once stays closed for the call: no send follows a refusal.
        let closed = std::cell::Cell::new(false);
        let fence = || {
            let open = !closed.get() && fence();
            closed.set(!open);
            open
        };
        // Live code needs nothing, unless a send of this transaction made it live: the journal
        // then holds that send, whose receipt is the call's outcome.
        let sent_elsewhere = |store: &dyn OperationStore| -> Result<Journaled> {
            Ok(this(store.load(id)?)?.filter(|(_, status)| *status != Status::Prepared))
        };
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
                    Ok(false) => match sent_elsewhere(&*store)? {
                        Some((original, journaled)) => {
                            signed = original;
                            status = journaled;
                        }
                        None => return Ok(None),
                    },
                    Err(error) => {
                        last = Some(error);
                        continue;
                    }
                }
            }
            if status == Status::Prepared {
                status = match self.send_unless_journaled(
                    store,
                    &mut signed,
                    id,
                    &this,
                    clock(),
                    &fence,
                ) {
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
                if status == Status::Prepared && closed.get() {
                    // The fence refused and the journal holds no send: nothing is awaited, and
                    // the call ends with the refusal.
                    return Err(last.unwrap_or_else(|| {
                        crate::error::Error::config(
                            "The caller's fence closed: the code transaction is not sent",
                        )
                    }));
                }
                if status == Status::Prepared {
                    // The network accepted no send. After the last one, live code needs nothing,
                    // unless another send of this transaction made it live.
                    if attempt + 1 == poll.attempts.max(1)
                        && matches!(
                            self.provider.code_needed(&native::Signed(signed.0.clone())),
                            Ok(false)
                        )
                    {
                        match sent_elsewhere(&*store)? {
                            Some((original, journaled)) => {
                                signed = original;
                                status = journaled;
                            }
                            None => return Ok(None),
                        }
                    } else {
                        continue;
                    }
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
    /// call follows, and a status past `Prepared` is looked up, never sent again. A journal has one
    /// writer ([`OperationStore`]).
    fn send_unless_journaled(
        &self,
        store: &mut dyn OperationStore,
        signed: &mut Signed,
        id: [u8; 32],
        this: &dyn Fn(Journaled) -> Result<Journaled>,
        now: u64,
        fence: &dyn Fn() -> bool,
    ) -> Result<Status> {
        if let Some((original, status)) = this(store.load(id)?)? {
            *signed = original;
            if status != Status::Prepared {
                return Ok(status);
            }
        }
        if !fence() {
            return Err(crate::error::Error::config(
                "The caller's fence closed: the code transaction is not sent",
            ));
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
    /// Send the owner-signed step `p` and wait for its receipt, in `poll.attempts` attempts and
    /// at least one. `on` is the installation that the step acts on, or `None` for a step that
    /// leaves before its wallet is installed, as a creation; every check and every send carries
    /// it. The rail checks the step ([`Self::validate_owner_signed`]) and `retain`
    /// keeps it before it leaves. It is sent once (`broadcast_owner`), again only while the
    /// network did not accept it yet, and then looked up until its receipt. A step at or past
    /// its time bound is never sent, before the call or during it: only its receipt answers, and
    /// without one its result stays unknown. `now` is a whole second that the caller read before
    /// the call: every check and send takes the second after it plus the whole seconds that the
    /// call has taken.
    ///
    /// An uncertain send ends the call, and so does a lookup that fails; a step without a
    /// receipt after the last attempt is an error too. Each of these errors names the step's
    /// transaction, and a failed lookup keeps its kind, code and details. Before any other send
    /// or any replacement the caller looks the step up by Core's rule: its receipt
    /// ([`Self::observe_owner`]), then what the network holds ([`Self::owner_status`]); a step
    /// that the network holds waits for its receipt. A send that the network rejects is the
    /// rail's error. A rail without Core's owner-step methods is refused by their default.
    pub fn submit_owner(
        &self,
        p: &OwnerSigned,
        on: Option<&Installation>,
        retain: impl FnOnce(&OwnerSigned) -> Result<()>,
        poll: Poll,
        now: u64,
    ) -> Result<Receipt> {
        // The call's time counts from its first statement.
        let started = std::time::Instant::now();
        let clock = || later(now, started);
        let step = native::OwnerSigned(p.0.clone());
        let on = on.map(|i| &i.0);
        let id = hex(&p.info()?.identity);
        // A lookup that failed: the result is unknown, and the failure keeps its kind, its code
        // and its details.
        let unknown = |e: crate::error::Error| crate::error::Error {
            message: format!(
                "Transaction {id} result is unknown ({}); look it up again before any replacement",
                e.message
            ),
            ..e
        };
        // At or past its bound a step is never sent: only its receipt answers.
        let past_bound = || match self.observe_owner(p) {
            Ok(Some(receipt)) => Ok(receipt),
            Ok(None) => Err(crate::error::Error::uncertain(
                "Original owner result remains unknown. Retain its packet; do not replace it.",
            )),
            Err(e) => Err(unknown(e)),
        };
        match self.provider.validate_owner_signed(&step, on, clock()) {
            Err(e) if owner_step_expired(&e) => return past_bound(),
            Err(e) => return Err(e),
            Ok(()) => {}
        }
        retain(p)?;
        let mut accepted = false;
        for attempt in 0..poll.attempts.max(1) {
            if attempt > 0 {
                std::thread::sleep(poll.interval);
            }
            if !accepted {
                match self.provider.broadcast_owner(&step, on, clock()) {
                    Ok(Status::Submitted) => accepted = true,
                    // Not accepted yet, and nothing is pending: the next attempt sends again.
                    Ok(Status::Prepared) => continue,
                    // Uncertain: the caller looks the step up before any other send.
                    Ok(_) => {
                        return Err(crate::error::Error::config(format!(
                            "Transaction {id} result is unknown (Uncertain native broadcast); look it up again before any replacement"
                        )));
                    }
                    Err(e) if owner_step_expired(&e) => return past_bound(),
                    Err(e) => return Err(e),
                }
            }
            match self.observe_owner(p) {
                Ok(Some(receipt)) => return Ok(receipt),
                Ok(None) => {}
                Err(e) => return Err(unknown(e)),
            }
        }
        Err(crate::error::Error::config(format!(
            "Transaction {id} has no terminal result yet; look it up again before any replacement"
        )))
    }
    pub fn observe_owner(&self, p: &OwnerSigned) -> Result<Option<Receipt>> {
        self.provider
            .observe_owner(&native::OwnerSigned(p.0.clone()))
            .map(|p| p.map(|p| Receipt(p.0)))
    }
    /// Send the owner-signed step `p` at most once through the journal `store`, keyed by the
    /// step's identity ([`OwnerSigned::info`]), by Core's rules for a caller
    /// (`RuntimeProvider::broadcast_owner`), and answer its recorded status. `on` is the
    /// installation that the step acts on, or `None` for a step that leaves before its wallet is
    /// installed, as a creation: the journal binds the step to it, and every check and every
    /// send carries it, so the rail reads the wallet's liveness before each send.
    ///
    /// A step that the journal holds as possibly sent is looked up first. Its receipt records
    /// and returns its final status (the receipt itself is [`Self::observe_owner`]'s). A step
    /// that the network accepted waits for its receipt and is not sent again, unless the rail
    /// shows that nothing of it is pending (`Prepared` of `owner_status`). After an uncertain
    /// send, the same bytes are sent again while the lookup stays inconclusive and the rail's
    /// check passes. A lookup that fails is that error: nothing is sent, and the record stays.
    ///
    /// The cutoff ([`OwnerSignedInfo::expires_at`]) is final. A possibly sent step is closed in
    /// the journal when the call's clock has reached it: the clock is read after each lookup,
    /// also one that failed, and after a send. At its cutoff no answer of a lookup clears it,
    /// and a closed step is looked up for its receipt only. A closed step is never sent again, also when a later call brings an earlier
    /// clock: only its receipt or its lapse ([`Self::owner_step_lapsed`]) ends it. A step that
    /// was never sent returns the rail's refusal, and at its cutoff the caller drops it.
    ///
    /// A step is recorded as possibly sent before it leaves. The answer of a send speaks for
    /// that send only: a send that the rail refused or the network rejected is that error, and
    /// it leaves a step that was never sent before as not sent, and an earlier uncertain send
    /// as possibly sent. A refusal for this attempt (`CONTRACT_EXPIRED`) and a check that got no
    /// answer are that error too: a possibly sent step stays as it was, a step that was never
    /// sent stays not sent, and a later call checks again. `now` is a whole second that the
    /// caller read before the call: every check and send takes the second after it plus the
    /// whole seconds that the call has taken. The answer is never final without a receipt.
    ///
    /// A journal write that fails ends the call with that failure, and the journal then holds
    /// what it held before that write. So a step that the rail did not send can stay recorded as
    /// possibly sent, and a step that the network accepted as possibly sent only: a later call
    /// looks it up, and Core lets the same bytes leave again while that lookup stays
    /// inconclusive. A journal that loses the write that closes a step can reopen it only for a
    /// caller whose clock goes back.
    pub fn submit_owner_step(
        &self,
        store: &mut dyn OwnerStepStore,
        p: &OwnerSigned,
        on: Option<&Installation>,
        now: u64,
    ) -> Result<Status> {
        // The call's time counts from its first statement.
        let started = std::time::Instant::now();
        let clock = || later(now, started);
        let info = p.info()?;
        let id = info.identity;
        let step = native::OwnerSigned(p.0.clone());
        let wallet = on.map(|i| i.0.description.wallet.as_str());
        let on = on.map(|i| &i.0);
        let mut record = match store.load(&id)? {
            Some(held) => {
                same_owner_step(&held, p, wallet)?;
                held
            }
            None => OwnerStepRecord {
                step: p.clone(),
                wallet: wallet.map(str::to_owned),
                status: Status::Prepared,
                closed: false,
            },
        };
        if matches!(record.status, Status::Confirmed | Status::Failed) {
            return Ok(record.status);
        }
        // The cutoff is final for a possibly sent step: the step closes when the call's clock
        // has reached it. A lookup and a send take time, so the clock is read after each
        // lookup, also one that failed, and after a send.
        let close_at_cutoff = |record: &mut OwnerStepRecord, store: &mut dyn OwnerStepStore| {
            if record.status != Status::Prepared && !record.closed && clock() >= info.expires_at {
                record.closed = true;
                store.record(&id, record)?;
            }
            Ok::<(), crate::error::Error>(())
        };
        if record.status != Status::Prepared {
            let receipt = self.observe_owner(p);
            close_at_cutoff(&mut record, store)?;
            if let Some(receipt) = receipt? {
                record.status = receipt.info()?.status;
                store.record(&id, &record)?;
                return Ok(record.status);
            }
            if record.closed {
                return Ok(record.status);
            }
            let held = self.provider.owner_status(&step);
            close_at_cutoff(&mut record, store)?;
            let held = held?;
            // At its cutoff a possibly sent step stays so, whatever this lookup answered.
            if record.closed {
                return Ok(record.status);
            }
            match held {
                Status::Submitted => {
                    if record.status != Status::Submitted {
                        record.status = Status::Submitted;
                        store.record(&id, &record)?;
                    }
                    return Ok(record.status);
                }
                // The rail's evidence that the network did not accept it and nothing is pending.
                Status::Prepared => {
                    record.status = Status::Prepared;
                    store.record(&id, &record)?;
                }
                // An accepted step waits for its receipt.
                _ if record.status == Status::Submitted => return Ok(record.status),
                // Inconclusive after an uncertain send: the same bytes may be sent again.
                _ => {}
            }
        }
        let possibly_sent = record.status != Status::Prepared;
        // An attempt that the rail refused or the network rejected. The refusal of the cutoff
        // closes a possibly sent step, and so does the call's clock; any other refusal leaves
        // the step as it was.
        let refused = |e: crate::error::Error,
                       record: &mut OwnerStepRecord,
                       store: &mut dyn OwnerStepStore| {
            if possibly_sent && owner_step_expired(&e) {
                if !record.closed {
                    record.closed = true;
                    store.record(&id, record)?;
                }
                return Ok(record.status.clone());
            }
            close_at_cutoff(record, store)?;
            Err(e)
        };
        if let Err(e) = self.provider.validate_owner_signed(&step, on, clock()) {
            return refused(e, &mut record, store);
        }
        if !possibly_sent {
            record.status = Status::SubmissionUnknown;
            store.record(&id, &record)?;
        }
        match self.provider.broadcast_owner(&step, on, clock()) {
            Ok(Status::Submitted) => record.status = Status::Submitted,
            Ok(Status::Prepared) if !possibly_sent => record.status = Status::Prepared,
            // Uncertain, or not accepted after an earlier uncertain send: possibly sent.
            Ok(_) => {}
            // The rail refused this send or the network rejected it: nothing of it is pending,
            // and a step that was never sent before is recorded as not sent.
            Err(e) => {
                if !possibly_sent {
                    record.status = Status::Prepared;
                    store.record(&id, &record)?;
                }
                return refused(e, &mut record, store);
            }
        }
        // The send took time too.
        if record.status != Status::Prepared && clock() >= info.expires_at {
            record.closed = true;
        }
        store.record(&id, &record)?;
        Ok(record.status)
    }
    /// Whether the caller may send the owner-signed step at `now`, by the rail's rules. `on` is
    /// the installation that the step acts on, or `None` for a step that leaves before its
    /// wallet is installed, as a creation: the caller's record of the step binds it to its
    /// installation, and the rail reads the liveness of that installation's wallet. A typed
    /// refusal: `OWNER_TRANSACTION_EXPIRED` at its send cutoff, for good, or `CONTRACT_EXPIRED`
    /// for this attempt, while the wallet of `on` is lapsed; the step that renews the wallet's
    /// own lifetime is never refused for a lapse. An uncertain error when a read failed: it is
    /// no lapse and no pass. Without an installation the rail checks the cutoff alone.
    pub fn validate_owner_signed(
        &self,
        p: &OwnerSigned,
        on: Option<&Installation>,
        now: u64,
    ) -> Result<()> {
        self.provider.validate_owner_signed(
            &native::OwnerSigned(p.0.clone()),
            on.map(|i| &i.0),
            now,
        )
    }
    /// Send the owner-signed step once, after the rail's checks
    /// ([`Self::validate_owner_signed`], for the same `on`); never a final status.
    pub fn broadcast_owner(
        &self,
        p: &OwnerSigned,
        on: Option<&Installation>,
        now: u64,
    ) -> Result<Status> {
        self.provider
            .broadcast_owner(&native::OwnerSigned(p.0.clone()), on.map(|i| &i.0), now)
    }
    /// What the network holds of the owner-signed step now; never a final status.
    pub fn owner_status(&self, p: &OwnerSigned) -> Result<Status> {
        self.provider
            .owner_status(&native::OwnerSigned(p.0.clone()))
    }
    /// Whether the sent owner step lapsed: it never applied and can no longer apply. The server
    /// prepared it at height `since`. `false` means not proven.
    pub fn owner_step_lapsed(&self, p: &OwnerSigned, since: u64) -> Result<bool> {
        self.provider
            .owner_step_lapsed(&native::OwnerSigned(p.0.clone()), since)
    }
    /// Whether the wallet of the installation is closed: a wallet kind that closes is gone from
    /// its network.
    pub fn wallet_closed(&self, i: &Installation) -> Result<bool> {
        self.provider.wallet_closed(&i.0)
    }
    /// The owner step that closes the wallet of the installation, for a wallet kind that closes.
    pub fn prepare_close(&self, i: &Installation, fee: u64, now: u64) -> Result<OwnerPacket> {
        self.provider
            .prepare_close(&i.0, fee, now)
            .map(|p| OwnerPacket(p.0))
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
        /// What each settlement call carried: the prepared, signed and receipt packets, the
        /// nonce of the accounting and the carry.
        settled: Vec<(String, String, String, Option<u64>, u128)>,
        /// What each expense call carried: the installation's wallet, the transfer's recipient
        /// and units, and the ceiling.
        expensed: Vec<(String, String, i128, FeeCeiling)>,
        /// The time bound of the owner step.
        owner_expires_at: u64,
        /// The answers of the next sends of the owner step; `Submitted` once they are used up.
        owner_answers: Vec<Status>,
        owner_sends: usize,
        /// The lookups of the owner step after which the network holds its receipt.
        owner_receipt_after: Option<usize>,
        owner_lookups: usize,
        /// The network holds the owner step pending.
        owner_held: bool,
        /// The rail shows that the network did not accept the owner step and holds nothing.
        owner_dropped: bool,
        /// How long a lookup of the owner step's receipt takes.
        owner_lookup_time: std::time::Duration,
        /// How long a lookup of what the network holds of the owner step takes.
        owner_status_time: std::time::Duration,
        owner_status_lookups: usize,
        /// How long a send of the owner step takes.
        owner_send_time: std::time::Duration,
        /// The checks of the owner step that pass before `owner_refusal` applies.
        owner_checks_before_refusal: usize,
        /// The lookups of what the network holds of the owner step fail.
        owner_status_fails: bool,
        /// The rail's check of the owner step gets no answer.
        owner_check_unanswered: bool,
        /// The lookups of the owner step's receipt fail with this error.
        owner_lookup_fails: Option<crate::error::Error>,
        owner_failed_lookups: usize,
        /// The network rejects each send of the owner step.
        owner_rejected: bool,
        /// The owner step's receipt is of a failed transaction.
        owner_receipt_failed: bool,
        /// The typed refusal of the rail's check of the owner step.
        owner_refusal: Option<&'static str>,
        /// The packet of each check, send, lookup of the network's hold and lapse proof.
        owner_packets: Vec<String>,
        /// The clock of each check and of each send of the owner step.
        owner_checked_at: Vec<u64>,
        owner_sent_at: Vec<u64>,
        /// The wallet of the installation that each check of the owner step carried.
        owner_checked_on: Vec<Option<String>>,
        /// What each lapse proof, closed-wallet read and close preparation carried.
        owner_lapse_since: Vec<u64>,
        owner_closed_wallets: Vec<String>,
        owner_closes: Vec<(String, u64, u64)>,
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
        /// The one owner step: identity `[9; 32]`, its time bound as its document names it.
        fn owner_signed_info(&self, p: &Packet) -> Result<native::OwnerSignedInfo> {
            let document: serde_json::Value = serde_json::from_str(p.document().get()).unwrap();
            Ok(native::OwnerSignedInfo {
                action: "fund".into(),
                identity: vec![9; 32],
                expires_at: document["expires"].as_u64().unwrap(),
            })
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
            let time = self.0.lock().unwrap().owner_lookup_time;
            std::thread::sleep(time);
            let mut chain = self.0.lock().unwrap();
            if let Some(error) = chain.owner_lookup_fails.clone() {
                chain.owner_failed_lookups += 1;
                return Err(error);
            }
            chain.owner_lookups += 1;
            let held = chain
                .owner_receipt_after
                .is_some_and(|n| chain.owner_lookups > n);
            Ok(held.then(|| owner_receipt(!chain.owner_receipt_failed)))
        }
        fn validate_owner_signed(
            &self,
            p: &native::OwnerSigned,
            on: Option<&native::Installation>,
            now: u64,
        ) -> Result<()> {
            let mut chain = self.0.lock().unwrap();
            chain.owner_packets.push(p.0.document().get().into());
            chain.owner_checked_at.push(now);
            chain
                .owner_checked_on
                .push(on.map(|i| i.description.wallet.clone()));
            if now >= chain.owner_expires_at {
                return Err(crate::error::Error::coded(
                    OWNER_TRANSACTION_EXPIRED,
                    "The owner step passed its time bound",
                ));
            }
            if chain.owner_check_unanswered {
                return Err(crate::error::Error::uncertain("The check got no answer"));
            }
            match chain.owner_refusal {
                Some(code) if chain.owner_checked_at.len() > chain.owner_checks_before_refusal => {
                    Err(crate::error::Error::coded(
                        code,
                        "The rail refuses the step",
                    ))
                }
                _ => Ok(()),
            }
        }
        fn broadcast_owner(
            &self,
            p: &native::OwnerSigned,
            on: Option<&native::Installation>,
            now: u64,
        ) -> Result<Status> {
            self.validate_owner_signed(p, on, now)?;
            let time = self.0.lock().unwrap().owner_send_time;
            std::thread::sleep(time);
            let mut chain = self.0.lock().unwrap();
            chain.owner_packets.push(p.0.document().get().into());
            chain.owner_sends += 1;
            chain.owner_sent_at.push(now);
            if chain.owner_rejected {
                return Err(crate::error::Error::config("The network rejected the send"));
            }
            let answer = if chain.owner_answers.is_empty() {
                Status::Submitted
            } else {
                chain.owner_answers.remove(0)
            };
            // A network that accepted the step holds something of it again.
            if answer == Status::Submitted {
                chain.owner_dropped = false;
            }
            Ok(answer)
        }
        fn owner_status(&self, p: &native::OwnerSigned) -> Result<Status> {
            let time = self.0.lock().unwrap().owner_status_time;
            std::thread::sleep(time);
            let mut chain = self.0.lock().unwrap();
            chain.owner_packets.push(p.0.document().get().into());
            chain.owner_status_lookups += 1;
            if chain.owner_status_fails {
                return Err(crate::error::Error::uncertain("The lookup got no answer"));
            }
            Ok(if chain.owner_held {
                Status::Submitted
            } else if chain.owner_dropped {
                Status::Prepared
            } else {
                Status::SubmissionUnknown
            })
        }
        fn owner_step_lapsed(&self, p: &native::OwnerSigned, since: u64) -> Result<bool> {
            let mut chain = self.0.lock().unwrap();
            chain.owner_packets.push(p.0.document().get().into());
            chain.owner_lapse_since.push(since);
            // A step that the network holds did not lapse.
            Ok(!chain.owner_held)
        }
        fn wallet_closed(&self, i: &native::Installation) -> Result<bool> {
            let mut chain = self.0.lock().unwrap();
            chain
                .owner_closed_wallets
                .push(i.description.wallet.clone());
            Ok(true)
        }
        fn prepare_close(
            &self,
            i: &native::Installation,
            fee: u64,
            now: u64,
        ) -> Result<native::OwnerPacket> {
            let mut chain = self.0.lock().unwrap();
            chain
                .owner_closes
                .push((i.description.wallet.clone(), fee, now));
            Ok(native::OwnerPacket(
                native::Packet::new(NETWORK, &serde_json::json!({"close": 1}), &PACKETS).unwrap(),
            ))
        }
        /// The settlement of any receipt: executed, for the carry that the call gave plus one.
        fn settle_receipt(
            &self,
            p: &native::Prepared,
            signed: &native::Signed,
            r: &native::Receipt,
            accounting: Option<&native::Accounting>,
            carry: u128,
        ) -> Result<Settlement> {
            self.0.lock().unwrap().settled.push((
                p.0.document().get().into(),
                signed.0.document().get().into(),
                r.0.document().get().into(),
                accounting.map(|a| a.nonce),
                carry,
            ));
            Ok(Settlement {
                transaction: vec![8; 32],
                outcome: OperationOutcome::Executed,
                usage: None,
                fee_units: 250,
                counted_gas: None,
                next_nonce: accounting.map(|a| a.nonce + 1),
                gas_carry: carry + 1,
            })
        }
        /// The bound of a transfer, as Core states it: no conversion, its amount plus the
        /// ceiling's gas, and the ceiling's fee, height and day.
        fn expense(
            &self,
            i: &native::Installation,
            action: &Action,
            ceiling: &FeeCeiling,
        ) -> Result<ExpenseBound> {
            let Action::Transfer { recipient, units } = action else {
                unimplemented!()
            };
            self.0.lock().unwrap().expensed.push((
                i.description.wallet.clone(),
                recipient.clone(),
                *units,
                *ceiling,
            ));
            Ok(ExpenseBound {
                gas_units: ceiling.gas_units,
                conversion_units: 0,
                spent_units: units + ceiling.gas_units,
                max_fee_units: ceiling.max_fee_units,
                observed_height: ceiling.observed_height,
                day: ceiling.day,
            })
        }
    }
    /// The receipt of the owner step, confirmed or failed.
    fn owner_receipt(confirmed: bool) -> native::Receipt {
        let status = if confirmed { "confirmed" } else { "failed" };
        native::Receipt(
            native::Packet::new(NETWORK, &serde_json::json!({ "status": status }), &PACKETS)
                .unwrap(),
        )
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
            .renew_code(&mut store, &[3; 32], 10_000, poll, 1, &|| true)
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
                .renew_code(&mut store, &[3; 32], 10_000, poll, 1, &|| true)
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
    /// A journal whose content changes between reads, as a durable one after another run: it
    /// holds nothing at the first read and another transaction under the
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
    /// The caller's fence decides just before each send: a renewal whose round lost its lease
    /// sends nothing, and one that holds it sends.
    #[test]
    fn a_renewal_sends_nothing_once_its_fence_closes() {
        let poll = || Poll {
            attempts: 2,
            interval: std::time::Duration::ZERO,
        };
        for open in [false, true] {
            let chain = Arc::new(Mutex::new(Chain {
                outcome: Some(Status::Confirmed),
                ..Chain::default()
            }));
            let runtime = Runtime {
                provider: Box::new(Rail(chain.clone())),
            };
            let mut store = MemoryOperations::default();
            let renewed = runtime.renew_code(&mut store, &[3; 32], 10_000, poll(), 1, &|| open);
            assert_eq!(renewed.is_ok(), open);
            assert_eq!(chain.lock().unwrap().sends, usize::from(open));
            assert!(!store.pending());
        }
    }
    /// A fence that refused once stays closed for the call, also when it would open again: a
    /// round that lost its lease for a moment sends nothing more.
    #[test]
    fn a_fence_that_refused_once_stays_closed() {
        let chain = Arc::new(Mutex::new(Chain {
            outcome: Some(Status::Confirmed),
            ..Chain::default()
        }));
        let runtime = Runtime {
            provider: Box::new(Rail(chain.clone())),
        };
        let asked = std::cell::Cell::new(0);
        let fence = || {
            asked.set(asked.get() + 1);
            asked.get() > 1
        };
        let poll = Poll {
            attempts: 3,
            interval: std::time::Duration::ZERO,
        };
        let mut store = MemoryOperations::default();
        let refused = runtime
            .renew_code(&mut store, &[3; 32], 10_000, poll, 1, &fence)
            .unwrap_err();
        assert!(refused.message.contains("fence"), "{}", refused.message);
        let chain = chain.lock().unwrap();
        assert_eq!(chain.sends, 0);
        // Nothing was sent, so nothing is awaited: the call ends with the refusal, after one
        // check of the code and no lookup, and holds its sending account no longer.
        assert_eq!((chain.checks, chain.lookups), (1, 0));
    }
    /// Code that an earlier send of this transaction, which the journal holds, made live: the call
    /// follows that send to its receipt instead of answering that nothing was needed.
    #[test]
    fn code_made_live_by_this_transaction_elsewhere_returns_its_receipt() {
        /// Holds nothing at the first read, then this transaction as confirmed.
        struct Shared {
            loads: std::cell::Cell<usize>,
            sent: Signed,
        }
        impl OperationStore for Shared {
            fn load(&self, _: [u8; 32]) -> Result<Option<(Signed, Status)>> {
                self.loads.set(self.loads.get() + 1);
                Ok((self.loads.get() > 1).then(|| (self.sent.clone(), Status::Confirmed)))
            }
            fn record(&mut self, _: [u8; 32], _: &Signed, _: Status) -> Result<()> {
                Ok(())
            }
        }
        let mut store = Shared {
            loads: std::cell::Cell::new(0),
            sent: Signed(
                native::Packet::new(NETWORK, &serde_json::json!({ "code": 1 }), &PACKETS).unwrap(),
            ),
        };
        // The other send made the code live before this call checked it.
        let chain = Arc::new(Mutex::new(Chain {
            live_at_send: None,
            outcome: Some(Status::Confirmed),
            ..Chain::default()
        }));
        let runtime = Runtime {
            provider: Box::new(Rail(chain.clone())),
        };
        let prepared = Signed(
            native::Packet::new(NETWORK, &serde_json::json!({ "code": 1 }), &PACKETS).unwrap(),
        );
        chain.lock().unwrap().live = true;
        let poll = Poll {
            attempts: 2,
            interval: std::time::Duration::ZERO,
        };
        let receipt = runtime
            .send_code(
                &mut store,
                prepared,
                poll,
                (1, std::time::Instant::now()),
                &|| true,
            )
            .unwrap();
        assert!(receipt.is_some());
        assert_eq!(chain.lock().unwrap().sends, 0);
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
            .renew_code(&mut store, &[3; 32], 10_000, poll, 1, &|| true)
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
    /// The owner step `{"expires": expires}`, and a runtime over a network as `chain` sets it.
    fn owner_run(chain: Chain, expires: u64) -> (Runtime, Arc<Mutex<Chain>>, OwnerSigned) {
        let chain = Arc::new(Mutex::new(Chain {
            owner_expires_at: expires,
            ..chain
        }));
        let runtime = Runtime {
            provider: Box::new(Rail(chain.clone())),
        };
        let step = OwnerSigned(
            native::Packet::new(
                NETWORK,
                &serde_json::json!({ "expires": expires }),
                &PACKETS,
            )
            .unwrap(),
        );
        (runtime, chain, step)
    }
    const OWNER_POLL: Poll = Poll {
        attempts: 4,
        interval: std::time::Duration::ZERO,
    };
    /// The code of an uncertain error.
    const UNCERTAIN: i32 = 5;
    /// Submit the step at `now`: the result, and how often it was retained. A step is retained
    /// before anything of it is sent.
    fn submit(
        runtime: &Runtime,
        chain: &Arc<Mutex<Chain>>,
        step: &OwnerSigned,
        now: u64,
    ) -> (Result<Receipt>, usize) {
        let retained = std::cell::Cell::new(0);
        let result = runtime.submit_owner(
            step,
            None,
            |_| {
                assert_eq!(chain.lock().unwrap().owner_sends, 0);
                retained.set(retained.get() + 1);
                Ok(())
            },
            OWNER_POLL,
            now,
        );
        (result, retained.get())
    }
    /// A step is checked, retained, sent once and followed to its receipt. The check and the
    /// send carry the second after the caller's `now`.
    #[test]
    fn an_owner_step_is_retained_sent_once_and_followed_to_its_receipt() {
        let (runtime, chain, step) = owner_run(
            Chain {
                owner_receipt_after: Some(1),
                ..Chain::default()
            },
            100,
        );
        let (receipt, retained) = submit(&runtime, &chain, &step, 10);
        assert_eq!(receipt.unwrap().info().unwrap().status, Status::Confirmed);
        let chain = chain.lock().unwrap();
        assert_eq!(
            (retained, chain.owner_sends, chain.owner_lookups),
            (1, 1, 2)
        );
        assert_eq!(chain.owner_sent_at, [11]);
        assert!(chain.owner_checked_at.iter().all(|at| *at == 11));
        drop(chain);
        // The receipt of a failed transaction is the step's receipt too.
        let (runtime, chain, step) = owner_run(
            Chain {
                owner_receipt_after: Some(0),
                owner_receipt_failed: true,
                ..Chain::default()
            },
            100,
        );
        let receipt = submit(&runtime, &chain, &step, 10).0.unwrap();
        assert_eq!(receipt.info().unwrap().status, Status::Failed);
    }
    /// A step that the network did not accept yet is sent again with no lookup between. A step
    /// without a receipt after the last attempt is an error that names its transaction.
    #[test]
    fn a_step_is_sent_again_while_the_network_did_not_accept_it() {
        let (runtime, chain, step) = owner_run(
            Chain {
                owner_answers: vec![Status::Prepared, Status::Submitted],
                owner_receipt_after: Some(0),
                ..Chain::default()
            },
            100,
        );
        submit(&runtime, &chain, &step, 10).0.unwrap();
        let seen = chain.lock().unwrap();
        assert_eq!((seen.owner_sends, seen.owner_lookups), (2, 1));
        drop(seen);
        // Accepted at once: one send and a lookup in each of the four attempts. Never accepted:
        // a send in each attempt and no lookup.
        for (answers, sends, lookups) in [(vec![], 1, 4), (vec![Status::Prepared; 4], 4, 0)] {
            let (runtime, chain, step) = owner_run(
                Chain {
                    owner_answers: answers,
                    ..Chain::default()
                },
                100,
            );
            let error = submit(&runtime, &chain, &step, 10).0.unwrap_err();
            assert!(
                error.message.contains("has no terminal result yet"),
                "{error:?}"
            );
            assert!(error.message.contains(&hex(&[9; 32])), "{error:?}");
            let seen = chain.lock().unwrap();
            assert_eq!((seen.owner_sends, seen.owner_lookups), (sends, lookups));
        }
        // A poll of no attempt makes one.
        let (runtime, chain, step) = owner_run(Chain::default(), 100);
        let none = Poll {
            attempts: 0,
            interval: std::time::Duration::ZERO,
        };
        assert!(
            runtime
                .submit_owner(&step, None, |_| Ok(()), none, 10)
                .is_err()
        );
        let seen = chain.lock().unwrap();
        assert_eq!((seen.owner_sends, seen.owner_lookups), (1, 1));
    }
    /// An uncertain send ends the call: the error names the transaction, and nothing is sent
    /// or looked up after it. The caller looks the step up before any other send.
    #[test]
    fn an_uncertain_send_ends_the_call() {
        let (runtime, chain, step) = owner_run(
            Chain {
                owner_answers: vec![Status::SubmissionUnknown],
                owner_receipt_after: Some(0),
                ..Chain::default()
            },
            100,
        );
        let (result, retained) = submit(&runtime, &chain, &step, 10);
        let error = result.unwrap_err();
        assert_eq!(
            error.message,
            format!(
                "Transaction {} result is unknown (Uncertain native broadcast); look it up again before any replacement",
                hex(&[9; 32])
            )
        );
        assert!(error.config);
        let seen = chain.lock().unwrap();
        assert_eq!((retained, seen.owner_sends, seen.owner_lookups), (1, 1, 0));
    }
    /// A lookup that fails ends the call and sends nothing more, after an accepted send and for
    /// a step at its bound. The error names the transaction and keeps the failure's kind, code,
    /// status and details.
    #[test]
    fn a_lookup_that_fails_ends_the_call_with_its_own_failure() {
        let busy = crate::error::Error::refused(503, "lookup_unavailable", "busy")
            .with_details(serde_json::json!({"reason": "busy"}));
        for (now, sends, failure) in [
            (10, 1, crate::error::Error::uncertain("no answer")),
            (10, 1, busy.clone()),
            // At its bound the step is not sent, and its lookup fails.
            (99, 0, busy),
        ] {
            let (runtime, chain, step) = owner_run(
                Chain {
                    owner_lookup_fails: Some(failure.clone()),
                    ..Chain::default()
                },
                100,
            );
            let error = submit(&runtime, &chain, &step, now).0.unwrap_err();
            assert_eq!(
                error.message,
                format!(
                    "Transaction {} result is unknown ({}); look it up again before any replacement",
                    hex(&[9; 32]),
                    failure.message
                )
            );
            assert_eq!(
                (error.code, error.status, &error.api_code, &error.details),
                (
                    failure.code,
                    failure.status,
                    &failure.api_code,
                    &failure.details
                )
            );
            // The first lookup that fails ends the call: no other lookup or send follows.
            let seen = chain.lock().unwrap();
            assert_eq!((seen.owner_sends, seen.owner_failed_lookups), (sends, 1));
        }
        assert_eq!(crate::error::Error::uncertain("").code, UNCERTAIN);
    }
    /// A step that the caller could not retain is not sent: the call ends with the caller's
    /// error.
    #[test]
    fn a_step_that_is_not_retained_is_not_sent() {
        let (runtime, chain, step) = owner_run(Chain::default(), 100);
        let error = runtime
            .submit_owner(
                &step,
                None,
                |_| Err(crate::error::Error::config("The store is full")),
                OWNER_POLL,
                10,
            )
            .unwrap_err();
        assert_eq!(error.message, "The store is full");
        let seen = chain.lock().unwrap();
        assert_eq!((seen.owner_sends, seen.owner_lookups), (0, 0));
    }
    /// A send that the network rejects is the rail's error: the step was retained, and nothing
    /// is sent or looked up after it.
    #[test]
    fn a_rejected_send_is_the_rails_error() {
        let (runtime, chain, step) = owner_run(
            Chain {
                owner_rejected: true,
                ..Chain::default()
            },
            100,
        );
        let (result, retained) = submit(&runtime, &chain, &step, 10);
        assert_eq!(result.unwrap_err().message, "The network rejected the send");
        let seen = chain.lock().unwrap();
        assert_eq!((retained, seen.owner_sends, seen.owner_lookups), (1, 1, 0));
    }
    /// A step at or past its time bound is never retained or sent: only its receipt answers,
    /// and without one its result stays unknown. The caller's `now` is a whole second read
    /// before the call, so the last second before the bound is past it too.
    #[test]
    fn a_step_past_its_time_bound_is_never_sent() {
        for now in [9, 10, 11] {
            let (runtime, chain, step) = owner_run(
                Chain {
                    owner_receipt_after: Some(0),
                    ..Chain::default()
                },
                10,
            );
            let (receipt, retained) = submit(&runtime, &chain, &step, now);
            assert_eq!(receipt.unwrap().info().unwrap().status, Status::Confirmed);
            assert_eq!((retained, chain.lock().unwrap().owner_sends), (0, 0));
            let (runtime, chain, step) = owner_run(Chain::default(), 10);
            let error = submit(&runtime, &chain, &step, now).0.unwrap_err();
            assert_eq!(error.code, UNCERTAIN, "{now}");
            assert_eq!(chain.lock().unwrap().owner_sends, 0);
        }
        // Two seconds before the bound the step is sent.
        let (runtime, chain, step) = owner_run(
            Chain {
                owner_receipt_after: Some(0),
                ..Chain::default()
            },
            10,
        );
        submit(&runtime, &chain, &step, 8).0.unwrap();
        assert_eq!(chain.lock().unwrap().owner_sent_at, [9]);
    }
    /// A step that the rail refuses, as a funding of a lapsed wallet, is neither retained nor
    /// sent, and the refusal keeps its code.
    #[test]
    fn a_refused_step_is_neither_retained_nor_sent() {
        let (runtime, chain, step) = owner_run(
            Chain {
                owner_refusal: Some(native::CONTRACT_EXPIRED),
                ..Chain::default()
            },
            100,
        );
        let (result, retained) = submit(&runtime, &chain, &step, 10);
        assert_eq!(
            result.unwrap_err().api_code.as_deref(),
            Some(native::CONTRACT_EXPIRED)
        );
        assert_eq!((retained, chain.lock().unwrap().owner_sends), (0, 0));
    }
    /// Each attempt is checked at its own clock: a step whose cutoff passes while the call
    /// waits is not sent again, and answers as a step past its bound.
    #[test]
    fn a_later_attempt_is_checked_at_its_own_clock() {
        let (runtime, chain, step) = owner_run(
            Chain {
                owner_answers: vec![Status::Prepared; 3],
                ..Chain::default()
            },
            12,
        );
        let error = runtime
            .submit_owner(
                &step,
                None,
                |_| Ok(()),
                Poll {
                    attempts: 3,
                    interval: std::time::Duration::from_millis(1_050),
                },
                10,
            )
            .unwrap_err();
        assert_eq!(error.code, UNCERTAIN);
        let seen = chain.lock().unwrap();
        assert_eq!(
            (&seen.owner_sent_at[..], seen.owner_lookups),
            (&[11][..], 1)
        );
        // The second check came a second or more after the first.
        assert!(seen.owner_checked_at.last().is_some_and(|at| *at >= 12));
    }
    /// The installation `wallet`.
    fn installed(wallet: &str) -> Installation {
        let mut installation = installation();
        installation.0.description.wallet = wallet.into();
        installation
    }
    /// Every check and every send of the polling form carries the step's installation, and a
    /// refusal for this attempt is the rail's error: nothing is retained or sent.
    #[test]
    fn the_polling_form_carries_the_installation_at_every_check_and_send() {
        let (runtime, chain, step) = owner_run(
            Chain {
                owner_answers: vec![Status::Prepared; 2],
                owner_receipt_after: Some(0),
                ..Chain::default()
            },
            100,
        );
        let wallet = installed("wallet");
        runtime
            .submit_owner(&step, Some(&wallet), |_| Ok(()), OWNER_POLL, 10)
            .unwrap();
        let seen = chain.lock().unwrap();
        // The first check, and three sends with the check of each.
        assert_eq!(seen.owner_sends, 3);
        assert_eq!(seen.owner_checked_on, vec![Some("wallet".to_owned()); 4]);
        drop(seen);
        for unanswered in [false, true] {
            let (runtime, chain, step) = owner_run(
                Chain {
                    owner_refusal: (!unanswered).then_some(native::CONTRACT_EXPIRED),
                    owner_check_unanswered: unanswered,
                    ..Chain::default()
                },
                100,
            );
            let retained = std::cell::Cell::new(false);
            let error = runtime
                .submit_owner(
                    &step,
                    Some(&wallet),
                    |_| {
                        retained.set(true);
                        Ok(())
                    },
                    OWNER_POLL,
                    10,
                )
                .unwrap_err();
            if unanswered {
                assert_eq!((error.code, error.api_code), (UNCERTAIN, None));
            } else {
                assert_eq!(error.api_code.as_deref(), Some(native::CONTRACT_EXPIRED));
            }
            let seen = chain.lock().unwrap();
            assert_eq!(
                (retained.get(), seen.owner_sends, seen.owner_lookups),
                (false, 0, 0)
            );
        }
    }
    fn journaled(journal: &MemoryOwnerSteps) -> (Status, bool) {
        let record = journal.load(&[9; 32]).unwrap().unwrap();
        (record.status, record.closed)
    }
    /// The journal form: a step is recorded before it leaves and looked up before it is sent
    /// again. After an uncertain send it is sent again while the lookup stays inconclusive. A
    /// step that the network accepted waits for its receipt and is sent again only when the
    /// rail shows that nothing of it is pending. Its receipt ends it.
    #[test]
    fn the_journal_form_looks_up_a_sent_step_before_it_sends_it_again() {
        let (runtime, chain, step) = owner_run(
            Chain {
                owner_answers: vec![Status::SubmissionUnknown],
                ..Chain::default()
            },
            100,
        );
        let mut journal = MemoryOwnerSteps::default();
        let pass = |journal: &mut MemoryOwnerSteps, now| {
            let status = runtime
                .submit_owner_step(journal, &step, None, now)
                .unwrap();
            let seen = chain.lock().unwrap();
            (status, seen.owner_sends, seen.owner_lookups)
        };
        assert_eq!(pass(&mut journal, 10), (Status::SubmissionUnknown, 1, 0));
        assert_eq!(journaled(&journal), (Status::SubmissionUnknown, false));
        // The lookup stays inconclusive, and the rail still accepts it: sent again.
        assert_eq!(pass(&mut journal, 11), (Status::Submitted, 2, 1));
        // The network accepted it: an inconclusive lookup sends nothing.
        assert_eq!(pass(&mut journal, 12), (Status::Submitted, 2, 2));
        // The network holds it pending: not sent again.
        chain.lock().unwrap().owner_held = true;
        assert_eq!(pass(&mut journal, 13), (Status::Submitted, 2, 3));
        // The rail shows that nothing of it is pending: sent again.
        chain.lock().unwrap().owner_held = false;
        chain.lock().unwrap().owner_dropped = true;
        assert_eq!(pass(&mut journal, 14), (Status::Submitted, 3, 4));
        assert_eq!(chain.lock().unwrap().owner_sent_at, [11, 12, 15]);
        // Its receipt ends it, and the journal keeps the final status with no other lookup.
        chain.lock().unwrap().owner_receipt_after = Some(0);
        assert_eq!(pass(&mut journal, 15), (Status::Confirmed, 3, 5));
        assert_eq!(pass(&mut journal, 16), (Status::Confirmed, 3, 5));
        // Other bytes under the same identity are refused.
        let other = OwnerSigned(
            native::Packet::new(
                NETWORK,
                &serde_json::json!({"expires": 100, "other": 1}),
                &PACKETS,
            )
            .unwrap(),
        );
        assert!(
            runtime
                .submit_owner_step(&mut journal, &other, None, 17)
                .is_err()
        );
    }
    /// After an uncertain send, a step that the network holds is not sent again: the journal
    /// records it as accepted, and it waits for its receipt.
    #[test]
    fn a_possibly_sent_step_that_the_network_holds_is_not_sent_again() {
        let (runtime, chain, step) = owner_run(
            Chain {
                owner_answers: vec![Status::SubmissionUnknown],
                ..Chain::default()
            },
            100,
        );
        let mut journal = MemoryOwnerSteps::default();
        let mut pass = |now| runtime.submit_owner_step(&mut journal, &step, None, now);
        assert_eq!(pass(10).unwrap(), Status::SubmissionUnknown);
        chain.lock().unwrap().owner_held = true;
        assert_eq!(pass(11).unwrap(), Status::Submitted);
        // The lookup then finds nothing: an accepted step still waits.
        chain.lock().unwrap().owner_held = false;
        assert_eq!(pass(12).unwrap(), Status::Submitted);
        assert_eq!(journaled(&journal), (Status::Submitted, false));
        let seen = chain.lock().unwrap();
        assert_eq!((seen.owner_sends, seen.owner_lookups), (1, 2));
    }
    /// The journal binds a step to its installation: every check and every send carries it, and
    /// a call for the same step with another installation, or with none, is refused before
    /// anything is read or sent.
    #[test]
    fn the_journal_form_binds_a_step_to_its_installation() {
        let (runtime, chain, step) = owner_run(
            Chain {
                owner_answers: vec![Status::SubmissionUnknown],
                ..Chain::default()
            },
            100,
        );
        let (wallet, other) = (installed("wallet"), installed("other"));
        let mut journal = MemoryOwnerSteps::default();
        for now in [10, 11] {
            runtime
                .submit_owner_step(&mut journal, &step, Some(&wallet), now)
                .unwrap();
        }
        let record = journal.load(&[9; 32]).unwrap().unwrap();
        assert_eq!(record.wallet.as_deref(), Some("wallet"));
        let seen = chain.lock().unwrap();
        assert_eq!(seen.owner_sends, 2);
        // The check before each send, and the check of each send.
        assert_eq!(seen.owner_checked_on, vec![Some("wallet".to_owned()); 4]);
        let calls = (seen.owner_packets.len(), seen.owner_lookups);
        drop(seen);
        for on in [Some(&other), None] {
            let error = runtime
                .submit_owner_step(&mut journal, &step, on, 12)
                .unwrap_err();
            assert_eq!(
                error.message,
                "Owner step identity already binds another installation"
            );
        }
        let seen = chain.lock().unwrap();
        assert_eq!((seen.owner_packets.len(), seen.owner_lookups), calls);
        drop(seen);
        // The store itself refuses a record of the step under another installation.
        let mut moved = record.clone();
        moved.wallet = None;
        assert!(journal.record(&[9; 32], &moved).is_err());
        journal.record(&[9; 32], &record).unwrap();
    }
    /// A refusal for this attempt and a check without an answer change nothing in the journal:
    /// a step that was never sent leaves no record, a possibly sent step stays possibly sent
    /// and open, nothing is sent, and a later call sends once the check passes.
    #[test]
    fn a_refusal_for_this_attempt_changes_nothing_in_the_journal() {
        for unanswered in [false, true] {
            let refuse = |chain: &Arc<Mutex<Chain>>, refused: bool| {
                let mut chain = chain.lock().unwrap();
                chain.owner_refusal = (refused && !unanswered).then_some(native::CONTRACT_EXPIRED);
                chain.owner_check_unanswered = refused && unanswered;
            };
            let refused = |error: crate::error::Error| {
                if unanswered {
                    assert_eq!((error.code, error.api_code), (UNCERTAIN, None));
                } else {
                    assert_eq!(error.api_code.as_deref(), Some(native::CONTRACT_EXPIRED));
                }
            };
            let (runtime, chain, step) = owner_run(
                Chain {
                    owner_answers: vec![Status::SubmissionUnknown],
                    ..Chain::default()
                },
                100,
            );
            let wallet = installed("wallet");
            let mut journal = MemoryOwnerSteps::default();
            let mut pass = |now| runtime.submit_owner_step(&mut journal, &step, Some(&wallet), now);
            // Never sent: the refusal, and no record.
            refuse(&chain, true);
            refused(pass(10).unwrap_err());
            refuse(&chain, false);
            assert_eq!(pass(11).unwrap(), Status::SubmissionUnknown);
            // Possibly sent: the refusal, after its lookup, and the record stays.
            refuse(&chain, true);
            refused(pass(12).unwrap_err());
            let seen = chain.lock().unwrap();
            assert_eq!((seen.owner_sends, seen.owner_lookups), (1, 1));
            drop(seen);
            refuse(&chain, false);
            assert_eq!(pass(13).unwrap(), Status::Submitted);
            assert_eq!(chain.lock().unwrap().owner_sends, 2);
            assert_eq!(journaled(&journal), (Status::Submitted, false));
        }
        // A never-sent step that is refused leaves no record.
        let (runtime, chain, step) = owner_run(
            Chain {
                owner_refusal: Some(native::CONTRACT_EXPIRED),
                ..Chain::default()
            },
            100,
        );
        let mut journal = MemoryOwnerSteps::default();
        assert!(
            runtime
                .submit_owner_step(&mut journal, &step, None, 10)
                .is_err()
        );
        assert!(journal.load(&[9; 32]).unwrap().is_none());
        assert_eq!(chain.lock().unwrap().owner_sends, 0);
    }
    /// The cutoff is final. A possibly sent step that reaches it is closed before any lookup,
    /// also when that lookup fails, and is never sent again, also at an earlier clock later and
    /// when the rail then shows nothing pending; its receipt is still looked up. A step that
    /// was never sent returns the rail's refusal and leaves no record.
    #[test]
    fn a_step_that_reached_its_cutoff_is_never_sent_again() {
        for accepted in [false, true] {
            let (status, answers) = if accepted {
                (Status::Submitted, vec![])
            } else {
                (Status::SubmissionUnknown, vec![Status::SubmissionUnknown])
            };
            let (runtime, chain, step) = owner_run(
                Chain {
                    owner_answers: answers,
                    ..Chain::default()
                },
                100,
            );
            let mut journal = MemoryOwnerSteps::default();
            let mut pass = |now| runtime.submit_owner_step(&mut journal, &step, None, now);
            assert_eq!(pass(10).unwrap(), status.clone());
            // The second before the bound is the cutoff by the caller's clock.
            chain.lock().unwrap().owner_lookup_fails =
                Some(crate::error::Error::uncertain("no answer"));
            assert!(pass(99).is_err());
            chain.lock().unwrap().owner_lookup_fails = None;
            chain.lock().unwrap().owner_dropped = true;
            let (lookups, held) = {
                let seen = chain.lock().unwrap();
                (seen.owner_lookups, seen.owner_status_lookups)
            };
            assert_eq!(pass(12).unwrap(), status.clone());
            let seen = chain.lock().unwrap();
            assert_eq!((seen.owner_sends, seen.owner_lookups), (1, lookups + 1));
            // A closed step is looked up for its receipt only.
            assert_eq!(seen.owner_status_lookups, held);
            drop(seen);
            assert_eq!(journaled(&journal), (status, true));
            // Its receipt ends a closed step.
            chain.lock().unwrap().owner_receipt_after = Some(0);
            assert_eq!(
                runtime
                    .submit_owner_step(&mut journal, &step, None, 13)
                    .unwrap(),
                Status::Confirmed
            );
        }
        let (runtime, chain, step) = owner_run(Chain::default(), 100);
        let mut journal = MemoryOwnerSteps::default();
        let error = runtime
            .submit_owner_step(&mut journal, &step, None, 99)
            .unwrap_err();
        assert_eq!(error.api_code.as_deref(), Some(OWNER_TRANSACTION_EXPIRED));
        assert!(journal.load(&[9; 32]).unwrap().is_none());
        assert_eq!(chain.lock().unwrap().owner_sends, 0);
    }
    /// The answer of a send speaks for that send only. A first send that the network did not
    /// accept leaves nothing pending, and the next call sends without a lookup. A step that was
    /// possibly sent before stays so when a later send is not accepted or is rejected, and is
    /// looked up again. A lookup that fails sends nothing and changes nothing.
    #[test]
    fn the_answer_of_a_send_speaks_for_that_send_only() {
        let (runtime, chain, step) = owner_run(
            Chain {
                owner_answers: vec![
                    Status::Prepared,
                    Status::SubmissionUnknown,
                    Status::Prepared,
                ],
                ..Chain::default()
            },
            100,
        );
        let mut journal = MemoryOwnerSteps::default();
        let mut pass = |now| runtime.submit_owner_step(&mut journal, &step, None, now);
        assert_eq!(pass(10).unwrap(), Status::Prepared);
        assert_eq!(pass(11).unwrap(), Status::SubmissionUnknown);
        assert_eq!(chain.lock().unwrap().owner_lookups, 0);
        assert_eq!(pass(12).unwrap(), Status::SubmissionUnknown);
        let seen = chain.lock().unwrap();
        assert_eq!((seen.owner_sends, seen.owner_lookups), (3, 1));
        drop(seen);
        // A rejected send after an uncertain one: the rail's error, and still possibly sent.
        chain.lock().unwrap().owner_rejected = true;
        assert_eq!(
            pass(13).unwrap_err().message,
            "The network rejected the send"
        );
        assert_eq!(chain.lock().unwrap().owner_sends, 4);
        // A lookup of the receipt or of the network's hold that fails sends nothing.
        chain.lock().unwrap().owner_status_fails = true;
        assert!(pass(14).is_err());
        chain.lock().unwrap().owner_status_fails = false;
        chain.lock().unwrap().owner_lookup_fails =
            Some(crate::error::Error::uncertain("no answer"));
        assert!(pass(15).is_err());
        assert_eq!(chain.lock().unwrap().owner_sends, 4);
        assert_eq!(journaled(&journal), (Status::SubmissionUnknown, false));
        // The receipt of a failed transaction ends the step as failed.
        let mut seen = chain.lock().unwrap();
        seen.owner_lookup_fails = None;
        seen.owner_receipt_failed = true;
        seen.owner_receipt_after = Some(0);
        drop(seen);
        assert_eq!(
            runtime
                .submit_owner_step(&mut journal, &step, None, 16)
                .unwrap(),
            Status::Failed
        );
        assert_eq!(journaled(&journal), (Status::Failed, false));
    }
    /// A first send that the network rejects leaves the step as not sent: at its cutoff it
    /// returns the rail's refusal and is not closed as a possibly sent step.
    #[test]
    fn a_rejected_first_send_leaves_the_step_not_sent() {
        let (runtime, chain, step) = owner_run(
            Chain {
                owner_rejected: true,
                ..Chain::default()
            },
            100,
        );
        let mut journal = MemoryOwnerSteps::default();
        let error = runtime
            .submit_owner_step(&mut journal, &step, None, 10)
            .unwrap_err();
        assert_eq!(error.message, "The network rejected the send");
        assert_eq!(journaled(&journal), (Status::Prepared, false));
        let error = runtime
            .submit_owner_step(&mut journal, &step, None, 99)
            .unwrap_err();
        assert_eq!(error.api_code.as_deref(), Some(OWNER_TRANSACTION_EXPIRED));
        assert_eq!(journaled(&journal), (Status::Prepared, false));
        let seen = chain.lock().unwrap();
        assert_eq!((seen.owner_sends, seen.owner_lookups), (1, 0));
    }
    /// The journal form reads its clock after each lookup, since a lookup takes time: a
    /// possibly sent step whose cutoff passes during a lookup of its receipt, also one that
    /// fails, or during a lookup of what the network holds, is closed and not sent. At its
    /// cutoff no answer of that lookup clears it, and no later call with an earlier clock sends
    /// it.
    #[test]
    fn a_step_whose_cutoff_passes_during_a_lookup_is_closed() {
        let slow = std::time::Duration::from_millis(1_050);
        // The receipt lookup is slow and answers nothing; it is slow and fails; the lookup of
        // the network's hold is slow and shows nothing pending.
        for case in 0..3 {
            let (runtime, chain, step) = owner_run(
                Chain {
                    owner_answers: vec![Status::SubmissionUnknown],
                    ..Chain::default()
                },
                12,
            );
            let mut journal = MemoryOwnerSteps::default();
            let mut pass = |now| runtime.submit_owner_step(&mut journal, &step, None, now);
            assert_eq!(pass(9).unwrap(), Status::SubmissionUnknown);
            {
                let mut chain = chain.lock().unwrap();
                match case {
                    0 => chain.owner_lookup_time = slow,
                    1 => {
                        chain.owner_lookup_time = slow;
                        chain.owner_lookup_fails =
                            Some(crate::error::Error::uncertain("no answer"));
                    }
                    _ => {
                        chain.owner_status_time = slow;
                        chain.owner_dropped = true;
                    }
                }
            }
            // The call begins at clock 11, before the cutoff 12, and reaches it in the lookup.
            let answer = pass(10);
            if case == 1 {
                assert!(answer.is_err());
            } else {
                assert_eq!(answer.unwrap(), Status::SubmissionUnknown, "{case}");
            }
            // A later call with an earlier clock, quick lookups and nothing pending: not sent.
            {
                let mut chain = chain.lock().unwrap();
                chain.owner_lookup_time = std::time::Duration::ZERO;
                chain.owner_status_time = std::time::Duration::ZERO;
                chain.owner_lookup_fails = None;
                chain.owner_dropped = true;
            }
            assert_eq!(pass(5).unwrap(), Status::SubmissionUnknown, "{case}");
            assert_eq!(
                journaled(&journal),
                (Status::SubmissionUnknown, true),
                "{case}"
            );
            let seen = chain.lock().unwrap();
            // The one send of the first call, and its two checks: no check came after.
            assert_eq!(seen.owner_sent_at, [10], "{case}");
            assert_eq!(seen.owner_checked_at, [10, 10], "{case}");
        }
    }
    /// A send takes time too: a step whose cutoff passes during its send is closed with the
    /// send's own answer, and no later call sends it again.
    #[test]
    fn a_step_whose_cutoff_passes_during_its_send_is_closed() {
        let (runtime, chain, step) = owner_run(
            Chain {
                owner_answers: vec![Status::SubmissionUnknown],
                owner_send_time: std::time::Duration::from_millis(1_050),
                ..Chain::default()
            },
            12,
        );
        let mut journal = MemoryOwnerSteps::default();
        let mut pass = |now| runtime.submit_owner_step(&mut journal, &step, None, now);
        assert_eq!(pass(10).unwrap(), Status::SubmissionUnknown);
        chain.lock().unwrap().owner_send_time = std::time::Duration::ZERO;
        assert_eq!(pass(5).unwrap(), Status::SubmissionUnknown);
        assert_eq!(journaled(&journal), (Status::SubmissionUnknown, true));
        assert_eq!(chain.lock().unwrap().owner_sends, 1);
    }
    /// A refusal for this attempt that comes from the send's own check, after the step was
    /// recorded as possibly sent, sends nothing: a step that was never sent before is recorded
    /// as not sent, and the next call sends it with no lookup.
    #[test]
    fn a_first_send_that_its_own_check_refuses_leaves_the_step_not_sent() {
        let (runtime, chain, step) = owner_run(
            Chain {
                owner_refusal: Some(native::CONTRACT_EXPIRED),
                // The check before the record passes; the check of the send refuses.
                owner_checks_before_refusal: 1,
                ..Chain::default()
            },
            100,
        );
        let mut journal = MemoryOwnerSteps::default();
        let error = runtime
            .submit_owner_step(&mut journal, &step, None, 10)
            .unwrap_err();
        assert_eq!(error.api_code.as_deref(), Some(native::CONTRACT_EXPIRED));
        assert_eq!(journaled(&journal), (Status::Prepared, false));
        assert_eq!(chain.lock().unwrap().owner_sends, 0);
        chain.lock().unwrap().owner_refusal = None;
        assert_eq!(
            runtime
                .submit_owner_step(&mut journal, &step, None, 11)
                .unwrap(),
            Status::Submitted
        );
        let seen = chain.lock().unwrap();
        assert_eq!((seen.owner_sends, seen.owner_lookups), (1, 0));
    }
    /// The journal in memory keeps what is final: a closed record is not reopened and the
    /// status of a receipt is not replaced, also by a copy of the record that was read before.
    #[test]
    fn the_memory_journal_never_reopens_a_final_record() {
        let (_, _, step) = owner_run(Chain::default(), 100);
        let record = |status, closed| OwnerStepRecord {
            step: step.clone(),
            wallet: None,
            status,
            closed,
        };
        let id = [9u8; 32];
        let mut journal = MemoryOwnerSteps::default();
        let open = record(Status::SubmissionUnknown, false);
        journal.record(&id, &open).unwrap();
        journal
            .record(&id, &record(Status::SubmissionUnknown, true))
            .unwrap();
        // The earlier copy would reopen the step.
        assert!(journal.record(&id, &open).is_err());
        assert_eq!(journaled(&journal), (Status::SubmissionUnknown, true));
        // A closed step still takes its receipt, and the receipt's status then stays.
        journal.record(&id, &record(Status::Failed, true)).unwrap();
        for other in [Status::Confirmed, Status::Submitted, Status::Prepared] {
            assert!(journal.record(&id, &record(other, true)).is_err());
        }
        journal.record(&id, &record(Status::Failed, true)).unwrap();
        assert_eq!(journaled(&journal), (Status::Failed, true));
    }
    /// A journal write that fails ends the call with that failure: a step is not sent before
    /// its record as possibly sent is kept.
    #[test]
    fn a_step_whose_record_is_not_kept_is_not_sent() {
        struct Full;
        impl OwnerStepStore for Full {
            fn load(&self, _: &[u8]) -> Result<Option<OwnerStepRecord>> {
                Ok(None)
            }
            fn record(&mut self, _: &[u8], _: &OwnerStepRecord) -> Result<()> {
                Err(crate::error::Error::config("The store is full"))
            }
        }
        let (runtime, chain, step) = owner_run(Chain::default(), 100);
        let error = runtime
            .submit_owner_step(&mut Full, &step, None, 10)
            .unwrap_err();
        assert_eq!(error.message, "The store is full");
        assert_eq!(chain.lock().unwrap().owner_sends, 0);
    }
    /// The owner-step wrappers pass each argument to the rail and its answer back.
    #[test]
    fn the_owner_step_wrappers_pass_through_to_the_rail() {
        let (runtime, chain, step) = owner_run(Chain::default(), 100);
        let mut wallet = installation();
        wallet.0.description.wallet = "wallet".into();
        runtime
            .validate_owner_signed(&step, Some(&wallet), 7)
            .unwrap();
        assert_eq!(
            runtime.broadcast_owner(&step, Some(&wallet), 8).unwrap(),
            Status::Submitted
        );
        runtime.validate_owner_signed(&step, None, 9).unwrap();
        assert_eq!(
            runtime.owner_status(&step).unwrap(),
            Status::SubmissionUnknown
        );
        chain.lock().unwrap().owner_held = true;
        assert_eq!(runtime.owner_status(&step).unwrap(), Status::Submitted);
        assert!(!runtime.owner_step_lapsed(&step, 41).unwrap());
        chain.lock().unwrap().owner_held = false;
        assert!(runtime.owner_step_lapsed(&step, 42).unwrap());
        assert!(runtime.wallet_closed(&wallet).unwrap());
        let close = runtime.prepare_close(&wallet, 55, 66).unwrap();
        assert_eq!(close.0.document().get(), r#"{"close":1}"#);
        let info = step.info().unwrap();
        assert_eq!(
            (info.action.as_str(), info.identity, info.expires_at),
            ("fund", vec![9; 32], 100)
        );
        let seen = chain.lock().unwrap();
        // The two checks, the send with its own check, two lookups and the two lapse proofs
        // carried the step's own packet.
        assert_eq!(seen.owner_packets, vec![r#"{"expires":100}"#; 8]);
        assert_eq!(seen.owner_checked_at, [7, 8, 9]);
        // Each wrapper passed the installation that it was given, or none.
        let wallet = Some("wallet".to_owned());
        assert_eq!(seen.owner_checked_on, [wallet.clone(), wallet, None]);
        assert_eq!(seen.owner_sent_at, [8]);
        assert_eq!(seen.owner_lapse_since, [41, 42]);
        assert_eq!(seen.owner_closed_wallets, ["wallet"]);
        assert_eq!(seen.owner_closes, [("wallet".to_owned(), 55, 66)]);
    }
    /// `settle_receipt` and `expense` pass each of the caller's inputs to the rail, in its own
    /// place, and the rail's whole answer back.
    #[test]
    fn the_settlement_and_expense_wrappers_pass_through_to_the_rail() {
        let chain = Arc::new(Mutex::new(Chain::default()));
        let runtime = Runtime {
            provider: Box::new(Rail(chain.clone())),
        };
        let packet = |document: serde_json::Value| {
            native::Packet::new(NETWORK, &document, &PACKETS).unwrap()
        };
        let accounting = Accounting {
            nonce: 4,
            ..Accounting::default()
        };
        let settled = runtime
            .settle_receipt(
                &Prepared(packet(serde_json::json!({"prepared": 1}))),
                &Signed(packet(serde_json::json!({"signed": 1}))),
                &Receipt(packet(serde_json::json!({"status": "confirmed"}))),
                Some(&accounting),
                7,
            )
            .unwrap();
        assert_eq!(
            settled,
            Settlement {
                transaction: vec![8; 32],
                outcome: OperationOutcome::Executed,
                usage: None,
                fee_units: 250,
                counted_gas: None,
                next_nonce: Some(5),
                gas_carry: 8,
            }
        );
        assert_eq!(
            chain.lock().unwrap().settled,
            [(
                r#"{"prepared":1}"#.to_owned(),
                r#"{"signed":1}"#.to_owned(),
                r#"{"status":"confirmed"}"#.to_owned(),
                Some(4),
                7
            )]
        );
        let ceiling = FeeCeiling {
            max_fee_units: 1_000,
            gas_units: 30,
            observed_height: 9,
            day: 1,
        };
        let transfer = Action::Transfer {
            recipient: "seller".into(),
            units: 1,
        };
        let mut wallet = installation();
        wallet.0.description.wallet = "wallet".into();
        let bound = runtime.expense(&wallet, &transfer, &ceiling).unwrap();
        // A transfer has no conversion: it spends its amount plus the priced gas.
        assert_eq!(
            bound,
            ExpenseBound {
                gas_units: 30,
                conversion_units: 0,
                spent_units: 31,
                max_fee_units: 1_000,
                observed_height: 9,
                day: 1,
            }
        );
        assert_eq!(
            chain.lock().unwrap().expensed,
            [("wallet".to_owned(), "seller".to_owned(), 1, ceiling)]
        );
    }
}
