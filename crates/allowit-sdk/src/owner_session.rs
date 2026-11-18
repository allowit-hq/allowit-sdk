//! The owner's session with the backend for the owner routes that the SDK sends itself, such as a
//! reputation vote or the owner's view of a Custom build. The SDK runs the sign-in: it reads the
//! challenge, checks the exact sign-in message, has the rail sign it as a wallet of the rail signs
//! a text message, verifies that signature, and keeps the rotated session cookie. The rail only
//! signs.
pub use allowit_core::vendor::owner_session::{Challenge, Client};

use crate::error::{Error, Result};
use allowit_core::{
    client::config::parse_origin,
    transport::{
        http::{HttpClient, Method, Request, Response},
        rpc::Options,
    },
    vendor::{owner_session::timestamp, wallet::MessageSignature, wallet::Signer},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

/// The session network that a wallet sign-in opens.
const WALLET_SESSION: &str = "wallet";
/// How long a sign-in message is valid, in seconds, from its issue time.
const MESSAGE_LIFETIME: u64 = 300;
/// How far ahead of this clock a sign-in message may be issued, in seconds.
const CLOCK_SKEW: u64 = 30;
/// The longest owner session that the server may open, in seconds.
const MAX_SESSION: u64 = 86_490;
/// How long one owner request may take.
const REQUEST_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Deserialize)]
struct Authenticated {
    session: SessionInfo,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionInfo {
    address: String,
    network: String,
    expires_at: String,
}

/// One signed-in owner at one backend.
pub struct Session {
    http: HttpClient,
    backend: String,
    origin: String,
    cookie: Zeroizing<String>,
    expires: u64,
}

impl Session {
    /// Sign the owner of `signer` in at `backend` from `origin`. `signature` is the text-message
    /// signature of the rail of `network`; the SDK checks the challenge and the message before it
    /// asks for a signature, and the signature before it sends it.
    pub fn sign_in(
        backend: &str,
        origin: &str,
        network: &str,
        signer: &dyn Signer,
        signature: &dyn MessageSignature,
        now: u64,
    ) -> Result<Self> {
        // An empty origin would read as the default app origin, which the caller never named.
        if origin.is_empty() {
            return Err(Error::config("Set the explicit owner sign-in Origin"));
        }
        let backend = parse_origin(backend)?;
        let origin = parse_origin(origin)?;
        let owner = signer.address().to_owned();
        let http = HttpClient::new(Options {
            timeout: Duration::from_secs(60),
            max_response_bytes: 1_048_576,
            no_proxy: false,
            connect_timeout: None,
        })
        .map_err(|_| Error::config("Cannot initialize owner HTTP transport"))?;
        let response = send(
            &http,
            &backend,
            &origin,
            Method::Post,
            "/api/auth/challenge",
            Some(&json!({"address": owner})),
            None,
        )?;
        let guest = cookie(&response, &backend)?;
        let challenge: Challenge = decode(&response)?;
        check_challenge(&challenge, &owner, &origin, now)?;
        let signed = signature.sign_message(signer, &challenge.message)?;
        signature
            .verify_message(network, &owner, &challenge.message, &signed)
            .map_err(|_| Error::config("Owner signer returned an invalid signature"))?;
        let response = send(
            &http,
            &backend,
            &origin,
            Method::Post,
            "/api/auth/verify",
            Some(&json!({"id": challenge.id, "address": owner,
                "message": challenge.message, "signature": STANDARD.encode(signed)})),
            Some(&guest),
        )?;
        let rotated = cookie(&response, &backend)?;
        let authenticated: Authenticated = decode(&response)?;
        let expires = timestamp(&authenticated.session.expires_at)?;
        if rotated.as_str() == guest.as_str()
            || authenticated.session.address != owner
            || authenticated.session.network != WALLET_SESSION
            || expires <= now
            || expires > now.saturating_add(MAX_SESSION)
        {
            return Err(Error::config(
                "Owner session was not safely rotated for this account",
            ));
        }
        Ok(Self {
            http,
            backend,
            origin,
            cookie: rotated,
            expires,
        })
    }

    /// Post `body` to the owner route `path` under this session. `path` must be an API route of
    /// the backend ([`crate::owner::check_route`]): the session cookie never goes to another
    /// origin or route. The reply is the server's JSON.
    pub fn post(&self, path: &str, body: &Value, now: u64) -> Result<Value> {
        crate::owner::check_route(path)?;
        if self.expires <= now {
            return Err(Error::config("The owner session expired; sign in again"));
        }
        decode(&send(
            &self.http,
            &self.backend,
            &self.origin,
            Method::Post,
            path,
            Some(body),
            Some(&self.cookie),
        )?)
    }
}

/// The challenge names this owner and origin, has a 64-character hex id and has not expired; its
/// message is the exact sign-in message for them, issued now and valid for five minutes.
fn check_challenge(challenge: &Challenge, owner: &str, origin: &str, now: u64) -> Result<()> {
    let invalid = || Error::config("Owner sign-in challenge is invalid or expired");
    let hex = challenge.id.len() == 64
        && challenge
            .id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    let expiry = timestamp(&challenge.expires_at)?;
    if !hex || challenge.address != owner || challenge.origin != origin || expiry <= now {
        return Err(invalid());
    }
    let (issued, expiration) = message_times(&challenge.message, owner, origin, &challenge.id)?;
    let issued = timestamp(issued)?;
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
/// `nonce`. Any other text is refused, so the owner signs nothing but this sign-in.
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

fn send(
    http: &HttpClient,
    backend: &str,
    origin: &str,
    method: Method,
    path: &str,
    body: Option<&Value>,
    session: Option<&str>,
) -> Result<Response> {
    let mut headers = vec![("Origin".to_owned(), origin.to_owned())];
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
    let response = http
        .send_refusal_body(
            Request {
                method,
                url: &format!("{backend}{path}"),
                headers: &headers,
                body: bytes.as_deref(),
            },
            Some(Instant::now() + REQUEST_DEADLINE),
        )
        .map_err(|_| Error::config("Owner server request is unavailable"))?;
    if !(200..300).contains(&response.status) {
        return Err(refusal(response.status, &response.body));
    }
    Ok(response)
}

fn decode<T: serde::de::DeserializeOwned>(response: &Response) -> Result<T> {
    serde_json::from_slice(&response.body)
        .map_err(|_| Error::config("Invalid owner server response"))
}

/// The one session cookie of `response`: `allowit_session` or `__Host-allowit` with a 32-character
/// value, scoped to the backend's root path, `HttpOnly` and `SameSite=Lax`, with no domain.
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

/// The error for a non-2xx owner reply. A typed body `{error, code, details}` keeps its `code`,
/// its bounded `error` text and a bounded `details` object; any other reply is a plain refusal.
fn refusal(status: u16, body: &[u8]) -> Error {
    let reply: Option<Value> = serde_json::from_slice(body).ok();
    let text = |value: &Value, limit: usize| {
        value
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= limit && !v.chars().any(char::is_control))
            .map(String::from)
    };
    let code = reply
        .as_ref()
        .and_then(|r| text(&r["code"], 64))
        .filter(|c| {
            c.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        });
    let message = reply
        .as_ref()
        .and_then(|r| text(&r["error"], 300))
        .unwrap_or_else(|| "Owner request was refused".into());
    match code {
        Some(code) => {
            let error = Error::refused(status, code, message);
            match reply.and_then(|mut r| r.get_mut("details").map(Value::take)) {
                Some(details)
                    if details.is_object()
                        && serde_json::to_vec(&details).is_ok_and(|bytes| bytes.len() <= 4096) =>
                {
                    error.with_details(details)
                }
                _ => error,
            }
        }
        None => Error::api(status, "Owner request was refused"),
    }
}

#[cfg(test)]
mod tests;
