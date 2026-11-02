//! Agent requests for an installed Stellar wallet profile. The public wallet
//! profile names the wallet, its roles and the engine endpoints; the agent
//! key signs. Requests go to the profile's `/api/native/execute` endpoints and
//! are journaled in caller-supplied private storage before they leave the process.
use crate::{
    config::parse_origin,
    error::{Error, Result},
    typed_request::operation_nonce,
};
use allowit_sdk::{
    client::store::Store,
    transport::{
        http::{HttpClient, Method, Request},
        rpc::Options,
    },
};
use allowit_stellar_wallet::workflow::ExecutionRequest;
use allowit_stellar_wallet::{
    self as sdk, DetachedSignature, OperationStatus, PreparedOperation, SignedOperation,
    WalletConfig, WalletRoute,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Bundle {
    version: u8,
    profile: String,
    pub network: String,
    pub policy_id: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    portable: Option<PortableProfile>,
    source_hash: String,
    #[serde(default)]
    ir_hash: String,
    pub module_digest: String,
    pub wallet: String,
    route: WalletRoute,
    pub config: WalletConfig,
    endpoints: Endpoints,
    /// A public profile never holds this; reading refuses a file that does.
    #[serde(default, deserialize_with = "local_secret")]
    signing_seed: Option<Zeroizing<String>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PortableProfile {
    language: String,
    source_hash: String,
    tier: String,
    #[serde(default)]
    rules: Vec<String>,
    params: sdk::PolicyParams,
    params_hash: String,
    asset: PortableAsset,
    unresolved: Vec<String>,
    build: PortableBuild,
    #[serde(default)]
    kinds: std::collections::BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PortableAsset {
    symbol: String,
    address: String,
    decimals: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PortableBuild {
    prefix: Vec<String>,
    writes_state: bool,
    library_revision: String,
    attempts: usize,
}
fn validate_portable_profile(bundle: &Bundle) -> Result<()> {
    let Some(portable) = &bundle.portable else {
        return Err(Error::config(
            "The portable Stellar profile is missing its source binding",
        ));
    };
    let shape = crate::authoring::check(&bundle.config.policy_source)?;
    let params = serde_json_canonicalizer::to_vec(&portable.params)
        .map_err(|_| Error::config("Cannot encode portable Stellar params"))?;
    if bundle.language.as_deref() != Some("allowit-portable-v1")
        || portable.language != "allowit-portable-v1"
        || portable.source_hash != bundle.source_hash
        || portable.params != bundle.config.params
        || portable.params_hash != allowit_sdk::digest(&params)
        || portable.asset.address != bundle.config.asset
        || portable.asset.symbol != bundle.config.asset_symbol
        || portable.asset.decimals != 7
        || !portable.unresolved.is_empty()
        || portable.build.prefix != shape.prefix
        || portable.build.writes_state != shape.writes_state
    {
        return Err(Error::config(
            "The portable Stellar descriptor differs from its installed configuration",
        ));
    }
    let tier = match portable.tier.as_str() {
        "standard" => sdk::PolicyTier::Common,
        "custom" => sdk::PolicyTier::Custom,
        _ => return Err(Error::config("Invalid portable Stellar policy tier")),
    };
    checked(portable.params.check(tier))?;
    checked(sdk::policy::parse_rules(&portable.rules))?;
    // Library revision, attempts and param kinds describe authoring. They are never
    // publication proof or signing authority supplied by the public descriptor.
    let _ = (
        &portable.build.library_revision,
        portable.build.attempts,
        &portable.kinds,
    );
    if bundle.route == WalletRoute::Native {
        let identity = json!({"language":portable.language,"tier":portable.tier,"rules":portable.rules,"sourceHash":portable.source_hash,"paramsHash":portable.params_hash});
        let identity = serde_json_canonicalizer::to_vec(&identity)
            .map_err(|_| Error::config("Cannot encode portable Stellar identity"))?;
        if allowit_sdk::digest(&identity) != bundle.module_digest {
            return Err(Error::config(
                "The Native Stellar module identity differs from its portable source",
            ));
        }
    }
    Ok(())
}
fn local_secret<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> std::result::Result<Option<Zeroizing<String>>, D::Error> {
    String::deserialize(decoder).map(|value| Some(Zeroizing::new(value)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Endpoints {
    execute: String,
    status: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Response {
    prepared: PreparedOperation,
    operation: Operation,
    signed_operation: Option<SignedOperation>,
    receipt: Option<sdk::rpc::VerifiedReceipt>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Operation {
    id: String,
    status: OperationStatus,
    transaction_hash: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Record {
    policy_id: String,
    request_id: String,
    config: WalletConfig,
    request: ExecutionRequest,
    prepared: Option<PreparedOperation>,
    agent_signature: Option<DetachedSignature>,
    status: Option<OperationStatus>,
}
pub struct Client {
    http: HttpClient,
    origin: String,
    bundle: Bundle,
    signer: Arc<dyn AgentSigner>,
    account: String,
    rpc_url: String,
}
fn revalidate_with_rpc_url(
    p: &PreparedOperation,
    now: u64,
    value: &str,
) -> Result<PreparedOperation> {
    if p.config.route == WalletRoute::Native {
        return Ok(p.clone());
    }
    if value.is_empty() {
        return Err(Error::config(
            "policy wallet signing revalidates through Stellar RPC; pass --rpc-url URL or set rpc_url for this rail",
        ));
    }
    let runtime = checked(super::Runtime::new(value))?;
    checked(runtime.revalidate_unsigned(p, now))
}
fn clock() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .map_err(|_| Error::config("System clock precedes the Unix epoch"))
}
fn checked<T>(v: sdk::Result<T>) -> Result<T> {
    v.map_err(Error::config)
}
fn journal_error(_: Error) -> Error {
    Error::config("Stellar operation journal is unsafe or unreadable; retain the original journal")
}
fn same_policy_config(expected: &WalletConfig, actual: &WalletConfig) -> bool {
    let mut current = actual.clone();
    // The server signer is checked against the prepared live chain state, not frozen token metadata.
    current.server = expected.server.clone();
    &current == expected
}
fn same_retained_policy_config(expected: &WalletConfig, actual: &WalletConfig) -> bool {
    let mut original = actual.clone();
    // Public role identities may rotate. Read-only recovery still binds the original approved source and exact packet.
    original.agent = expected.agent.clone();
    same_policy_config(expected, &original)
}
fn validate_bundle(policy: &str, bundle: &Bundle) -> Result<()> {
    if bundle.version != 1
        || bundle.profile != sdk::PROFILE
        || bundle.policy_id != policy
        || bundle.network != bundle.config.network
        || bundle.wallet != bundle.config.wallet
        || bundle.route != bundle.config.route
        || checked(sdk::unhex::<32>(&bundle.source_hash))? != bundle.config.source_hash
        || checked(sdk::unhex::<32>(&bundle.module_digest))? != bundle.config.module_digest
        || (!bundle.ir_hash.is_empty() && sdk::unhex::<32>(&bundle.ir_hash).is_err())
        || (bundle.ir_hash.is_empty() && bundle.config.language != sdk::PolicyLanguage::Portable)
        || bundle.endpoints.execute != "/api/native/execute"
        || bundle.endpoints.status != "/api/native/execute/status"
    {
        return Err(Error::config(
            "The Stellar bundle does not match the installed policy profile",
        ));
    }
    checked(sdk::validate_config(&bundle.config))?;
    if bundle.config.language == sdk::PolicyLanguage::Portable {
        validate_portable_profile(bundle)?;
    } else if bundle.language.is_some() || bundle.portable.is_some() {
        return Err(Error::config(
            "A retained legacy profile cannot claim portable source",
        ));
    }
    Ok(())
}
pub fn read_public_bundle(path: &Path) -> Result<Bundle> {
    // The file reader reports request-file errors; name the profile instead.
    let mut bundle: Bundle = crate::typed_request::read_request_file(path).map_err(|_| {
        Error::config(format!(
            "{} is not a readable public Stellar wallet profile",
            path.display()
        ))
    })?;
    if let Some(_private) = bundle.signing_seed.take() {
        return Err(Error::config(
            "The public Stellar bundle must not contain signing material",
        ));
    }
    validate_bundle(&bundle.policy_id, &bundle)?;
    if bundle.config.owner == bundle.config.agent
        || bundle.config.owner == bundle.config.server
        || bundle.config.agent == bundle.config.server
    {
        return Err(Error::config(
            "The public Stellar wallet must use distinct owner, agent and server identities",
        ));
    }
    Ok(bundle)
}
pub fn public_description(bundle: &Bundle, source: bool) -> serde_json::Value {
    let mut value = json!({
        "policyId":bundle.policy_id,"profile":bundle.profile,"network":bundle.network,
        "wallet":bundle.wallet,"route":bundle.route,"owner":bundle.config.owner,
        "agent":bundle.config.agent,"server":bundle.config.server,
        "sourceHash":bundle.source_hash,"irHash":bundle.ir_hash,"moduleDigest":bundle.module_digest,
        "asset":bundle.config.asset,"assetSymbol":bundle.config.asset_symbol,"assetIssuer":bundle.config.asset_issuer,
        "maxFeeStroops":bundle.config.max_fee_stroops
    });
    if source {
        value["policy"] = json!(bundle.config.policy_source);
    }
    value
}
/// A Stellar key capability. Its implementation owns key custody outside this workflow.
pub trait AgentSigner: Send + Sync {
    fn account(&self) -> &str;
    fn sign_hash(&self, digest: &[u8; 32]) -> sdk::Result<[u8; 64]>;
}
fn validate_response_receipt(r: &Response) -> Result<()> {
    let p = &r.prepared;
    if let Some(receipt) = &r.receipt {
        checked(sdk::rpc::validate_receipt(
            receipt,
            &p.config.network,
            &p.transaction_hash,
        ))?;
        if super::projected_status(&p.config.route, receipt.status.clone()) != r.operation.status
            || r.signed_operation
                .as_ref()
                .is_none_or(|v| v.transaction_xdr != receipt.envelope_xdr)
        {
            return Err(Error::uncertain(
                "Stellar receipt differs from its retained signed operation",
            ));
        }
    }
    Ok(())
}
impl Client {
    pub fn new(
        bundle: Bundle,
        signer: Arc<dyn AgentSigner>,
        origin: &str,
        rpc_url: Option<String>,
    ) -> Result<Self> {
        let account = signer.account().to_owned();
        if account != bundle.config.agent {
            return Err(Error::config(
                "the agent key differs from the wallet profile agent",
            ));
        }
        validate_bundle(&bundle.policy_id, &bundle)?;
        Ok(Self {
            http: HttpClient::new(Options {
                timeout: Duration::from_secs(30),
                max_response_bytes: 1_048_576,
                no_proxy: false,
                connect_timeout: None,
            })
            .map_err(|_| Error::config("Cannot initialize Stellar transport"))?,
            origin: parse_origin(origin)?,
            bundle,
            signer,
            account,
            rpc_url: rpc_url.unwrap_or_default(),
        })
    }
    fn post(&self, path: &str, body: serde_json::Value) -> Result<Response> {
        use ed25519_dalek::{Signature, VerifyingKey};
        let issued_at = clock()?;
        let canonical = serde_json_canonicalizer::to_vec(&body)
            .map_err(|_| Error::config("Cannot encode Stellar request"))?;
        let mut message = format!("allowit-stellar-request-v1\n{issued_at}\n{path}\n").into_bytes();
        message.extend_from_slice(&canonical);
        let digest = sdk::hash(message);
        let signature = checked(self.signer.sign_hash(&digest))?;
        checked(
            VerifyingKey::from_bytes(&checked(sdk::account_key(&self.account))?)
                .map_err(|_| "Invalid Stellar signing account".to_owned())
                .and_then(|key| {
                    key.verify_strict(&digest, &Signature::from_bytes(&signature))
                        .map_err(|_| {
                            "Signing capability returned an invalid request signature".to_owned()
                        })
                }),
        )?;
        let proof = format!(
            "Bearer stellar-proof.{}.{issued_at}.{}",
            self.account,
            sdk::hex(&signature)
        );
        let bytes = serde_json::to_vec(&body)
            .map_err(|_| Error::config("Cannot encode Stellar request"))?;
        let headers = [
            ("Authorization".into(), proof),
            ("Content-Type".into(), "application/json".into()),
        ];
        let response = self
            .http
            .send_success_body(
                Request {
                    method: Method::Post,
                    url: &format!("{}{path}", self.origin),
                    headers: &headers,
                    body: Some(&bytes),
                },
                None,
            )
            .map_err(|_| {
                Error::uncertain(
                    "Stellar transport is uncertain; recover the original request ID with status",
                )
            })?;
        if !(200..300).contains(&response.status) {
            return Err(Error::api(
                response.status,
                "Stellar operation was not resolved; retain the original request ID",
            ));
        }
        let mut parsed: Response = serde_json::from_slice(&response.body).map_err(|_| {
            Error::uncertain("Invalid Stellar response; recover the original request ID")
        })?;
        parsed.operation.status =
            super::projected_status(&parsed.prepared.config.route, parsed.operation.status);
        Ok(parsed)
    }
    fn validate(
        &self,
        r: &Response,
        id: &str,
        record: Option<&Record>,
        read_only: bool,
    ) -> Result<()> {
        let config_matches = if read_only {
            same_retained_policy_config
        } else {
            same_policy_config
        };
        let p = &r.prepared;
        if r.operation.id != id
            || !config_matches(&self.bundle.config, &p.config)
            || p.request.operation.nonce != operation_nonce(&self.bundle.policy_id, id)
            || r.operation.transaction_hash != sdk::hex(&p.transaction_hash)
            || record.is_some_and(|old| {
                p.request != old.request
                    || !config_matches(&old.config, &p.config)
                    || old.prepared.as_ref().is_some_and(|v| v != p)
            })
        {
            return Err(Error::uncertain(
                "Stellar response changed the original operation identity",
            ));
        }
        if terminal(&r.operation.status) && r.receipt.is_none() {
            return Err(Error::uncertain(
                "Terminal Stellar status requires a verified chain receipt",
            ));
        }
        validate_response_receipt(r)?;
        // Status verifies retained bytes at their preparation time, including after expiry.
        checked(sdk::validate_retained_prepared(p))?;
        if let Some(s) = &r.signed_operation {
            if s.prepared != *p {
                return Err(Error::uncertain(
                    "Stellar signed packet differs from the original operation",
                ));
            }
            if s.prepared.config.route == WalletRoute::PolicyWallet {
                if let Some(receipt) = &r.receipt {
                    checked(sdk::rpc::validate_settled_policy_packet(s, receipt))?;
                } else {
                    checked(sdk::validate_retained_signed(s))?;
                }
            } else {
                checked(sdk::validate_retained_signed(s))?;
            }
        }
        if matches!(
            r.operation.status,
            OperationStatus::Submitted | OperationStatus::Confirmed | OperationStatus::Failed
        ) && r.signed_operation.is_none()
        {
            return Err(Error::uncertain(
                "Stellar settlement omitted its signed packet",
            ));
        }
        if let Some(old) = record.and_then(|v| v.status.as_ref())
            && terminal(&super::projected_status(&p.config.route, old.clone()))
            && super::projected_status(&p.config.route, old.clone()) != r.operation.status
        {
            return Err(Error::uncertain("A terminal Stellar receipt changed"));
        }
        Ok(())
    }
    fn status(&self, id: &str, record: Option<&Record>) -> Result<Response> {
        let r = self.post(
            &self.bundle.endpoints.status,
            json!({"policyId":self.bundle.policy_id,"requestId":id}),
        )?;
        self.validate(&r, id, record, true)?;
        Ok(r)
    }
}
/// The request retained under this policy and request ID, if any. `exec`
/// checks the rest of the record.
pub fn retained_request(
    journal: &Store,
    policy: &str,
    id: &str,
) -> Result<Option<ExecutionRequest>> {
    let name = sdk::hex(&operation_nonce(policy, id));
    let old: Option<Record> = journal.load(&name).map_err(journal_error)?;
    Ok(old.map(|r| r.request))
}
/// Reads one request by its ID without creating another.
pub fn status(c: &Client, journal: &Store, id: &str, wait: Duration) -> Result<ExecutionOutcome> {
    validate_wait_and_id(id, wait)?;
    let policy = c.bundle.policy_id.clone();
    let name = sdk::hex(&operation_nonce(&policy, id));
    let old: Option<Record> = journal.load(&name).map_err(journal_error)?;
    if old.as_ref().is_some_and(|v| {
        v.policy_id != policy
            || v.request_id != *id
            || !same_retained_policy_config(&c.bundle.config, &v.config)
    }) {
        return Err(Error::config(
            "The journal does not bind this installed Stellar policy",
        ));
    }
    poll(c, id, old, journal, &name, wait)
}
/// Prepares, signs and submits one typed request, then polls within `wait`.
pub fn exec(
    c: &Client,
    journal: &Store,
    request_id: &str,
    request: ExecutionRequest,
    wait: Duration,
) -> Result<ExecutionOutcome> {
    validate_wait_and_id(request_id, wait)?;
    let policy = c.bundle.policy_id.clone();
    let id = request_id.to_owned();
    if request.operation.nonce != operation_nonce(&policy, &id) {
        return Err(Error::usage(
            "The operation nonce does not bind this policy and request ID",
        ));
    }
    checked(sdk::operation_from_request(&c.bundle.config, &request))?;
    let name = sdk::hex(&request.operation.nonce);
    let old = journal.load::<Record>(&name).map_err(journal_error)?;
    if c.bundle.config.language != sdk::PolicyLanguage::Portable
        && old.as_ref().is_none_or(|record| {
            record.agent_signature.is_none()
                && matches!(record.status, None | Some(OperationStatus::Prepared))
        })
    {
        return Err(Error::unsupported(
            "Retired policy source supports original signed-operation status only",
        ));
    }
    let (mut record, resumed) = if let Some(old) = old {
        if old.policy_id != policy
            || old.request_id != id
            || !same_retained_policy_config(&c.bundle.config, &old.config)
            || old.request != request
        {
            return Err(Error::config(
                "The original request ID already binds different input",
            ));
        }
        if old.agent_signature.is_some()
            || !matches!(&old.status, None | Some(OperationStatus::Prepared))
        {
            return poll(c, &id, Some(old), journal, &name, wait);
        }
        if !same_policy_config(&c.bundle.config, &old.config) {
            return Err(Error::uncertain(
                "The customer key changed. Read the original operation with status; do not authorize replacement bytes.",
            ));
        }
        let pure_intent = old.prepared.is_none() && old.status.is_none();
        let resumed = match c.status(&id, Some(&old)) {
            Ok(response)
                if response.operation.status == OperationStatus::Prepared
                    && response.signed_operation.is_none() =>
            {
                Some(response)
            }
            Ok(_) => return poll(c, &id, Some(old), journal, &name, wait),
            Err(error) if error.status == Some(404) && pure_intent => None,
            Err(error) => return Err(error),
        };
        (old, resumed)
    } else {
        (
            Record {
                policy_id: policy.clone(),
                request_id: id.clone(),
                config: c.bundle.config.clone(),
                request: request.clone(),
                prepared: None,
                agent_signature: None,
                status: None,
            },
            None,
        )
    };
    // Preserve the original intent. Only an explicit exec can continue unsigned preparation.
    journal.save(&name, &record).map_err(journal_error)?;
    let mut prepared = if let Some(response) = resumed {
        response
    } else {
        c.post(
            &c.bundle.endpoints.execute,
            json!({"policyId":policy,"requestId":id,"phase":"prepare","request":request}),
        )?
    };
    c.validate(&prepared, &id, Some(&record), false)?;
    if prepared.operation.status != OperationStatus::Prepared || prepared.signed_operation.is_some()
    {
        return Err(Error::uncertain(
            "Stellar prepare returned an unexpected lifecycle state",
        ));
    }
    // Retain observed bytes even when stale. They must never become a fresh all-None intent.
    record.prepared = Some(prepared.prepared.clone());
    record.status = Some(OperationStatus::Prepared);
    journal.save(&name, &record).map_err(journal_error)?;
    prepared.prepared = revalidate_with_rpc_url(&prepared.prepared, clock()?, &c.rpc_url)?;
    checked(sdk::validate_prepared(&prepared.prepared, clock()?))?;
    let signature = checked(sdk::sign_with(
        &prepared.prepared,
        &c.account,
        clock()?,
        |hash| c.signer.sign_hash(hash),
    ))?;
    record.prepared = Some(prepared.prepared);
    record.agent_signature = Some(signature.clone());
    record.status = Some(OperationStatus::SubmissionUnknown);
    journal.save(&name, &record).map_err(journal_error)?;
    let response = c.post(
        &c.bundle.endpoints.execute,
        json!({"policyId":policy,"requestId":id,"phase":"submit","agentSignature":signature}),
    )?;
    c.validate(&response, &id, Some(&record), false)?;
    record.status = Some(response.operation.status.clone());
    journal.save(&name, &record).map_err(journal_error)?;
    if terminal(&response.operation.status) || wait.is_zero() {
        return Ok(render(&response));
    }
    poll(c, &id, Some(record), journal, &name, wait)
}
fn validate_wait_and_id(id: &str, wait: Duration) -> Result<()> {
    if !crate::request::request_id_valid(id) {
        return Err(Error::usage("Invalid Stellar request ID"));
    }
    if wait > Duration::from_secs(600) {
        return Err(Error::usage(
            "Stellar polling waits must not exceed 10 minutes",
        ));
    }
    Ok(())
}
fn terminal(status: &OperationStatus) -> bool {
    matches!(status, OperationStatus::Confirmed | OperationStatus::Failed)
}
fn poll(
    c: &Client,
    id: &str,
    mut record: Option<Record>,
    journal: &Store,
    name: &str,
    wait: Duration,
) -> Result<ExecutionOutcome> {
    let deadline = std::time::Instant::now()
        .checked_add(wait)
        .ok_or_else(|| Error::usage("The Stellar polling deadline is too large"))?;
    loop {
        let r = c.status(id, record.as_ref())?;
        let mut old = match record {
            Some(old) => old,
            None => Record {
                policy_id: c.bundle.policy_id.clone(),
                request_id: id.into(),
                config: c.bundle.config.clone(),
                request: r.prepared.request.clone(),
                prepared: Some(r.prepared.clone()),
                agent_signature: None,
                status: None,
            },
        };
        if old.prepared.is_none() {
            old.prepared = Some(r.prepared.clone());
        }
        old.status = Some(r.operation.status.clone());
        journal.save(name, &old).map_err(journal_error)?;
        record = Some(old);
        if terminal(&r.operation.status) || std::time::Instant::now() >= deadline {
            return Ok(render(&r));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionOutcome {
    pub request_id: String,
    pub status: OperationStatus,
    pub transaction_hash: String,
}
fn render(r: &Response) -> ExecutionOutcome {
    ExecutionOutcome {
        request_id: r.operation.id.clone(),
        status: r.operation.status.clone(),
        transaction_hash: r.operation.transaction_hash.clone(),
    }
}

#[cfg(test)]
mod portable_descriptor_tests {
    use super::*;
    fn fixtures() -> Vec<serde_json::Value> {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/stellar-portable-descriptors.json"
        ))
        .unwrap()
    }
    #[test]
    fn actual_native_and_all_policy_wallet_descriptor_shapes_are_accepted() {
        for vector in fixtures() {
            let bundle: Bundle = serde_json::from_value(vector["bundle"].clone()).unwrap();
            validate_bundle("portable-policy", &bundle).unwrap();
            let mut omitted_ir = vector["bundle"].clone();
            omitted_ir.as_object_mut().unwrap().remove("irHash");
            let bundle: Bundle = serde_json::from_value(omitted_ir).unwrap();
            validate_bundle("portable-policy", &bundle).unwrap();
        }
    }
    #[test]
    fn portable_descriptor_mismatch_and_unknown_or_private_fields_are_refused() {
        let original = fixtures()[0]["bundle"].clone();
        for field in ["unexpected", "signing_seed", "privateKey"] {
            let mut changed = original.clone();
            changed[field] = json!("not-public");
            assert!(
                serde_json::from_value::<Bundle>(changed).is_err(),
                "{field}"
            );
        }
        for pointer in [
            "/language",
            "/portable/sourceHash",
            "/portable/paramsHash",
            "/portable/asset/address",
        ] {
            let mut changed = original.clone();
            *changed.pointer_mut(pointer).unwrap() = json!("changed");
            let bundle: Bundle = serde_json::from_value(changed).unwrap();
            assert!(
                validate_bundle("portable-policy", &bundle).is_err(),
                "{pointer}"
            );
        }
        let mut missing = original;
        missing.as_object_mut().unwrap().remove("portable");
        let bundle: Bundle = serde_json::from_value(missing).unwrap();
        assert!(validate_bundle("portable-policy", &bundle).is_err());
    }
}

#[cfg(test)]
mod public_bundle_tests {
    use super::*;
    use crate::typed_request::decode_bytes;
    fn fixture() -> serde_json::Value {
        let f: serde_json::Value = serde_json::from_str(include_str!(
            "../../../cli/tests/fixtures/stellar-approval.json"
        ))
        .unwrap();
        let policy = &f["policy"];
        let config = WalletConfig {
            network: "stellar:testnet".into(),
            policy_instance: [7; 32],
            wallet: f["intent"]["owner"].as_str().unwrap().into(),
            owner: f["intent"]["owner"].as_str().unwrap().into(),
            agent: sdk::account_for_seed(&[2; 32]),
            server: sdk::account_for_seed(&[3; 32]),
            route: WalletRoute::Native,
            tier: None,
            language: Default::default(),
            rules: vec![],
            params: Default::default(),
            source_digest: [0; 32],
            asset: policy["mint"].as_str().unwrap().into(),
            asset_symbol: "XLM".into(),
            asset_issuer: None,
            policy_source: policy["code"].as_str().unwrap().into(),
            source_hash: sdk::unhex(policy["sourceHash"].as_str().unwrap()).unwrap(),
            module_digest: [5; 32],
            max_fee_stroops: 100,
        };
        json!({"version":1,"profile":sdk::PROFILE,"network":config.network,
            "policyId":policy["id"],"sourceHash":policy["sourceHash"],"irHash":policy["irHash"],
            "moduleDigest":sdk::hex(&config.module_digest),"wallet":config.wallet,
            "route":"native","config":config,
            "endpoints":{"execute":"/api/native/execute","status":"/api/native/execute/status"}})
    }
    #[test]
    fn public_inspection_binds_roles_and_source_without_loading_a_signer() {
        let value = fixture();
        let bundle: Bundle = decode_bytes(&serde_json::to_vec(&value).unwrap()).unwrap();
        validate_bundle("existing-native-policy", &bundle).unwrap();
        let description = public_description(&bundle, false);
        assert!(description.get("policy").is_none());
        assert!(description.get("signingSeed").is_none());
        assert_eq!(description["agent"], sdk::account_for_seed(&[2; 32]));
        assert!(description["assetIssuer"].is_null());
        assert_eq!(
            public_description(&bundle, true)["policy"],
            value["config"]["policySource"]
        );
        let mut wrong: Bundle = decode_bytes(&serde_json::to_vec(&value).unwrap()).unwrap();
        wrong.network = "stellar:mainnet".into();
        assert!(validate_bundle("existing-native-policy", &wrong).is_err());
        let mut issued: Bundle = decode_bytes(&serde_json::to_vec(&value).unwrap()).unwrap();
        issued.config.asset_symbol = "XLM".into();
        issued.config.asset_issuer = Some(sdk::account_for_seed(&[8; 32]));
        issued.config.asset = sdk::sac_address(
            &issued.config.network,
            &sdk::asset_xdr(&issued.config).unwrap(),
        )
        .unwrap();
        validate_bundle("existing-native-policy", &issued).unwrap();
        assert_eq!(
            public_description(&issued, false)["assetIssuer"],
            sdk::account_for_seed(&[8; 32])
        );
    }
    #[test]
    fn public_profile_refuses_private_fields_and_changed_identity() {
        let path = std::env::temp_dir().join(format!(
            "allowit-stellar-public-{}.json",
            std::process::id()
        ));
        let mut value = fixture();
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let bundle = read_public_bundle(&path).unwrap();
        assert_eq!(bundle.policy_id, "existing-native-policy");
        assert_eq!(bundle.network, "stellar:testnet");
        value["signingSeed"] = json!("must-stay-local");
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(read_public_bundle(&path).is_err());
        value.as_object_mut().unwrap().remove("signingSeed");
        value["config"]["agent"] = value["config"]["owner"].clone();
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(read_public_bundle(&path).is_err());
        value = fixture();
        value["config"]["sourceHash"] = json!(vec![0; 32]);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(read_public_bundle(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod native_revalidation_tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../../cli/tests/fixtures/stellar-policy-wallet-revalidation-rpc.json"
        ))
        .unwrap()
    }
    #[test]
    fn local_rpc_revalidates_exact_gateway_bytes_before_signing_without_credentials() {
        let data = fixture();
        let p: PreparedOperation = serde_json::from_value(data["prepared"].clone()).unwrap();
        let calls = data["calls"].as_array().unwrap().clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for expected in calls {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let end = loop {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                    if bytes.ends_with(b"\r\n\r\n") {
                        break bytes.len();
                    }
                };
                let headers = std::str::from_utf8(&bytes).unwrap().to_ascii_lowercase();
                assert!(!headers.contains("authorization:"));
                assert!(!headers.contains("cookie:"));
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                bytes.resize(end + length, 0);
                stream.read_exact(&mut bytes[end..]).unwrap();
                let body = std::str::from_utf8(&bytes[end..]).unwrap();
                assert!(!body.contains("Bearer"));
                assert!(!body.contains("signing_seed"));
                assert!(!body.contains("ALLOWIT_TOKEN"));
                let request: serde_json::Value = serde_json::from_str(body).unwrap();
                assert_eq!(request["method"], expected["method"]);
                assert_eq!(request["params"], expected["params"]);
                assert_eq!(request["jsonrpc"], "2.0");
                let response = serde_json::to_vec(
                    &json!({"jsonrpc":"2.0","id":request["id"],"result":expected["result"]}),
                )
                .unwrap();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",response.len()).unwrap();
                stream.write_all(&response).unwrap();
            }
        });
        let checked = revalidate_with_rpc_url(&p, 86402, &url).unwrap();
        server.join().unwrap();
        assert_eq!(checked, p);
        assert_eq!(checked.transaction_xdr, p.transaction_xdr);
        assert_eq!(checked.transaction_hash, p.transaction_hash);
        assert!(checked.native_preflight.is_some());
        sdk::sign(&checked, &[2; 32], 86402).unwrap();
    }
    #[test]
    fn missing_rpc_stale_and_tampered_intents_fail_before_signing_but_classic_bypasses() {
        let data = fixture();
        let p: PreparedOperation = serde_json::from_value(data["prepared"].clone()).unwrap();
        assert!(revalidate_with_rpc_url(&p, 86402, "").is_err());
        assert!(revalidate_with_rpc_url(&p, 86700, "http://127.0.0.1:1").is_err());
        let mut changed = p.clone();
        changed.request.operation.nonce[0] ^= 1;
        assert!(revalidate_with_rpc_url(&changed, 86402, "http://127.0.0.1:1").is_err());
        let old: SignedOperation = serde_json::from_str(include_str!(
            "../../../cli/tests/fixtures/canonical-main-9682-native-signed.json"
        ))
        .unwrap();
        assert_eq!(
            revalidate_with_rpc_url(&old.prepared, 86402, "").unwrap(),
            old.prepared
        );
    }
    #[test]
    fn failed_policy_wallet_envelope_remains_unknown_on_the_server_client_wire() {
        use ed25519_dalek::Signer;
        use stellar_xdr::{
            Limits, TransactionResult, TransactionResultExt, TransactionResultResult, WriteXdr,
        };
        struct TestKey {
            account: String,
        }
        impl AgentSigner for TestKey {
            fn account(&self) -> &str {
                &self.account
            }
            fn sign_hash(&self, hash: &[u8; 32]) -> sdk::Result<[u8; 64]> {
                Ok(ed25519_dalek::SigningKey::from_bytes(&[42; 32])
                    .sign(hash)
                    .to_bytes())
            }
        }
        let account = sdk::account_for_seed(&[42; 32]);
        let data = fixture();
        let signed: SignedOperation = serde_json::from_value(data["signed"].clone()).unwrap();
        let result = TransactionResult {
            fee_charged: 100,
            result: TransactionResultResult::TxFailed(Default::default()),
            ext: TransactionResultExt::V0,
        };
        let receipt = sdk::rpc::VerifiedReceipt {
            transaction_hash: signed.transaction_hash,
            status: OperationStatus::Failed,
            ledger: 777,
            envelope_xdr: signed.transaction_xdr.clone(),
            result_xdr: result.to_xdr_base64(Limits::none()).unwrap(),
        };
        sdk::rpc::validate_settled_policy_packet(&signed, &receipt).unwrap();
        let body = serde_json::to_vec(&json!({"prepared":signed.prepared,"signedOperation":signed,"operation":{"id":"wire-status-001","status":OperationStatus::SubmissionUnknown,"transactionHash":sdk::hex(&receipt.transaction_hash)},"receipt":receipt})).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let headers = String::from_utf8(request).unwrap();
            let length: usize = headers
                .lines()
                .find_map(|v| {
                    v.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|s| s.trim().parse().unwrap())
                })
                .unwrap();
            let mut request = vec![0; length];
            stream.read_exact(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        });
        let vectors: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../tests/fixtures/stellar-portable-descriptors.json"
        ))
        .unwrap();
        let mut bundle: Bundle = serde_json::from_value(vectors[1]["bundle"].clone()).unwrap();
        bundle.config.agent = account.clone();
        let client = Client::new(bundle, Arc::new(TestKey { account }), &origin, None).unwrap();
        let response = client
            .post(
                "/api/native/execute/status",
                json!({"requestId":"wire-status-001"}),
            )
            .unwrap();
        server.join().unwrap();
        assert_eq!(
            response.operation.status,
            OperationStatus::SubmissionUnknown
        );
        validate_response_receipt(&response).unwrap();
        let receipt = response.receipt.unwrap();
        assert_eq!(receipt.status, OperationStatus::Failed);
        assert_eq!(
            super::super::projected_status(&response.prepared.config.route, receipt.status),
            response.operation.status
        );
    }
}
