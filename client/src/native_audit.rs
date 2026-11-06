//! Capability-scoped SQL audit reporting; it grants no signing or owner authority.
use crate::{config::parse_origin, error::Error};
use allowit_native::{
    client::Binding,
    crypto::Key,
    error::Result,
    journal::FileJournal,
    lifecycle::{AuthorizedExecution, Record},
    native::{ApprovalCommitment, ApprovalRequest, ExecutionRequestIdentity, Simulation},
    rpc::Rpc,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::Read,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuditConfig {
    pub origin: String,
    pub token: String,
}
impl AuditConfig {
    pub fn validate(&self) -> crate::error::Result<()> {
        if parse_origin(&self.origin)? != self.origin
            || self
                .token
                .strip_prefix("native-report.")
                .is_none_or(|secret| {
                    secret.len() != 32
                        || !secret
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
                })
        {
            return Err(Error::config("Invalid native audit capability"));
        }
        Ok(())
    }
}
pub(crate) struct Reporter {
    config: AuditConfig,
    policy_id: String,
    client: reqwest::blocking::Client,
    pub blocked: AtomicBool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExecuteResponse {
    binding: Binding,
    request: ApprovalRequest,
    approval: ApprovalCommitment,
    intent: String,
    message: String,
    partial_transaction: String,
    operation: PreparedOperation,
    /// Current servers echo the saved execution-policy revision they evaluated.
    /// It is display data: the signed approval and request digests above remain
    /// the authority. It must still name this vault's owner, network and mint.
    #[serde(default)]
    execution_binding: Option<ExecutionBinding>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(dead_code)] // policy_id, revision, source_hash and recipient_mode are shown, not trusted.
struct ExecutionBinding {
    policy_id: String,
    revision: u64,
    source_hash: String,
    owner: String,
    network: String,
    #[serde(default)]
    recipient_mode: Option<String>,
    #[serde(default)]
    recipient_mint: Option<String>,
}
/// An HTTP 200 reply without a prepared approval: the owner must answer first,
/// or the purchase already ended without an executor signature.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingResponse {
    status: String,
    request: PendingRequest,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingRequest {
    #[serde(default)]
    question: Option<String>,
    #[serde(default)]
    question_number: Option<u64>,
    #[serde(default)]
    expires_at: Option<u64>,
}
/// Exit status for a purchase that waits for the owner's answer; it matches the
/// gateway's awaiting_input status.
const AWAITING_OWNER: i32 = 11;
fn utc_time(unix: u64) -> String {
    let (days, secs) = (unix / 86_400, unix % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        secs / 3_600,
        secs % 3_600 / 60
    )
}
fn pending_outcome(pending: PendingResponse, request_id: &str) -> allowit_native::error::Error {
    use allowit_native::error::Error;
    let nothing = "No executor signature was issued and nothing was paid.";
    match pending.status.as_str() {
        "awaiting_input" => {
            let number = pending
                .request
                .question_number
                .map(|n| format!(" {n}"))
                .unwrap_or_default();
            let question = pending
                .request
                .question
                .filter(|q| !q.is_empty() && q.len() <= 1_024 && !q.chars().any(char::is_control))
                .map(|q| format!(": \"{q}\""))
                .unwrap_or_default();
            let deadline = pending
                .request
                .expires_at
                .map(|t| format!(" by {}", utc_time(t)))
                .unwrap_or_default();
            Error {
                message: format!(
                    "Allowit is waiting for the owner's answer to policy question{number}{question}. {nothing} Ask the owner to answer it in Allowit Requests{deadline}, then run the same command again with ALLOWIT_REQUEST_ID={request_id}."
                ),
                code: AWAITING_OWNER,
            }
        }
        "denied" => Error::denied(format!(
            "The owner answered No in Allowit Requests. {nothing} Do not retry this purchase under a new request ID."
        )),
        "expired" => Error::denied(format!(
            "The owner did not answer the Allowit policy question in time. {nothing} Ask the owner before trying again with a new request ID."
        )),
        "failed" => Error::denied(format!(
            "The Allowit policy denied this purchase. {nothing}"
        )),
        "revoked" => Error::denied(format!(
            "The owner stopped this vault in Allowit. {nothing}"
        )),
        "evaluating" | "ready" | "prepared" => Error::config(format!(
            "Allowit is still preparing this approved purchase. {nothing} Run the same command again with ALLOWIT_REQUEST_ID={request_id}."
        )),
        _ => unavailable(),
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreparedOperation {
    id: String,
    policy_id: String,
    method: String,
    status: String,
    blockhash: Key,
    last_valid_block_height: u64,
    nonce: String,
    revision: String,
    commitment: String,
    expires_at: u64,
    instance_slot: u64,
    decision_code: String,
    simulation: Simulation,
    #[serde(default, deserialize_with = "present_bool")]
    blockhash_expired: Option<bool>,
    #[serde(default, deserialize_with = "present_bool")]
    retry_preparation_allowed: Option<bool>,
}
fn present_bool<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<bool>, D::Error> {
    bool::deserialize(deserializer).map(Some)
}
fn unavailable() -> allowit_native::error::Error {
    allowit_native::error::Error::uncertain(
        "Native audit acknowledgment unavailable; retain the original journal and request ID",
    )
}
impl Reporter {
    pub fn new(config: AuditConfig, policy_id: String) -> Result<Self> {
        config.validate().map_err(|_| unavailable())?;
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .min_tls_version(reqwest::tls::Version::TLS_1_2)
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| unavailable())?;
        Ok(Self {
            config,
            policy_id,
            client,
            blocked: AtomicBool::new(false),
        })
    }
    pub fn report(&self, record: &Record) -> Result<Value> {
        if record.method != "execute" {
            return Err(unavailable());
        }
        let body = serde_json::to_vec(&json!({"policyId":self.policy_id,"record":record}))
            .map_err(|_| unavailable())?;
        if body.len() > 65_536 {
            return Err(unavailable());
        }
        let mut response = self
            .client
            .post(format!("{}/api/native/report", self.config.origin))
            .bearer_auth(&self.config.token)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .map_err(|_| unavailable())?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(unavailable());
        }
        let mut raw = Vec::new();
        Read::by_ref(&mut response)
            .take(2_097_153)
            .read_to_end(&mut raw)
            .map_err(|_| unavailable())?;
        if raw.len() > 2_097_152 {
            return Err(unavailable());
        }
        let result: Value = serde_json::from_slice(&raw).map_err(|_| unavailable())?;
        let operation = &result["operation"];
        if operation["id"] != record.id
            || operation["signature"] != record.signature
            || operation["policyId"] != self.policy_id
            || operation["method"] != "execute"
            || (record.final_status() && operation["status"] != record.status)
            || !matches!(
                operation["status"].as_str(),
                Some("uncertain" | "submitted" | "settled" | "failed")
            )
        {
            return Err(unavailable());
        }
        Ok(operation.clone())
    }
    /// `requested_recipient` is what the agent named: the token account itself, or
    /// for wallet payments the wallet whose canonical token account is
    /// `identity.recipient`.
    pub fn authorize(
        &self,
        identity: &ExecutionRequestIdentity,
        context: &Value,
        requested_recipient: Key,
    ) -> Result<AuthorizedExecution> {
        if identity.vault_policy_id != self.policy_id {
            return Err(allowit_native::error::Error::config(
                "Execution request belongs to a different policy",
            ));
        }
        let body = serde_json::to_vec(&json!({
            "policyId": &self.policy_id,
            "requestId": &identity.operation_id,
            "action": &identity.action,
            "merchant": &identity.merchant,
            "context": context,
            "options": {"amount": &identity.amount, "recipient": requested_recipient},
        }))
        .map_err(|_| unavailable())?;
        if body.len() > 65_536 {
            return Err(allowit_native::error::Error::config(
                "Native execution request is too large",
            ));
        }
        let mut response = self
            .client
            .post(format!("{}/api/native/execute", self.config.origin))
            .bearer_auth(&self.config.token)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .map_err(|_| unavailable())?;
        let status = response.status();
        let mut raw = Vec::new();
        Read::by_ref(&mut response)
            .take(2_097_153)
            .read_to_end(&mut raw)
            .map_err(|_| unavailable())?;
        if raw.len() > 2_097_152 {
            return Err(unavailable());
        }
        if status != reqwest::StatusCode::OK {
            let message = serde_json::from_slice::<Value>(&raw)
                .ok()
                .and_then(|value| value["error"].as_str().map(str::to_owned))
                .filter(|message| {
                    !message.is_empty()
                        && message.len() <= 1_000
                        && !message.chars().any(char::is_control)
                })
                .unwrap_or_else(|| "Native execution authorization failed".into());
            return Err(match status.as_u16() {
                400 | 401 | 404 | 409 => allowit_native::error::Error::config(message),
                403 => allowit_native::error::Error::denied(message),
                _ => allowit_native::error::Error::uncertain(message),
            });
        }
        let value: Value = serde_json::from_slice(&raw).map_err(|_| unavailable())?;
        if value.get("approval").is_none()
            && value.get("operation").is_none()
            && value.get("request").is_some()
        {
            let pending: PendingResponse =
                serde_json::from_value(value).map_err(|_| unavailable())?;
            return Err(pending_outcome(pending, &identity.operation_id));
        }
        let response: ExecuteResponse = serde_json::from_value(value).map_err(|_| unavailable())?;
        if response.execution_binding.as_ref().is_some_and(|shown| {
            shown.owner != response.binding.owner.to_string()
                || shown.network != response.approval.network
                || shown
                    .recipient_mint
                    .as_ref()
                    .is_some_and(|mint| *mint != response.binding.mint.to_string())
        }) {
            return Err(unavailable());
        }
        let wallet = (requested_recipient != identity.recipient).then_some(requested_recipient);
        if response.approval.recipient_wallet != wallet {
            return Err(unavailable());
        }
        let operation = response.operation;
        let commitment = response.approval.digest()?;
        if operation.id != identity.operation_id
            || operation.policy_id != self.policy_id
            || operation.method != "execute"
            || operation.status != "prepared"
            || operation.commitment != commitment
            || operation.expires_at != response.approval.expires_at
            || operation.instance_slot.to_string() != response.approval.instance_slot
            || operation.nonce != response.approval.nonce
            || operation.revision != response.approval.policy_revision
            || operation.decision_code != response.request.decision_code
            || response.request.operation_id != identity.operation_id
            || response.request.input_digest != identity.digest()?
            || response.request.vault_policy_id != self.policy_id
            || response.request.recipient != identity.recipient
            || response.request.amount_units != allowit_native::policy::units(&identity.amount)?
            || response.request.action != identity.action
            || response.request.merchant != identity.merchant
            || response.request.context_hash != identity.context_hash
            || response.approval.operation_id != identity.operation_id
            || response.approval.policy_id != self.policy_id
            || response.approval.request_digest != response.request.digest()?
            || response.approval.recipient != identity.recipient
            || response.approval.amount_units != response.request.amount_units.to_string()
            || response.approval.custody_program != response.binding.custody
            || response.approval.vault != response.binding.vault
            || response.approval.mint != response.binding.mint
            || response.approval.executor != response.binding.executor
            || response.approval.authority != response.binding.authority
        {
            return Err(unavailable());
        }
        let expired = match (
            operation.blockhash_expired,
            operation.retry_preparation_allowed,
        ) {
            (None, None) => false,
            (Some(true), Some(true)) => true,
            _ => return Err(unavailable()),
        };
        let message = base64::engine::general_purpose::STANDARD
            .decode(response.message)
            .map_err(|_| unavailable())?;
        let partial_transaction = base64::engine::general_purpose::STANDARD
            .decode(response.partial_transaction)
            .map_err(|_| unavailable())?;
        if message.is_empty()
            || message.len() > 1_232
            || partial_transaction.len() > 1_232
            || operation.simulation.transaction_bytes != partial_transaction.len()
        {
            return Err(unavailable());
        }
        if expired {
            return Err(allowit_native::error::Error::config(format!(
                "Unsigned native approval for request {} expired; no executor proof was signed. Retain this request ID and its original approval. The owner must authorize an explicit replacement before using a fresh request ID; retrying this ID returns the same expired approval.",
                identity.operation_id
            )));
        }
        Ok(AuthorizedExecution {
            binding: response.binding,
            request: response.request,
            approval: response.approval,
            intent: response.intent,
            message,
            partial_transaction,
            blockhash: operation.blockhash,
            last_valid_block_height: operation.last_valid_block_height,
            nonce: operation.nonce,
            revision: operation.revision,
            simulation: operation.simulation,
        })
    }
    pub fn finish(&self, journal: &FileJournal, id: &str) -> (bool, bool) {
        let withheld = self.blocked.load(Ordering::Relaxed);
        let acknowledged = journal
            .read::<Record>(&format!("request-{id}"))
            .ok()
            .flatten()
            .is_some_and(|record| self.report(&record).is_ok());
        (withheld, withheld || !acknowledged)
    }
}
pub(crate) struct AuditedRpc {
    pub rpc: Box<dyn Rpc>,
    pub reporter: std::sync::Arc<Reporter>,
    pub journal: FileJournal,
}
impl Rpc for AuditedRpc {
    fn call(&self, method: &str, params: Value) -> Result<Value> {
        if method == "sendTransaction" {
            // The SDK has already validated and durably saved this exact proof.
            let records = self.journal.entries::<Record>()?;
            let record = records
                .iter()
                .find(|r| r.method == "execute" && params[0] == r.signed_bytes)
                .ok_or_else(unavailable)?;
            let acknowledgment = self.reporter.report(record);
            if match &acknowledgment {
                Err(_) => true,
                Ok(op) => matches!(op["status"].as_str(), Some("settled" | "failed")),
            } {
                self.reporter.blocked.store(true, Ordering::Relaxed);
                return Err(unavailable());
            }
            self.reporter.blocked.store(false, Ordering::Relaxed);
        }
        self.rpc.call(method, params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allowit_native::{
        client::{Config, Deployment, LOADER, NativeClient},
        crypto::LocalSigner,
        native::State,
        policy::{Policy, digest, units},
        release,
        transaction::Transaction,
    };
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, atomic::AtomicUsize},
    };
    fn record() -> Record {
        Record {
            id: "audit-proof-0001".into(),
            intent: "test-only".into(),
            method: "execute".into(),
            status: "uncertain".into(),
            signature: "fixture-signature".into(),
            signatures: vec!["fixture-signature".into()],
            signed_bytes: "cHJvb2Y=".into(),
            blockhash: Key([9; 32]),
            last_valid_block_height: 100,
            nonce: Some("0".into()),
            revision: Some("1".into()),
            expires_at: None,
            commitment: None,
            instance_slot: None,
            transaction_url: "fixture".into(),
            extra: Default::default(),
        }
    }
    struct Chain {
        acknowledged: Arc<AtomicBool>,
        sends: Arc<AtomicUsize>,
    }
    impl Rpc for Chain {
        fn call(&self, method: &str, params: Value) -> Result<Value> {
            assert_eq!(method, "sendTransaction");
            assert_eq!(params[0], record().signed_bytes);
            assert!(self.acknowledged.load(Ordering::SeqCst));
            self.sends.fetch_add(1, Ordering::SeqCst);
            Ok(json!(record().signature))
        }
    }
    fn check_report_before_broadcast(status: u16, mut operation: Value, expect_send: bool) {
        let dir = std::env::temp_dir().join(format!(
            "allowit-audit-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let journal = FileJournal::new(&dir);
        journal
            .locked(|| journal.write("request-audit-proof-0001", &record()))
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let acknowledged = Arc::new(AtomicBool::new(false));
        let observed = acknowledged.clone();
        let path = dir.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut header = Vec::new();
            let mut byte = [0];
            while !header.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                header.push(byte[0]);
            }
            let header = String::from_utf8(header).unwrap();
            assert!(header.starts_with("POST /api/native/report HTTP/1.1"));
            assert!(
                header
                    .to_ascii_lowercase()
                    .contains("authorization: bearer native-report.")
            );
            let length = header
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .and_then(|n| n.parse::<usize>().ok())
                })
                .unwrap();
            let mut body = vec![0; length];
            stream.read_exact(&mut body).unwrap();
            let sent: Value = serde_json::from_slice(&body).unwrap();
            let durable = FileJournal::new(path)
                .read::<Record>("request-audit-proof-0001")
                .unwrap()
                .unwrap();
            assert_eq!(sent["policyId"], "fixture-policy");
            assert_eq!(sent["record"], serde_json::to_value(durable).unwrap());
            observed.store(true, Ordering::SeqCst);
            let response = json!({"operation":operation.take()}).to_string();
            write!(stream,"HTTP/1.1 {status} Result\r\nConnection: close\r\nLocation: https://example.invalid/steal\r\nContent-Length: {}\r\n\r\n{response}",response.len()).unwrap();
        });
        let reporter = Arc::new(
            Reporter::new(
                AuditConfig {
                    origin,
                    token: format!("native-report.{}", "a".repeat(32)),
                },
                "fixture-policy".into(),
            )
            .unwrap(),
        );
        let sends = Arc::new(AtomicUsize::new(0));
        let rpc = AuditedRpc {
            rpc: Box::new(Chain {
                acknowledged,
                sends: sends.clone(),
            }),
            reporter: reporter.clone(),
            journal,
        };
        let result = rpc.call("sendTransaction", json!([record().signed_bytes, {}]));
        assert_eq!(result.is_ok(), expect_send);
        assert_eq!(sends.load(Ordering::SeqCst), usize::from(expect_send));
        assert_eq!(reporter.blocked.load(Ordering::Relaxed), !expect_send);
        if let Err(error) = result {
            assert!(!error.message.contains("native-report."));
        }
        server.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
    fn acknowledgment() -> Value {
        json!({"id":record().id,"signature":record().signature,"policyId":"fixture-policy","method":"execute","status":"uncertain"})
    }
    struct Offline;
    impl Rpc for Offline {
        fn call(&self, _: &str, _: Value) -> Result<Value> {
            Err(allowit_native::error::Error::config("unexpected test RPC"))
        }
    }
    fn seeded(seed: u8, public: &str) -> LocalSigner {
        let public = Key::parse(public).unwrap();
        let mut pair = [seed; 64];
        pair[32..].copy_from_slice(&public.0);
        LocalSigner::from_secret(&pair).unwrap()
    }
    #[test]
    fn scoped_execute_request_accepts_only_the_canonical_authority_partial() {
        check_authorization_response("live");
    }
    #[test]
    fn expired_unsigned_response_is_actionable_but_never_authorizes_signing() {
        check_authorization_response("expired");
    }
    #[test]
    fn displayed_execution_binding_is_accepted_only_for_this_vault() {
        check_authorization_response("live-with-binding");
        for case in [
            "binding-other-owner",
            "binding-other-mint",
            "binding-unknown-field",
        ] {
            check_authorization_response(case);
        }
    }
    #[test]
    fn wallet_recipient_is_sent_and_must_match_the_approved_wallet() {
        check_authorization_response("live-wallet");
        for case in ["wallet-other", "wallet-unannounced"] {
            check_authorization_response(case);
        }
    }
    #[test]
    fn owner_questions_and_answers_explain_that_nothing_was_signed() {
        for case in ["owner-question", "owner-denied", "owner-expired"] {
            check_authorization_response(case);
        }
    }
    #[test]
    fn utc_deadline_formatting() {
        assert_eq!(utc_time(0), "1970-01-01 00:00 UTC");
        assert_eq!(utc_time(951_782_400), "2000-02-29 00:00 UTC");
        assert_eq!(utc_time(1_793_983_172), "2026-11-06 16:39 UTC");
    }
    #[test]
    fn expired_response_rejects_invalid_flags_and_substituted_bindings() {
        for case in [
            "missing-retry-flag",
            "false-flags",
            "null-flag",
            "string-flag",
            "unknown-field",
            "changed-digest",
            "changed-binding",
        ] {
            check_authorization_response(case);
        }
    }
    fn check_authorization_response(case: &str) {
        let owner = seeded(1, "AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9");
        let executor = seeded(2, "9hSR6S7WPtxmTojgo6GG3k4yDPecgJY292j7xrsUGWBu");
        let authority = seeded(3, "GyGKxMyg1p9SsHfm15MkNUu1u9TN2JtTspcdmrtGUdse");
        let policy =
            Policy::generate("solana:testnet", "Spend up to 5 test tokens per day").unwrap();
        let policy_program = Key([7; 32]);
        let deployment = Deployment {
            network: policy.network.clone(),
            source_bundle: release().source_bundle.clone(),
            policy: policy_program,
            policy_data: Key::find_program_address(
                &[&policy_program.0],
                Key::parse(LOADER).unwrap(),
            )
            .unwrap()
            .0,
            custody: Key([9; 32]),
        };
        let client = NativeClient::new(
            Config {
                network: policy.network.clone(),
                mint: Some(Key([4; 32])),
                executor: Some(executor.public_key()),
                authority: Some(authority.public_key()),
                deployment: Some(deployment),
            },
            Arc::new(Offline),
        )
        .unwrap();
        let binding = client.public_binding(&policy, owner.public_key()).unwrap();
        let state = State {
            binding: binding.clone(),
            abi: 2,
            source_bundle: policy.source_bundle.clone(),
            policy_artifact: policy.policy_artifact.clone(),
            vault_id: policy.id.clone(),
            daily_limit: "5000000".into(),
            action_limit: "1000000".into(),
            spent: "0".into(),
            spent_day: "0".into(),
            nonce: "4".into(),
            revision: "7".into(),
            instance_slot: "19".into(),
            approved: true,
            balance: "5000000".into(),
        };
        let context = json!({"invoice":"fixed-1"});
        // Wallet payments: the agent names a wallet; the request and approval pay its
        // canonical token account, and the approval also commits to the wallet.
        let wallet = Key([12; 32]);
        let wallet_case = matches!(case, "live-wallet" | "wallet-other" | "wallet-unannounced");
        let paid = if wallet_case {
            allowit_native::client::associated_token_address(Key([4; 32]), wallet, false).unwrap()
        } else {
            owner.public_key()
        };
        let identity = ExecutionRequestIdentity::new(
            &policy,
            "server-authorize-001",
            "1.0",
            paid,
            "fetch fixed fixture",
            "fixture merchant",
            &context,
        )
        .unwrap();
        let request = ApprovalRequest {
            version: 1,
            operation_id: identity.operation_id.clone(),
            input_digest: identity.digest().unwrap(),
            vault_policy_id: policy.id.clone(),
            execution_policy_id: "fixture-execution-policy".into(),
            execution_policy_digest: policy.execution_policy_digest.clone(),
            execution_requirements_digest: policy.execution_requirements_digest.clone(),
            execution_policy_revision: 2,
            recipient: identity.recipient,
            amount_units: units(&identity.amount).unwrap(),
            action: identity.action.clone(),
            merchant: identity.merchant.clone(),
            context_hash: identity.context_hash.clone(),
            decision_code: "PASS".into(),
            evidence_digest: digest(b"[]"),
        };
        let mut approval = ApprovalCommitment::new(
            &policy,
            &state,
            &identity.operation_id,
            identity.recipient,
            &identity.amount,
            200,
            &request.digest().unwrap(),
            None,
        )
        .unwrap();
        if matches!(case, "live-wallet" | "wallet-other") {
            approval.recipient_wallet = Some(wallet);
        }
        let requested = match case {
            "live-wallet" | "wallet-unannounced" => wallet,
            "wallet-other" => Key([13; 32]),
            _ => identity.recipient,
        };
        let options = approval.options().unwrap();
        let blockhash = Key([10; 32]);
        let transaction = Transaction::new(
            binding.executor,
            blockhash,
            client
                .expected_instructions(&policy, &binding, "execute", &options, Some("4"), Some("7"))
                .unwrap(),
        )
        .unwrap();
        let partial = transaction
            .partially_signed(&[(authority.public_key(), authority.sign(&transaction.message))])
            .unwrap();
        let mut response = json!({
            "binding": &binding,
            "request": &request,
            "approval": &approval,
            "intent": allowit_native::lifecycle::intent_for(
                &client,
                &policy,
                owner.public_key(),
                "execute",
                &options,
            )
            .unwrap(),
            "message": base64::engine::general_purpose::STANDARD.encode(&transaction.message),
            "partialTransaction": base64::engine::general_purpose::STANDARD.encode(&partial),
            "operation": {
                "id": &identity.operation_id,
                "policyId": &policy.id,
                "method": "execute",
                "status": "prepared",
                "blockhash": blockhash,
                "lastValidBlockHeight": 100,
                "nonce": "4",
                "revision": "7",
                "commitment": approval.digest().unwrap(),
                "expiresAt": 200,
                "instanceSlot": 19,
                "decisionCode": "PASS",
                "simulation": {"contextSlot":99,"unitsConsumed":10000,"transactionBytes":partial.len()},
            },
        });
        let fresh = case.starts_with("live")
            || case.starts_with("binding-")
            || case.starts_with("owner-")
            || case.starts_with("wallet-");
        if !fresh {
            response["operation"]["blockhashExpired"] = json!(true);
            response["operation"]["retryPreparationAllowed"] = json!(true);
        }
        match case {
            "missing-retry-flag" => {
                response["operation"]
                    .as_object_mut()
                    .unwrap()
                    .remove("retryPreparationAllowed");
            }
            "false-flags" => {
                response["operation"]["blockhashExpired"] = json!(false);
                response["operation"]["retryPreparationAllowed"] = json!(false);
            }
            "null-flag" => response["operation"]["blockhashExpired"] = Value::Null,
            "string-flag" => response["operation"]["blockhashExpired"] = json!("true"),
            "unknown-field" => {
                response["operation"]["additionalOwnerOperationAllowed"] = json!(true)
            }
            "changed-digest" => response["request"]["inputDigest"] = json!(digest(b"changed")),
            "changed-binding" => response["binding"]["executor"] = json!(owner.public_key()),
            _ => (),
        }
        let shown = json!({
            "policyId": "fixture-execution-policy",
            "revision": 2,
            "sourceHash": "ab".repeat(32),
            "owner": owner.public_key().to_string(),
            "network": &policy.network,
            "recipientMode": "any-wallet",
            "recipientMint": Key([4; 32]).to_string(),
        });
        match case {
            "live-with-binding" => response["executionBinding"] = shown.clone(),
            "binding-other-owner" => {
                response["executionBinding"] = shown.clone();
                response["executionBinding"]["owner"] = json!(executor.public_key().to_string());
            }
            "binding-other-mint" => {
                response["executionBinding"] = shown.clone();
                response["executionBinding"]["recipientMint"] = json!(Key([5; 32]).to_string());
            }
            "binding-unknown-field" => {
                response["executionBinding"] = shown.clone();
                response["executionBinding"]["signer"] = json!("unexpected");
            }
            "owner-question" => {
                response = json!({
                    "status": "awaiting_input",
                    "request": {
                        "status": "awaiting_input",
                        "question": "Is this purchase about Solana validators?",
                        "questionNumber": 2,
                        "expiresAt": 1_793_983_172u64,
                        "requestId": "server-authorize-001",
                    },
                    "executionBinding": shown.clone(),
                })
            }
            "owner-denied" => {
                response = json!({"status":"denied","request":{"status":"denied"},"executionBinding":shown.clone()})
            }
            "owner-expired" => {
                response = json!({"status":"expired","request":{"status":"expired"}})
            }
            _ => (),
        }
        let sent = json!(requested);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut header = Vec::new();
            let mut byte = [0];
            while !header.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                header.push(byte[0]);
            }
            let header = String::from_utf8(header).unwrap();
            assert!(header.starts_with("POST /api/native/execute HTTP/1.1"));
            assert!(
                header
                    .to_ascii_lowercase()
                    .contains("authorization: bearer native-report.")
            );
            let length = header
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .and_then(|n| n.parse::<usize>().ok())
                })
                .unwrap();
            let mut body = vec![0; length];
            stream.read_exact(&mut body).unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["requestId"], "server-authorize-001");
            assert_eq!(body["action"], "fetch fixed fixture");
            assert_eq!(body["merchant"], "fixture merchant");
            assert_eq!(body["context"]["invoice"], "fixed-1");
            assert_eq!(body["options"]["amount"], "1");
            assert_eq!(body["options"]["recipient"], sent);
            let response = response.to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{response}",
                response.len()
            )
            .unwrap();
        });
        let reporter = Reporter::new(
            AuditConfig {
                origin,
                token: format!("native-report.{}", "a".repeat(32)),
            },
            policy.id,
        )
        .unwrap();
        let result = reporter.authorize(&identity, &context, requested);
        if case.starts_with("live") {
            let authorized = result.unwrap();
            assert_eq!(authorized.message, transaction.message);
            assert_eq!(authorized.partial_transaction, partial);
        } else {
            let error = result.err().unwrap();
            if case == "owner-question" {
                assert_eq!(error.code, AWAITING_OWNER);
                assert!(error.message.contains("question 2"), "{}", error.message);
                assert!(
                    error
                        .message
                        .contains("Is this purchase about Solana validators?")
                );
                assert!(
                    error.message.contains("2026-11-06 16:39 UTC"),
                    "{}",
                    error.message
                );
                assert!(
                    error
                        .message
                        .contains("ALLOWIT_REQUEST_ID=server-authorize-001")
                );
                assert!(error.message.contains("nothing was paid"));
            } else if case == "owner-denied" || case == "owner-expired" {
                assert_eq!(error.code, 20, "{case}");
                assert!(error.message.contains("nothing was paid"), "{case}");
            } else if case == "expired" {
                assert_eq!(error.code, 3);
                assert!(error.message.contains("server-authorize-001"));
                assert!(error.message.contains("no executor proof was signed"));
                assert!(error.message.contains("same expired approval"));
            } else {
                assert_eq!(error.code, 5, "{case}");
                assert!(
                    error.message.contains("acknowledgment unavailable"),
                    "{case}"
                );
            }
            assert!(!error.message.contains("native-report."));
        }
        server.join().unwrap();
    }
    #[test]
    fn durable_exact_proof_is_acknowledged_before_chain_broadcast() {
        check_report_before_broadcast(200, acknowledgment(), true);
    }
    #[test]
    fn failed_redirected_or_substituted_acknowledgment_blocks_broadcast() {
        check_report_before_broadcast(503, acknowledgment(), false);
        check_report_before_broadcast(302, acknowledgment(), false);
        let mut wrong = acknowledgment();
        wrong["signature"] = json!("different");
        check_report_before_broadcast(200, wrong, false);
        let mut final_state = acknowledgment();
        final_state["status"] = json!("settled");
        check_report_before_broadcast(200, final_state, false);
    }
    #[test]
    fn withheld_broadcast_stays_pending_after_late_durable_acknowledgment() {
        let directory =
            std::env::temp_dir().join(format!("allowit-audit-late-{}", std::process::id()));
        let journal = FileJournal::new(&directory);
        journal
            .locked(|| journal.write("request-audit-proof-0001", &record()))
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut raw = Vec::new();
            let mut byte = [0];
            while !raw.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                raw.push(byte[0]);
            }
            let header = String::from_utf8(raw).unwrap();
            let length = header
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .and_then(|n| n.parse::<usize>().ok())
                })
                .unwrap();
            let mut body = vec![0; length];
            stream.read_exact(&mut body).unwrap();
            let submitted: Value = serde_json::from_slice(&body).unwrap();
            assert!(submitted["record"]["replayed"].is_null());
            let body = json!({"operation":acknowledgment()}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        let reporter = Reporter::new(
            AuditConfig {
                origin,
                token: format!("native-report.{}", "a".repeat(32)),
            },
            "fixture-policy".into(),
        )
        .unwrap();
        reporter.blocked.store(true, Ordering::Relaxed);
        assert_eq!(reporter.finish(&journal, "audit-proof-0001"), (true, true));
        server.join().unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn audit_capability_refuses_url_credentials_paths_and_unknown_token_formats() {
        let token = format!("native-report.{}", "a".repeat(32));
        for origin in [
            "https://example.invalid/path",
            "https://user:pass@example.invalid",
            "http://example.invalid",
            "https://example.invalid/?token=abc",
        ] {
            assert!(
                AuditConfig {
                    origin: origin.into(),
                    token: token.clone()
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            AuditConfig {
                origin: "https://example.invalid".into(),
                token: "wrong".into()
            }
            .validate()
            .is_err()
        );
    }
}
