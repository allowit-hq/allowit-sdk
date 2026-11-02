//! Owner HTTP authentication and portable generation. This workflow grants no spending authority.
use crate::{
    config::parse_origin,
    error::{Error, Result},
};
use allowit_sdk::transport::{
    http::{HttpClient, Method, Request, Response},
    rpc::Options,
};
use allowit_stellar_wallet::{self as native, PolicyParams, PolicyTier};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use zeroize::Zeroizing;

const SOURCE_PROFILE: &str = "allowit-stellar-transfer-v1";
const OPERATION: &str = "stellar::sep41_transfer";
const LANGUAGE: &str = "allowit-portable-v1";

/// The caller owns this owner key. It is independent of any selected agent key.
pub trait OwnerSigner {
    fn account(&self) -> &str;
    fn sign_hash(&self, digest: &[u8; 32]) -> native::Result<[u8; 64]>;
}

#[derive(Clone, Debug)]
pub enum Target {
    NewWallet,
    Installation(String),
}
#[derive(Clone, Debug)]
pub struct GenerateRequest {
    pub prompt: String,
    pub network: String,
    pub profile: String,
    pub target: Target,
}
impl GenerateRequest {
    pub fn validate(&self) -> Result<()> {
        if !(5..=3000).contains(&self.prompt.len()) {
            return Err(Error::usage("Describe the policy in 5–3000 UTF-8 bytes"));
        }
        crate::routing::info(&self.network)?;
        if self.profile.is_empty()
            || self.profile.len() > 128
            || !self
                .profile
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
        {
            return Err(Error::usage("Select an advertised Stellar wallet profile"));
        }
        if let Target::Installation(id) = &self.target {
            lower_hash(id)?;
        }
        Ok(())
    }
}

pub struct OwnerClient {
    http: HttpClient,
    backend: String,
    origin: String,
    owner: String,
    session: Option<OwnerSession>,
}
struct OwnerSession {
    cookie: Zeroizing<String>,
    expires: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Challenge {
    id: String,
    message: String,
    address: String,
    origin: String,
    expires_at: String,
}
#[derive(Deserialize)]
struct Authenticated {
    session: SessionInfo,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SessionInfo {
    address: String,
    network: String,
    expires_at: String,
}
#[derive(Deserialize)]
struct Discovery {
    wallets: Vec<Advertisement>,
    operations: Vec<OperationAdvertisement>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Advertisement {
    kind: String,
    profile: String,
    source_profiles: Vec<String>,
    networks: Vec<String>,
    operations: Vec<String>,
    capabilities: GenerateCapability,
}
#[derive(Deserialize)]
struct GenerateCapability {
    generate: bool,
}
#[derive(Deserialize)]
struct OperationAdvertisement {
    function: String,
}
#[derive(Deserialize)]
struct Templates {
    templates: Vec<Template>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Template {
    network: String,
    profiles: Vec<String>,
    asset: String,
    asset_symbol: String,
    asset_issuer: Option<String>,
    max_fee_stroops: u32,
}
#[derive(Deserialize)]
struct Installations {
    installations: Vec<Installation>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Installation {
    policy_id: String,
    config: native::WalletConfig,
    profile: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Generated {
    policies: Vec<Box<serde_json::value::RawValue>>,
    selected_id: String,
}

/// Only public generation fields are returned. Extra server state and credentials are never copied.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedPolicy {
    pub id: String,
    pub revision: u32,
    pub network: String,
    pub language: String,
    pub original_intent: String,
    pub title: String,
    pub explanation: String,
    pub code: String,
    pub source_hash: String,
    pub ir_hash: String,
    pub status: String,
    pub token: String,
    pub mint: String,
    pub runtime: PublicRuntime,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicRuntime {
    pub source_profile: String,
    pub wallet: WalletSelection,
    pub operation: String,
    pub instance_binding: PolicyBinding,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct WalletSelection {
    pub kind: String,
    pub profile: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolicyBinding {
    pub profile: String,
    pub language: String,
    pub deployment_id: String,
    pub policy_instance: String,
    pub module_digest: String,
    pub wallet: String,
    pub route: String,
    pub asset: String,
    pub asset_symbol: String,
    pub asset_issuer: Option<String>,
    pub max_fee_stroops: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<PolicyTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<bool>,
    pub portable: PortableDraft,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableDraft {
    pub language: String,
    pub source_hash: String,
    pub tier: String,
    #[serde(default)]
    pub rules: Vec<String>,
    pub params: PolicyParams,
    pub params_hash: String,
    pub asset: PortableAsset,
    pub unresolved: Vec<String>,
    pub build: BuildReport,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub kinds: std::collections::BTreeMap<String, String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableAsset {
    pub symbol: String,
    pub address: String,
    pub decimals: u32,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildReport {
    pub prefix: Vec<String>,
    pub writes_state: bool,
    pub library_revision: String,
    pub attempts: usize,
}

fn lower_hash(value: &str) -> Result<()> {
    if value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(())
    } else {
        Err(Error::config("Invalid lowercase Stellar identity"))
    }
}
fn timestamp(value: &str) -> Result<u64> {
    let parsed = OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| Error::config("Invalid owner authentication timestamp"))?;
    if parsed.format(&Rfc3339).ok().as_deref() != Some(value) {
        return Err(Error::config("Noncanonical owner authentication timestamp"));
    }
    u64::try_from(parsed.unix_timestamp())
        .map_err(|_| Error::config("Invalid owner authentication timestamp"))
}
fn cookie(response: &Response, backend: &str) -> Result<Zeroizing<String>> {
    let headers: Vec<_> = response.header_values("set-cookie").collect();
    if headers.len() != 1 {
        return Err(Error::config(
            "Owner authentication requires one unambiguous session cookie",
        ));
    }
    let raw = std::str::from_utf8(headers[0])
        .map_err(|_| Error::config("Invalid owner session cookie"))?;
    if !raw.bytes().all(|b| (32..127).contains(&b)) {
        return Err(Error::config("Invalid owner session cookie"));
    }
    let mut fields = raw.split(';').map(str::trim);
    let pair = fields
        .next()
        .ok_or_else(|| Error::config("Missing owner session cookie"))?;
    let (name, value) = pair
        .split_once('=')
        .ok_or_else(|| Error::config("Invalid owner session cookie"))?;
    if !matches!(name, "allowit_session" | "__Host-allowit")
        || value.len() != 32
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(Error::config("Invalid owner session cookie"));
    }
    let attributes: Vec<_> = fields.map(str::to_ascii_lowercase).collect();
    let mut seen = std::collections::BTreeSet::new();
    if attributes
        .iter()
        .any(|a| !seen.insert(a.split('=').next().unwrap_or("")))
    {
        return Err(Error::config("Ambiguous owner session cookie attributes"));
    }
    if !attributes.iter().any(|a| a == "path=/")
        || !attributes.iter().any(|a| a == "httponly")
        || !attributes.iter().any(|a| a == "samesite=lax")
        || attributes.iter().any(|a| a.starts_with("domain="))
        || (name == "__Host-allowit"
            && (!backend.starts_with("https://") || !attributes.iter().any(|a| a == "secure")))
    {
        return Err(Error::config("Unsafe owner session cookie scope"));
    }
    Ok(Zeroizing::new(pair.to_owned()))
}
impl OwnerClient {
    pub fn new(backend: &str, origin: &str, owner: &str) -> Result<Self> {
        let owner_key = native::account_key(owner)
            .map_err(|_| Error::config("Select a canonical Stellar G owner account"))?;
        if stellar_strkey::ed25519::PublicKey(owner_key).to_string() != owner {
            return Err(Error::config("Select a canonical Stellar G owner account"));
        }
        if origin.is_empty() {
            return Err(Error::config("Set the explicit owner sign-in Origin"));
        }
        Ok(Self {
            http: HttpClient::new(Options {
                timeout: Duration::from_secs(180),
                max_response_bytes: 1_048_576,
                no_proxy: false,
                connect_timeout: None,
            })
            .map_err(|_| Error::config("Cannot initialize owner HTTP transport"))?,
            backend: parse_origin(backend)?,
            origin: parse_origin(origin)?,
            owner: owner.into(),
            session: None,
        })
    }
    fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
        session: Option<&str>,
        uncertain: bool,
    ) -> Result<Response> {
        let mut headers = vec![("Origin".into(), self.origin.clone())];
        if let Some(cookie) = session {
            headers.push(("Cookie".into(), cookie.into()));
        }
        let bytes = body
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| Error::config("Cannot encode owner request"))?;
        if bytes.is_some() {
            headers.push(("Content-Type".into(), "application/json".into()));
        }
        let response=self.http.send_success_body(Request{method,url:&format!("{}{path}",self.backend),headers:&headers,body:bytes.as_deref()},if uncertain {None} else {Some(Instant::now()+Duration::from_secs(30))})
            .map_err(|_|if uncertain {Error::uncertain("Policy creation is uncertain; inspect the owner workspace before any new generation")} else {Error::config("Owner server request is unavailable")})?;
        if !(200..300).contains(&response.status) {
            if uncertain && response.status >= 500 {
                return Err(Error::uncertain(
                    "Policy creation returned a server error; inspect the owner workspace before any new generation",
                ));
            }
            return Err(Error::api(response.status, "Owner request was refused"));
        }
        Ok(response)
    }
    fn decode<T: serde::de::DeserializeOwned>(response: &Response, uncertain: bool) -> Result<T> {
        serde_json::from_slice(&response.body).map_err(|_|if uncertain {Error::uncertain("Invalid generation response; inspect the original owner workspace before retrying")} else {Error::config("Invalid owner server response")})
    }
    pub fn authenticate(&mut self, signer: &dyn OwnerSigner, now: u64) -> Result<()> {
        if signer.account() != self.owner {
            return Err(Error::config(
                "Owner signer differs from the selected account",
            ));
        }
        self.session = None;
        let response = self.send(
            Method::Post,
            "/api/auth/challenge",
            Some(&json!({"address":self.owner})),
            None,
            false,
        )?;
        let guest = cookie(&response, &self.backend)?;
        let challenge: Challenge = Self::decode(&response, false)?;
        let expiry = timestamp(&challenge.expires_at)?;
        lower_hash(&challenge.id)?;
        let host = self.origin.split_once("://").expect("parsed origin").1;
        let prefix = format!(
            "{host} wants you to sign in with your Stellar account:\n{}\n\nSign in to AllowIt. This does not authorize spending or move funds.\n\nURI: {}\nVersion: 1\nNonce: {}\nIssued At: ",
            self.owner, self.origin, challenge.id
        );
        let (issued, expiration) = challenge
            .message
            .strip_prefix(&prefix)
            .and_then(|s| s.split_once("\nExpiration Time: "))
            .ok_or_else(|| {
                Error::config("Owner sign-in message differs from its exact account and Origin")
            })?;
        let issued = timestamp(issued)?;
        if challenge.address != self.owner
            || challenge.origin != self.origin
            || expiration != challenge.expires_at
            || expiry <= now
            || expiry.checked_sub(issued) != Some(300)
            || issued > now.saturating_add(30)
            || now.saturating_sub(issued) > 300
        {
            return Err(Error::config(
                "Owner sign-in challenge is invalid or expired",
            ));
        }
        let mut message = b"Stellar Signed Message:\n".to_vec();
        message.extend_from_slice(challenge.message.as_bytes());
        let digest = native::hash(message);
        let signature = signer
            .sign_hash(&digest)
            .map_err(|_| Error::config("Owner signing capability failed"))?;
        VerifyingKey::from_bytes(&native::account_key(&self.owner).map_err(Error::config)?)
            .map_err(|_| Error::config("Invalid owner key"))?
            .verify_strict(&digest, &Signature::from_bytes(&signature))
            .map_err(|_| Error::config("Owner signer returned an invalid signature"))?;
        let response=self.send(Method::Post,"/api/auth/verify",Some(&json!({"id":challenge.id,"address":self.owner,"message":challenge.message,"signature":STANDARD.encode(signature)})),Some(&guest),false)?;
        let authenticated: Authenticated = Self::decode(&response, false)?;
        let rotated = cookie(&response, &self.backend)?;
        let expires = timestamp(&authenticated.session.expires_at)?;
        if rotated.as_str() == guest.as_str()
            || authenticated.session.address != self.owner
            || authenticated.session.network != "stellar"
            || expires <= now
            || expires > now.saturating_add(86_490)
        {
            return Err(Error::config(
                "Owner session was not safely rotated for this account",
            ));
        }
        self.session = Some(OwnerSession {
            cookie: rotated,
            expires,
        });
        Ok(())
    }
    fn owner_request<T: serde::de::DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
        now: u64,
        uncertain: bool,
    ) -> Result<T> {
        let session = self
            .session
            .as_ref()
            .filter(|s| s.expires > now)
            .ok_or_else(|| Error::config("Authenticate the selected Stellar owner first"))?;
        Self::decode(
            &self.send(method, path, body, Some(&session.cookie), uncertain)?,
            uncertain,
        )
    }
    pub fn generate(&self, request: &GenerateRequest, now: u64) -> Result<GeneratedPolicy> {
        request.validate()?;
        let discovery: Discovery =
            self.owner_request(Method::Get, "/api/micro/runtime", None, now, false)?;
        let candidates: Vec<_> = discovery
            .wallets
            .iter()
            .filter(|w| {
                w.profile == request.profile
                    && w.capabilities.generate
                    && w.source_profiles.iter().any(|s| s == SOURCE_PROFILE)
                    && w.operations.iter().any(|s| s == OPERATION)
            })
            .collect();
        if candidates.len() != 1 || !discovery.operations.iter().any(|o| o.function == OPERATION) {
            return Err(Error::config(
                "The selected Stellar generation profile is not advertised",
            ));
        }
        let wallet = candidates[0];
        let canonical = crate::routing::normalize(&request.network)?;
        let networks: Vec<_> = wallet
            .networks
            .iter()
            .filter(|n| crate::routing::normalize(n).ok() == Some(canonical))
            .collect();
        if networks.len() != 1 {
            return Err(Error::config(
                "The selected network is not unambiguously advertised",
            ));
        }
        let network = networks[0];
        let mut selected_template = None;
        let mut selected_config = None;
        let installation = match &request.target {
            Target::NewWallet => {
                if wallet.kind != "contract-wallet" {
                    return Err(Error::usage(
                        "A new wallet requires an advertised contract-wallet template",
                    ));
                }
                let templates: Templates = self.owner_request(
                    Method::Post,
                    "/api/native/templates",
                    Some(&json!({})),
                    now,
                    false,
                )?;
                let matching: Vec<_> = templates
                    .templates
                    .into_iter()
                    .filter(|t| t.network == *network && t.profiles.contains(&request.profile))
                    .collect();
                if matching.len() != 1 {
                    return Err(Error::config(
                        "No matching new-wallet template is configured",
                    ));
                }
                let template = matching.into_iter().next().expect("one matching template");
                let asset = native::asset_from_identity(
                    &template.asset_symbol,
                    template.asset_issuer.as_deref(),
                )
                .map_err(|_| Error::config("Invalid advertised template asset"))?;
                if native::sac_address(network, &asset).map_err(Error::config)? != template.asset
                    || !(100..=10_000_000).contains(&template.max_fee_stroops)
                {
                    return Err(Error::config("Advertised template asset or fee is invalid"));
                }
                selected_template = Some(template);
                String::new()
            }
            Target::Installation(id) => {
                let all: Installations =
                    self.owner_request(Method::Get, "/api/native/policies", None, now, false)?;
                let matching: Vec<_> = all
                    .installations
                    .iter()
                    .filter(|i| {
                        i.policy_id == *id
                            && i.profile == request.profile
                            && i.config.network == *network
                            && i.config.owner == self.owner
                            && native::hex(&i.config.policy_instance) == *id
                    })
                    .collect();
                if matching.len() != 1 {
                    return Err(Error::config(
                        "The selected installation does not belong to this owner, profile and network",
                    ));
                }
                let config = matching[0].config.clone();
                let expected_kind = match config.route {
                    native::WalletRoute::Native => "owner-wallet",
                    native::WalletRoute::PolicyWallet => "contract-wallet",
                    _ => {
                        return Err(Error::config(
                            "Retired installations cannot generate new policy",
                        ));
                    }
                };
                if expected_kind != wallet.kind
                    || config.language != native::PolicyLanguage::Portable
                {
                    return Err(Error::config(
                        "The installation has a different custody profile",
                    ));
                }
                native::validate_config(&config).map_err(|_| {
                    Error::config("The selected installation has invalid portable configuration")
                })?;
                selected_config = Some(config);
                id.clone()
            }
        };
        let result:Generated=self.owner_request(Method::Post,"/api/micro/generate",Some(&json!({"prompt":request.prompt,"network":network,"runtime":{"sourceProfile":SOURCE_PROFILE,"wallet":{"kind":wallet.kind,"profile":request.profile}},"operation":{"function":OPERATION},"stellarInstallationId":installation})),now,true)?;
        if result.policies.len() > 30 || result.selected_id.is_empty() {
            return Err(Error::uncertain(
                "Generation returned an invalid workspace identity",
            ));
        }
        #[derive(Deserialize)]
        struct Identity {
            id: String,
        }
        let mut selected = None;
        for raw in result.policies {
            let id: Identity = serde_json::from_str(raw.get())
                .map_err(|_| Error::uncertain("Generation returned an invalid policy identity"))?;
            if id.id == result.selected_id {
                if selected.is_some() {
                    return Err(Error::uncertain(
                        "Generation returned a duplicate selected identity",
                    ));
                }
                selected = Some(serde_json::from_str::<GeneratedPolicy>(raw.get()).map_err(
                    |_| Error::uncertain("Generation returned invalid portable policy fields"),
                )?);
            }
        }
        let policy =
            selected.ok_or_else(|| Error::uncertain("Generation omitted the selected policy"))?;
        validate_generated(&policy, request, network, &wallet.kind).map_err(|_|Error::uncertain("Generated policy failed portable identity validation; inspect the owner workspace before retrying"))?;
        let binding = &policy.runtime.instance_binding;
        if let Some(template) = selected_template
            && (binding.asset != template.asset
                || binding.asset_symbol != template.asset_symbol
                || binding.asset_issuer != template.asset_issuer
                || binding.max_fee_stroops != template.max_fee_stroops)
        {
            return Err(Error::uncertain(
                "Generation changed the selected template asset or fee",
            ));
        }
        if let Some(config) = selected_config
            && (binding.wallet != config.wallet
                || binding.route
                    != serde_json::to_value(&config.route)
                        .map_err(|_| Error::config("Cannot encode native custody route"))?
                        .as_str()
                        .unwrap_or("")
                || binding.asset != config.asset
                || binding.asset_symbol != config.asset_symbol
                || binding.asset_issuer != config.asset_issuer
                || binding.max_fee_stroops != config.max_fee_stroops
                || (config.route == native::WalletRoute::PolicyWallet
                    && (binding.module_digest != native::hex(&config.module_digest)
                        || binding.tier != config.tier
                        || policy.code != config.policy_source
                        || binding.portable.params != config.params)))
        {
            return Err(Error::uncertain(
                "Generation changed the explicitly selected installation",
            ));
        }
        Ok(policy)
    }
}
fn validate_generated(
    policy: &GeneratedPolicy,
    request: &GenerateRequest,
    network: &str,
    kind: &str,
) -> Result<()> {
    let b = &policy.runtime.instance_binding;
    let d = &b.portable;
    let shape = crate::authoring::check(&policy.code).map_err(|_| {
        Error::uncertain("Returned policy source is outside the portable vocabulary")
    })?;
    let params = serde_json_canonicalizer::to_vec(&d.params)
        .map_err(|_| Error::uncertain("Invalid returned policy params"))?;
    if !crate::request::request_id_valid(&policy.id)
        || policy.revision != 1
        || policy.network != network
        || policy.language != LANGUAGE
        || policy.original_intent != request.prompt
        || !policy.ir_hash.is_empty()
        || policy.status != "draft"
        || policy.source_hash != allowit_sdk::digest(policy.code.as_bytes())
        || policy.runtime.source_profile != SOURCE_PROFILE
        || policy.runtime.wallet.kind != kind
        || policy.runtime.wallet.profile != request.profile
        || policy.runtime.operation != OPERATION
        || b.profile != native::PROFILE
        || b.language != LANGUAGE
        || b.asset != policy.mint
        || b.asset_symbol != policy.token
        || d.language != LANGUAGE
        || d.source_hash != policy.source_hash
        || d.params_hash != allowit_sdk::digest(&params)
        || d.asset.address != policy.mint
        || d.asset.symbol != policy.token
        || d.asset.decimals != 7
        || d.build.prefix != shape.prefix
        || d.build.writes_state != shape.writes_state
        || d.build.attempts == 0
        || d.build.attempts > 3
    {
        return Err(Error::uncertain(
            "Returned policy changed the exact owner intent or portable binding",
        ));
    }
    lower_hash(&b.policy_instance)?;
    lower_hash(&d.params_hash)?;
    if d.build.library_revision.len() != 40
        || !d
            .build
            .library_revision
            .bytes()
            .all(|v| v.is_ascii_hexdigit())
    {
        return Err(Error::uncertain("Invalid reported policy library revision"));
    }
    let tier = match d.tier.as_str() {
        "standard" => PolicyTier::Common,
        "custom" => PolicyTier::Custom,
        _ => return Err(Error::uncertain("Invalid returned portable tier")),
    };
    if (d.tier == "standard" && (!shape.canonical_only || d.rules != shape.prefix))
        || (d.tier == "custom" && !d.rules.is_empty())
    {
        return Err(Error::uncertain(
            "Returned rules differ from the portable source tier",
        ));
    }
    d.params
        .check(tier)
        .map_err(|_| Error::uncertain("Invalid returned typed params"))?;
    native::policy::parse_rules(&d.rules)
        .map_err(|_| Error::uncertain("Invalid returned canonical rules"))?;
    let asset = native::asset_from_identity(&b.asset_symbol, b.asset_issuer.as_deref())
        .map_err(Error::config)?;
    if native::sac_address(network, &asset).map_err(Error::config)? != b.asset
        || !(100..=if kind == "contract-wallet" {
            10_000_000
        } else {
            1_000_000
        })
            .contains(&b.max_fee_stroops)
    {
        return Err(Error::uncertain(
            "Returned policy asset or fee binding differs",
        ));
    }
    if kind == "owner-wallet" {
        let identity = json!({"language":d.language,"tier":d.tier,"rules":d.rules,"sourceHash":d.source_hash,"paramsHash":d.params_hash});
        let identity = serde_json_canonicalizer::to_vec(&identity)
            .map_err(|_| Error::uncertain("Cannot encode returned native policy identity"))?;
        if b.route != "native"
            || b.tier.is_some()
            || !shape.canonical_only
            || b.module_digest != allowit_sdk::digest(&identity)
        {
            return Err(Error::uncertain("Returned native policy identity differs"));
        }
    } else {
        let tier = b
            .tier
            .ok_or_else(|| Error::uncertain("Missing returned contract policy tier"))?;
        let profile_fits = match tier {
            PolicyTier::Custom => request.profile == "allowit-stellar-custom-wallet-v1",
            _ => request.profile == "allowit-stellar-policy-wallet-v1" && d.tier == "standard",
        };
        if b.route != "policyWallet"
            || !profile_fits
            || b.deployment_id != b.policy_instance
            || (tier.standard() && shape.writes_state)
        {
            return Err(Error::uncertain("Returned contract policy tier differs"));
        }
    }
    match &request.target {
        Target::NewWallet
            if b.template == Some(true)
                && b.route == "policyWallet"
                && b.wallet.is_empty()
                && b.module_digest.is_empty()
                && b.tier.is_some() => {}
        Target::Installation(id)
            if b.template != Some(true) && b.deployment_id == *id && !b.wallet.is_empty() =>
        {
            lower_hash(&b.module_digest)?;
            native::address(&b.wallet).map_err(Error::config)?;
        }
        _ => {
            return Err(Error::uncertain(
                "Generation changed existing-versus-new wallet selection",
            ));
        }
    }
    Ok(())
}
