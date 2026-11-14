//! Paid calls: a seller's payment challenge, the bounded seller request and the proof of the
//! settled payment. The rail of the selected network reads the challenge and builds the proof.
//! The payment itself is an ordinary operation: [`crate::execution::execute`] with
//! `Request::paid_service` set to [`Challenge::context`]. Nothing here pays.
use crate::{
    error::{Error, Result},
    plugins::for_network,
    transport::{
        http::{HttpClient, Method, Request},
        rpc::{ErrorKind, Options},
    },
};
pub use allowit_core::vendor::paid_service::{
    Challenge, MAX_CHALLENGE_BYTES, MAX_CHALLENGE_ID_BYTES, MAX_RESOURCE_BYTES, PaidService,
    PaidServiceProvider, Proof, SellerReply, challenge_id, resource_url,
};
use std::time::Duration;

/// The error code (and CLI exit code) of a seller that refused, failed or sent a reply that is
/// no usable challenge or resource. It differs from every payment and policy code.
pub const SELLER_FAILED: i32 = 8;
/// The stable name of [`SELLER_FAILED`] in JSON errors.
pub const SELLER_FAILED_NAME: &str = "seller_failed";
/// The key of the paid-service context in an outcome document.
pub const KEY: &str = "paidService";
/// The largest resource body that [`send`] reads.
pub const MAX_BODY_BYTES: usize = 8 << 20;
/// The longest that one seller request may take.
pub const SELLER_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_HEADERS: usize = 32;

/// A seller error with [`SELLER_FAILED`].
pub fn seller_failed(message: impl Into<String>) -> Error {
    let mut error = Error::unsupported(message);
    error.code = SELLER_FAILED;
    error
}

fn hook(network: &str) -> Result<(&'static str, &'static dyn PaidServiceProvider)> {
    let network = crate::routing::normalize(network)?;
    for_network(network)?
        .paid_service()
        .map(|hook| (network, hook))
        .ok_or_else(|| Error::unsupported(format!("Paid calls are not supported on {network}")))
}

/// Whether the rail of `network` reads seller challenges.
pub fn supported(network: &str) -> bool {
    hook(network).is_ok()
}

/// Parse the seller's challenge for `resource` on `network`. An unknown network, a rail without
/// the hook and a resource that is no URL keep their own codes; a reply that the rail refuses
/// is [`SELLER_FAILED`], and so is a challenge that has already expired.
pub fn challenge(network: &str, resource: &str, reply: &SellerReply<'_>) -> Result<Challenge> {
    let (network, hook) = hook(network)?;
    resource_url(resource)?;
    if reply.body.len() > MAX_CHALLENGE_BYTES {
        return Err(seller_failed(
            "The seller's payment challenge is too large.",
        ));
    }
    // What the rail refuses here is the seller's reply: malformed, of another scheme or
    // network, or with terms that this rail cannot settle. It is a seller failure, apart from
    // the caller's own input above and from every payment and policy error.
    let challenge = hook
        .challenge(network, resource, reply)
        .map_err(|error| seller_failed(error.message))?;
    // The rail answers for the selected network and resource only.
    if challenge.network != network || challenge.resource != resource {
        return Err(Error::config(
            "The rail read a challenge for another network or resource.",
        ));
    }
    challenge.units()?;
    challenge.context().validate()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::config("The system clock is before 1970."))?
        .as_secs();
    unexpired(&challenge, now)?;
    Ok(challenge)
}

/// Refuse a challenge that the seller no longer accepts at `now`, in Unix seconds. A payment
/// for it would buy nothing, so it is refused before anything is paid.
pub fn unexpired(challenge: &Challenge, now: u64) -> Result<()> {
    match challenge.expires_at {
        Some(expires_at) if expires_at <= now => Err(seller_failed(
            "The seller's payment challenge has expired. Request the resource again.",
        )),
        _ => Ok(()),
    }
}

/// Build the proof that the client presents for `challenge` from the outcome document of the
/// confirmed operation that paid it.
pub fn proof(challenge: &Challenge, receipt: &serde_json::Value) -> Result<Proof> {
    let (_, hook) = hook(&challenge.network)?;
    let proof = hook.proof(challenge, receipt)?;
    if !header_name(&proof.header) || !header_value(&proof.value) {
        return Err(Error::config("The rail built an invalid payment proof."));
    }
    Ok(proof)
}

/// The paid-service context that an outcome document records, if it has a bounded one.
pub fn recorded(document: &serde_json::Value) -> Option<PaidService> {
    serde_json::from_value::<PaidService>(document.get(KEY)?.clone())
        .ok()
        .filter(|context| context.validate().is_ok())
}

/// One request to a seller.
#[derive(Clone, Debug)]
pub struct SellerRequest {
    /// `GET`, `POST`, `PUT`, `PATCH` or `DELETE`.
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// A seller's reply with a bounded body.
#[derive(Clone, Debug)]
pub struct SellerResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}
impl SellerResponse {
    /// This reply for a rail's challenge parser.
    pub fn reply(&self) -> SellerReply<'_> {
        SellerReply {
            status: self.status,
            headers: &self.headers,
            body: &self.body,
        }
    }
    /// The first value of the header `name`.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

fn header_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
fn header_value(value: &str) -> bool {
    value.len() <= 16_384 && value.bytes().all(|byte| (0x20..0x7f).contains(&byte))
}
fn method(name: &str) -> Result<Method> {
    Ok(match name {
        "GET" => Method::Get,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "PATCH" => Method::Patch,
        "DELETE" => Method::Delete,
        _ => {
            return Err(Error::usage(
                "The method must be GET, POST, PUT, PATCH or DELETE.",
            ));
        }
    })
}

/// Send one request to a seller, with `proof` as its payment header when given. The reply body
/// is bounded by [`MAX_BODY_BYTES`], redirects are not followed and the request is sent once.
/// A reply of any status is returned; a seller that gave no answer is a [`SELLER_FAILED`]
/// error. A caller header never replaces the proof header.
pub fn send(request: &SellerRequest, proof: Option<&Proof>) -> Result<SellerResponse> {
    resource_url(&request.url)?;
    if request.url.contains('#') {
        return Err(Error::usage("The resource URL must have no fragment."));
    }
    let method = method(&request.method)?;
    if request.headers.len() > MAX_HEADERS
        || request
            .headers
            .iter()
            .any(|(name, value)| !header_name(name) || !header_value(value))
    {
        return Err(Error::usage("A request header is invalid."));
    }
    if request
        .body
        .as_ref()
        .is_some_and(|body| body.len() > MAX_BODY_BYTES)
    {
        return Err(Error::usage("The request body is too large."));
    }
    let mut headers: Vec<(String, String)> = request
        .headers
        .iter()
        .filter(|(name, _)| proof.is_none_or(|proof| !name.eq_ignore_ascii_case(&proof.header)))
        .cloned()
        .collect();
    if let Some(proof) = proof {
        headers.push((proof.header.clone(), proof.value.clone()));
    }
    let client = HttpClient::new(Options {
        timeout: SELLER_TIMEOUT,
        max_response_bytes: MAX_BODY_BYTES,
        no_proxy: false,
        connect_timeout: Some(Duration::from_secs(10)),
    })
    .map_err(|_| Error::config("The seller transport is unavailable."))?;
    let response = client
        .send(
            Request {
                method,
                url: &request.url,
                headers: &headers,
                body: request.body.as_deref(),
            },
            None,
        )
        .map_err(|error| {
            seller_failed(match error.kind {
                ErrorKind::Size => "The seller's reply exceeds 8 MiB.",
                ErrorKind::Timeout | ErrorKind::Deadline => "The seller did not answer in time.",
                ErrorKind::Connection => "The seller could not be reached.",
                ErrorKind::Configuration => "The seller URL cannot be requested.",
                _ => "The seller's reply could not be read.",
            })
        })?;
    Ok(SellerResponse {
        status: response.status,
        headers: response
            .headers
            .into_iter()
            .filter_map(|(name, value)| Some((name, String::from_utf8(value).ok()?)))
            .collect(),
        body: response.body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    /// A local seller that answers one request with `reply` and returns what it received.
    fn seller(reply: Vec<u8>) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/report", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut received = Vec::new();
            let mut buffer = [0u8; 4096];
            while !received.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                received.extend_from_slice(&buffer[..count]);
            }
            stream.write_all(&reply).unwrap();
            String::from_utf8_lossy(&received).into_owned()
        });
        (url, handle)
    }
    fn get(url: &str) -> SellerRequest {
        SellerRequest {
            method: "GET".into(),
            url: url.into(),
            headers: vec![],
            body: None,
        }
    }

    #[test]
    fn a_seller_reply_of_any_status_is_returned_with_its_headers() {
        let (url, handle) = seller(
            b"HTTP/1.1 402 Payment Required\r\nX-Terms: a\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"
                .to_vec(),
        );
        let reply = send(&get(&url), None).unwrap();
        assert_eq!((reply.status, reply.body.as_slice()), (402, &b"{}"[..]));
        assert_eq!(reply.header("x-terms"), Some("a"));
        assert!(
            handle
                .join()
                .unwrap()
                .starts_with("GET /report HTTP/1.1\r\n")
        );
    }

    #[test]
    fn the_proof_header_is_sent_once_and_no_caller_header_replaces_it() {
        let (url, handle) =
            seller(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec());
        let mut request = get(&url);
        request.headers = vec![
            ("x-proof".into(), "forged".into()),
            ("Accept".into(), "text/plain".into()),
        ];
        let proof = Proof {
            header: "X-Proof".into(),
            value: "settled".into(),
        };
        assert_eq!(send(&request, Some(&proof)).unwrap().body, b"ok");
        let received = handle.join().unwrap().to_ascii_lowercase();
        assert!(received.contains("x-proof: settled\r\n"), "{received}");
        assert!(!received.contains("forged"), "{received}");
        assert!(received.contains("accept: text/plain\r\n"), "{received}");
    }

    #[test]
    fn a_redirect_is_returned_and_never_followed() {
        let (url, handle) = seller(
            b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/other\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec(),
        );
        let reply = send(&get(&url), None).unwrap();
        assert_eq!(reply.status, 302);
        handle.join().unwrap();
    }

    #[test]
    fn an_invalid_request_never_reaches_the_network() {
        let refused = |change: &dyn Fn(&mut SellerRequest)| {
            let mut request = get("http://127.0.0.1:1/report");
            change(&mut request);
            assert_eq!(send(&request, None).unwrap_err().code, 2);
        };
        refused(&|r| r.url = "file:///etc/passwd".into());
        refused(&|r| r.url = "http://user:secret@127.0.0.1:1/".into());
        refused(&|r| r.url = "http://127.0.0.1:1/a#b".into());
        refused(&|r| r.method = "TRACE".into());
        refused(&|r| r.method = "get".into());
        refused(&|r| r.headers = vec![("bad name".into(), "x".into())]);
        refused(&|r| r.headers = vec![("X-A".into(), "line\r\nX-B: 1".into())]);
        refused(&|r| r.headers = vec![("X-A".into(), "x".into()); MAX_HEADERS + 1]);
    }

    #[test]
    fn a_seller_that_gives_no_answer_has_its_own_code() {
        // Port 1 on the loopback accepts no connection.
        let error = send(&get("http://127.0.0.1:1/report"), None).unwrap_err();
        assert_eq!(error.code, SELLER_FAILED);
        assert_ne!(error.code, crate::transport_failure::NETWORK_UNAVAILABLE);
        assert_eq!(seller_failed("x").code, 8);
    }

    #[test]
    fn an_expired_challenge_is_a_seller_failure_before_any_payment() {
        let challenge = |expires_at| Challenge {
            id: None,
            resource: "https://seller.example/report".into(),
            network: "example:testnet".into(),
            scheme: "exact".into(),
            pay_to: "RECIPIENT".into(),
            amount: crate::execution::Quantity::new(
                25,
                crate::input::asset::AssetDisplay {
                    symbol: "XYZ".into(),
                    decimals: 2,
                    name: "Example".into(),
                    native: true,
                },
            ),
            asset: "XYZ".into(),
            expires_at,
            terms: serde_json::Value::Null,
        };
        let now = 1_800_000_000;
        // A seller that states no expiry, and one that expires later, are payable.
        unexpired(&challenge(None), now).unwrap();
        unexpired(&challenge(Some(now + 1)), now).unwrap();
        for expired in [1, now - 1, now] {
            let error = unexpired(&challenge(Some(expired)), now).unwrap_err();
            assert_eq!(error.code, SELLER_FAILED, "{expired}");
        }
    }

    #[test]
    fn a_recorded_context_reads_from_an_outcome_document_when_it_is_bounded() {
        let context = serde_json::json!({"resource":"https://seller.example/report",
            "challengeId":"c-0001","payTo":"RECIPIENT","amount":"25","asset":"XYZ"});
        let document = serde_json::json!({"requestId":"r", KEY: context});
        assert_eq!(
            recorded(&document).unwrap().challenge_id.as_deref(),
            Some("c-0001")
        );
        for other in [
            serde_json::json!({"requestId":"r"}),
            serde_json::json!({KEY: "text"}),
            serde_json::json!({KEY: {"resource":"javascript:alert(1)","payTo":"R","amount":"1","asset":"X"}}),
        ] {
            assert!(recorded(&other).is_none());
        }
    }
}
