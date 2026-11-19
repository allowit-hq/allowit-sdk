//! The custom wallet build for hosts: the rail's build hook and the client of a server's build
//! routes. A Custom wallet runs code that is built from the exact policy source; the owner's
//! policy signature binds that source. The owner's CLI builds the code and hands it to the
//! server, which uploads it when the owner deploys. The owner's one creation signature names the
//! code by its hash, so it covers the build. The rail of each network supplies the build and the
//! code hash. No function here names a vendor.
use crate::{
    error::{Error, Result},
    plugins::for_network,
    routing::normalize,
    server::{component, send},
    transport::http::Method,
};
use allowit_core::vendor::custom_build::{CustomWalletBuild, refusal};
pub use allowit_core::vendor::custom_build::{WalletCode, check_code_hash, code, hex};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The route of a server that reads the admitted source of a policy, relative to its origin.
pub const BUILD_PATH: &str = "/api/custom/build";
/// The route that hands the built code to the server.
pub const CODE_PATH: &str = "/api/custom/build/code";
/// The largest wallet code that a host sends or stores.
pub const MAX_CODE_BYTES: usize = 131_072;
/// The largest policy source of a build reply.
pub const MAX_SOURCE_BYTES: usize = 262_144;
const MAX_REPLY_BYTES: usize = 1 << 20;

fn hook(network: &str) -> Result<(&'static str, &'static dyn CustomWalletBuild)> {
    let network = normalize(network)?;
    for_network(network)?
        .custom_wallet_build()
        .map(|hook| (network, hook))
        .ok_or_else(|| Error::unsupported("The rail of this network builds no custom wallet"))
}

/// True when the rail of `network` implements the build hook.
pub fn supported(network: &str) -> bool {
    hook(network).is_ok()
}

/// Build the wallet code of a Custom wallet from the exact bytes of `source`. `work` is a
/// folder that the caller owns. A missing tool is a [`code::TOOLCHAIN_MISSING`] refusal.
pub fn build(network: &str, source: &str, work: &Path) -> Result<WalletCode> {
    let (network, hook) = hook(network)?;
    hook.build(network, source, work)
}

/// Refuse a source that the Custom wallet of `network` cannot hold, without building it: the
/// rail's own limits beyond Core's derivation, such as its storage of the state keys that the
/// contract keeps. The rail's build refuses the same sources. A client calls it before it reports
/// a source as fit for a Custom wallet.
pub fn check(network: &str, source: &str) -> Result<()> {
    let (network, hook) = hook(network)?;
    hook.check(network, source)
}

/// The identity of wallet code as the network records it.
pub fn code_hash(network: &str, code: &[u8]) -> Result<[u8; 32]> {
    let (network, hook) = hook(network)?;
    hook.code_hash(network, code)
}

/// The build that the server holds for a policy: the code that a build handed over. A reply may
/// carry more fields: a later server can add facts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildState {
    /// The identity of the built wallet code, as lowercase hexadecimal text.
    pub code_hash: String,
}

/// A policy that awaits its build: the source that the server admitted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Admission {
    pub policy_id: String,
    /// The CAIP-2 network of the policy.
    pub network: String,
    /// The exact policy source that the server admitted.
    pub source: String,
    #[serde(default)]
    pub build: Option<BuildState>,
}
impl Admission {
    /// Refuse a source other than the one that the owner reviewed and signed. `source_hash` is
    /// the SHA-256 of that source, as lowercase hexadecimal text, from the owner's own record:
    /// the policy file or the build command that the owner's app shows.
    pub fn check_source(&self, source_hash: &str) -> Result<()> {
        if crate::digest(self.source.as_bytes()) == source_hash {
            return Ok(());
        }
        Err(Error::config(
            "The server's policy source differs from the source that the owner signed. Nothing was built.",
        ))
    }
}

fn token(name: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 || !value.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Error::usage(format!("The {name} is invalid")));
    }
    Ok(())
}
fn reply<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T> {
    serde_json::from_slice(body)
        .map_err(|_| Error::uncertain("The server returned an invalid custom build reply"))
}
const REFUSED: &str = "the custom build request was refused";

/// Read the admitted source of a policy with its build ticket. Check it with
/// [`Admission::check_source`] before building.
pub fn fetch(server: &str, policy_id: &str, ticket: &str) -> Result<Admission> {
    token("policy id", policy_id)?;
    token("build ticket", ticket)?;
    let body = send(
        server,
        Method::Get,
        &format!(
            "{BUILD_PATH}?id={}&ticket={}",
            component(policy_id),
            component(ticket)
        ),
        None,
        MAX_REPLY_BYTES,
        REFUSED,
    )?;
    let admission: Admission = reply(&body)?;
    if admission.policy_id != policy_id {
        return Err(Error::uncertain("The server returned another policy"));
    }
    if admission.source.len() > MAX_SOURCE_BYTES {
        return Err(Error::uncertain(
            "The server returned a policy source over the build limit",
        ));
    }
    Ok(admission)
}

/// The owner's view of a build on the server: the build ticket and the hash of the source that
/// the owner's policy signature binds.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerView {
    pub policy_id: String,
    pub network: String,
    /// The capability of the build routes of this policy.
    pub ticket: String,
    /// The SHA-256 of the source that the owner signed, as lowercase hexadecimal text.
    pub source_hash: String,
    #[serde(default)]
    pub build: Option<BuildState>,
}

/// Read the build ticket of a Custom policy with the owner's session. The rail signs the owner
/// in; the SDK sends the request.
pub fn owner_view(cx: &crate::owner::Context<'_>, policy_id: &str) -> Result<OwnerView> {
    token("policy id", policy_id)?;
    let value = crate::owner::call(
        cx,
        "/api/native/custom/build",
        &serde_json::json!({ "policyId": policy_id }),
    )?;
    let view: OwnerView = serde_json::from_value(value)
        .map_err(|_| Error::uncertain("The server returned an invalid custom build reply"))?;
    if view.policy_id != policy_id {
        return Err(Error::uncertain("The server returned another policy"));
    }
    Ok(view)
}

/// Hand built wallet code to the server, which uploads it when the owner deploys. The reply
/// names the hash that the server computed, which must be the hash of `built`.
pub fn submit(
    server: &str,
    policy_id: &str,
    ticket: &str,
    built: &WalletCode,
) -> Result<BuildState> {
    token("policy id", policy_id)?;
    token("build ticket", ticket)?;
    if built.code.is_empty() || built.code.len() > MAX_CODE_BYTES {
        return Err(Error::usage("The wallet code exceeds the build limit"));
    }
    let request = serde_json::json!({
        "policyId": policy_id,
        "ticket": ticket,
        "code": STANDARD.encode(&built.code),
    });
    let body = send(
        server,
        Method::Post,
        CODE_PATH,
        Some(request.to_string().as_bytes()),
        MAX_REPLY_BYTES,
        REFUSED,
    )?;
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Submitted {
        policy_id: String,
        code_hash: String,
    }
    let submitted: Submitted = reply(&body)?;
    if submitted.policy_id != policy_id || submitted.code_hash != hex(&built.code_hash) {
        return Err(refusal(
            code::CODE_HASH_MISMATCH,
            "The server reports another code hash than this build. Build the policy again.",
        ));
    }
    Ok(BuildState {
        code_hash: submitted.code_hash,
    })
}
