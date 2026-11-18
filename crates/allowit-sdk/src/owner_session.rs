//! The owner's session with the backend for the owner routes that the SDK sends itself, such as a
//! reputation vote or the owner's view of a Custom build. The SDK runs the sign-in: Core's session
//! client reads the challenge and keeps the rotated cookie, the SDK checks the exact sign-in
//! message, has the rail sign it as a wallet of the rail signs a text message, and verifies that
//! signature before it is sent. The rail only signs.
pub use allowit_core::vendor::owner_session::{Challenge, Client};

use crate::error::{Error, Result};
use allowit_core::{
    transport::http::Method,
    vendor::{owner_session::timestamp, wallet::MessageSignature, wallet::Signer},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;

/// The session network that a wallet sign-in opens.
const WALLET_SESSION: &str = "wallet";
/// How long a sign-in message is valid, in seconds, from its issue time.
const MESSAGE_LIFETIME: u64 = 300;
/// How far ahead of this clock a sign-in message may be issued, in seconds.
const CLOCK_SKEW: u64 = 30;

/// One signed-in owner at one backend.
pub struct Session(Client);

impl Session {
    /// Sign the owner of `signer` in at `backend` from `origin`. `signature` is the text-message
    /// signature of the rail of `network`. The SDK checks the exact sign-in message before it asks
    /// for a signature, and the signature before Core's client sends it.
    pub fn sign_in(
        backend: &str,
        origin: &str,
        network: &str,
        signer: &dyn Signer,
        signature: &dyn MessageSignature,
        now: u64,
    ) -> Result<Self> {
        let owner = signer.address();
        let mut client = Client::new(backend, origin, owner)?;
        let origin = client.origin().to_owned();
        client.authenticate(WALLET_SESSION, now, |challenge| {
            check_message(challenge, owner, &origin, now)?;
            let signed = signature.sign_message(signer, &challenge.message)?;
            signature
                .verify_message(network, owner, &challenge.message, &signed)
                .map_err(|_| Error::config("Owner signer returned an invalid signature"))?;
            Ok(STANDARD.encode(signed))
        })?;
        Ok(Self(client))
    }

    /// Post `body` to the owner route `path` under this session. `path` must be an API route of
    /// the backend ([`crate::owner::check_route`]): the session cookie never goes to another
    /// origin or route. The reply is the server's JSON.
    pub fn post(&self, path: &str, body: &Value, now: u64) -> Result<Value> {
        crate::owner::check_route(path)?;
        self.0
            .owner_request(Method::Post, path, Some(body), now, false)
    }
}

/// The message of `challenge` is the exact sign-in message for `owner` at `origin` with the
/// challenge's nonce, issued now and valid for five minutes until the challenge expires. Any other
/// text is refused, so the owner signs nothing but this sign-in.
fn check_message(challenge: &Challenge, owner: &str, origin: &str, now: u64) -> Result<()> {
    let invalid = || Error::config("Owner sign-in challenge is invalid or expired");
    let (issued, expiration) = message_times(&challenge.message, owner, origin, &challenge.id)?;
    let issued = timestamp(issued)?;
    let expiry = timestamp(&challenge.expires_at)?;
    if expiration != challenge.expires_at
        || expiry.checked_sub(issued) != Some(MESSAGE_LIFETIME)
        || issued > now.saturating_add(CLOCK_SKEW)
        || now.saturating_sub(issued) > MESSAGE_LIFETIME
    {
        return Err(invalid());
    }
    Ok(())
}

/// The issue and expiration times of the exact sign-in message for `owner` at `origin` with
/// `nonce`.
fn message_times<'a>(
    message: &'a str,
    owner: &str,
    origin: &str,
    nonce: &str,
) -> Result<(&'a str, &'a str)> {
    let host = origin
        .split_once("://")
        .map(|(_, host)| host)
        .ok_or_else(|| Error::config("Invalid sign-in Origin"))?;
    let prefix = format!(
        "{host} wants you to sign in with your wallet account:\n{owner}\n\nSign in to AllowIt. This does not authorize spending or move funds.\n\nURI: {origin}\nVersion: 1\nNonce: {nonce}\nIssued At: "
    );
    message
        .strip_prefix(&prefix)
        .and_then(|rest| rest.split_once("\nExpiration Time: "))
        .filter(|(issued, expiration)| !issued.contains('\n') && !expiration.contains('\n'))
        .ok_or_else(|| {
            Error::config("Owner sign-in message differs from its exact account and Origin")
        })
}

#[cfg(test)]
mod tests;
