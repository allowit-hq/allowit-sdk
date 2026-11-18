//! Requests to the public routes of one configured server: no session and no credential.
use crate::{
    error::{Error, Result},
    transport::{
        http::{HttpClient, Method, Request},
        rpc::Options,
    },
};
use serde::Deserialize;
use std::time::Duration;

/// Percent-encode one URL query value. Only unreserved characters stay as they are.
pub(crate) fn component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// A typed server refusal `{error, code}`.
#[derive(Deserialize)]
struct Refusal {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    code: Option<String>,
}

/// Send one request to `path` of `server` and return the body of a success reply. A refusal
/// keeps the server's typed code. `refused` is the text of a refusal that carries none.
pub(crate) fn send(
    server: &str,
    method: Method,
    path: &str,
    body: Option<&[u8]>,
    max_reply_bytes: usize,
    refused: &str,
) -> Result<Vec<u8>> {
    send_accepting(
        server,
        method,
        path,
        body,
        max_reply_bytes,
        refused,
        "application/json",
    )
}

/// [`send`] with the `accept` header `accept`, which can name the shape of the reply.
pub(crate) fn send_accepting(
    server: &str,
    method: Method,
    path: &str,
    body: Option<&[u8]>,
    max_reply_bytes: usize,
    refused: &str,
    accept: &str,
) -> Result<Vec<u8>> {
    let origin = crate::config::parse_origin(server)?;
    let url = format!("{origin}{path}");
    let client = HttpClient::new(Options {
        timeout: Duration::from_secs(if body.is_some() { 60 } else { 20 }),
        max_response_bytes: max_reply_bytes,
        no_proxy: false,
        connect_timeout: Some(Duration::from_secs(10)),
    })
    .map_err(|_| Error::config("Cannot configure the server request"))?;
    let mut headers = vec![("accept".to_owned(), accept.to_owned())];
    if body.is_some() {
        headers.push(("content-type".into(), "application/json".into()));
    }
    let response = client
        .send_refusal_body(
            Request {
                method,
                url: &url,
                headers: &headers,
                body,
            },
            None,
        )
        .map_err(|_| crate::transport_failure::network_unavailable("Allowit server"))?;
    if !(200..300).contains(&response.status) {
        let refusal: Option<Refusal> = serde_json::from_slice(&response.body).ok();
        let (message, code) = refusal
            .map(|refusal| (refusal.error, refusal.code))
            .unwrap_or_default();
        // Server text is data: one bounded line with no control character.
        let message: String = message
            .unwrap_or_else(|| refused.into())
            .chars()
            .filter(|c| !c.is_control())
            .take(300)
            .collect();
        return Err(
            match code.filter(|code| {
                !code.is_empty()
                    && code.len() <= 64
                    && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            }) {
                Some(code) => Error::refused(response.status, code, message),
                None => Error::api(response.status, message),
            },
        );
    }
    Ok(response.body)
}
