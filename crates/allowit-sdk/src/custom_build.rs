//! The custom wallet build for hosts: the rail's build hook, the two signed build statements and
//! the client of a server's build routes. Core defines the statements; the rail of each network
//! supplies the build, the code hash and the signature scheme. No function here names a vendor.
use crate::{
    error::{Error, Result},
    plugins::for_network,
    routing::normalize,
    server::{component, send},
    transport::http::Method,
};
use allowit_core::vendor::custom_build::{self as core, CustomWalletBuild};
pub use allowit_core::vendor::custom_build::{
    ATTESTATION_TITLE, Admitted, Attestation, PRESIGNATURE_TITLE, Presignature, WalletCode,
    attestation_message, check_code_hash, code, hex, signature_bytes,
};
pub use allowit_core::vendor::wallet::Signer;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The route of a server that reads the admitted source of a policy with its presignature,
/// relative to its origin.
pub const BUILD_PATH: &str = "/api/custom/build";
/// The route that hands the built code to the server.
pub const CODE_PATH: &str = "/api/custom/build/code";
/// The route that registers the owner's attestation of a build.
pub const ATTEST_PATH: &str = "/api/custom/build/attest";
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

/// The identity of wallet code as the network records it.
pub fn code_hash(network: &str, code: &[u8]) -> Result<[u8; 32]> {
    let (network, hook) = hook(network)?;
    hook.code_hash(network, code)
}

/// Sign the server presignature of an admitted policy source. `signer` is the server key.
pub fn presign(signer: &dyn Signer, policy_id: &str, source: &str) -> Result<Presignature> {
    let (network, hook) = hook(signer.network())?;
    core::presign(hook, signer, network, policy_id, source)
}

/// Verify a presignature against the caller's own network, policy id, source and trusted server
/// key. See [`allowit_core::vendor::custom_build::verify_presignature`].
pub fn verify_presignature(presignature: &Presignature, admitted: &Admitted<'_>) -> Result<()> {
    let (network, hook) = hook(admitted.network)?;
    core::verify_presignature(
        hook,
        presignature,
        &Admitted {
            network,
            ..*admitted
        },
    )
}

/// Sign the owner attestation of a build. `signer` is the owner key. Verify the presignature
/// first.
pub fn attest(
    signer: &dyn Signer,
    presignature: &Presignature,
    code_hash: &[u8; 32],
) -> Result<Attestation> {
    let (network, hook) = hook(signer.network())?;
    if presignature.network != network {
        return Err(core::refusal(
            code::PRESIGNATURE_MISMATCH,
            "The server presignature is for another network than the owner key. Select the rail of the policy.",
        ));
    }
    core::attest(hook, signer, presignature, code_hash)
}

/// Verify an attestation against the caller's own network, policy id, source, trusted server
/// key and owner.
pub fn verify_attestation(
    attestation: &Attestation,
    admitted: &Admitted<'_>,
    owner: &str,
) -> Result<()> {
    let (network, hook) = hook(admitted.network)?;
    core::verify_attestation(
        hook,
        attestation,
        &Admitted {
            network,
            ..*admitted
        },
        owner,
    )
}

/// The build that a server holds for a policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildState {
    /// The identity of the built wallet code.
    pub code_hash: String,
    /// Whether the owner attested this code.
    pub attested: bool,
}

/// A policy that awaits its build: the admitted source with the server presignature.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Admission {
    pub policy_id: String,
    /// The CAIP-2 network of the policy.
    pub network: String,
    /// The owner of the policy: the account that attests the build.
    pub owner: String,
    /// The exact policy source that the server admitted.
    pub source: String,
    pub presignature: Presignature,
    #[serde(default)]
    pub build: Option<BuildState>,
}

/// The server's answer to built code: its hash and the attestation text for the owner to sign.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Submitted {
    pub policy_id: String,
    pub code_hash: String,
    pub attested: bool,
    /// The exact attestation message of this build.
    pub attestation_message: String,
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

/// Read the admitted source of a policy and verify its presignature against `server_key`, the
/// server key that the caller trusts. The reply is returned only when that key signed exactly
/// this policy id, the reply's network and the reply's source.
pub fn fetch(server: &str, policy_id: &str, ticket: &str, server_key: &str) -> Result<Admission> {
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
    if admission.source.len() > MAX_SOURCE_BYTES {
        return Err(Error::uncertain(
            "The server returned a policy source over the build limit",
        ));
    }
    verify_presignature(
        &admission.presignature,
        &Admitted {
            network: &admission.network,
            policy_id,
            source: &admission.source,
            server: server_key,
        },
    )?;
    Ok(admission)
}

/// Hand built wallet code to the server. The server keeps it for the owner's code upload; it
/// does not accept or vouch for the build. The reply names the hash that the server computed,
/// which must be the hash of `built`.
pub fn submit(
    server: &str,
    policy_id: &str,
    ticket: &str,
    built: &WalletCode,
) -> Result<Submitted> {
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
    let submitted: Submitted = reply(&body)?;
    if submitted.policy_id != policy_id || submitted.code_hash != hex(&built.code_hash) {
        return Err(core::refusal(
            code::CODE_HASH_MISMATCH,
            "The server reports another code hash than this build. Build the policy again.",
        ));
    }
    Ok(submitted)
}

/// Register the owner's attestation of a build with the server.
pub fn register(
    server: &str,
    policy_id: &str,
    ticket: &str,
    attestation: &Attestation,
) -> Result<BuildState> {
    token("policy id", policy_id)?;
    token("build ticket", ticket)?;
    let request = serde_json::json!({
        "policyId": policy_id,
        "ticket": ticket,
        "attestation": attestation,
    });
    let body = send(
        server,
        Method::Post,
        ATTEST_PATH,
        Some(request.to_string().as_bytes()),
        MAX_REPLY_BYTES,
        REFUSED,
    )?;
    let state: BuildState = reply(&body)?;
    if !state.attested || state.code_hash != attestation.code_hash {
        return Err(Error::uncertain(
            "The server did not register this build attestation",
        ));
    }
    Ok(state)
}
