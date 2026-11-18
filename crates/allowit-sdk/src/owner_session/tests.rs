//! The SDK's owner sign-in against a fake backend, with a fake rail signature.
use super::*;
use serde_json::json;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

const OWNER: &str = "owner-account-1";
const ORIGIN: &str = "http://localhost:5173";
const NETWORK: &str = "example:testnet";
const NOW: u64 = 1_800_000_000;
const GUEST: &str = "allowit_session=gggggggggggggggggggggggggggggggg";
const ROTATED: &str = "allowit_session=ssssssssssssssssssssssssssssssss";

struct Owner;
impl Signer for Owner {
    fn network(&self) -> &str {
        NETWORK
    }
    fn address(&self) -> &str {
        OWNER
    }
    fn sign_hash(&self, digest: &[u8]) -> crate::error::Result<Vec<u8>> {
        Ok(digest.to_vec())
    }
}

/// The fake rail's text signature: the message bytes in reverse.
fn signed(message: &str) -> Vec<u8> {
    message.bytes().rev().collect()
}

/// A rail whose text signature is [`signed`]; it counts what it signed.
#[derive(Default)]
struct Signature(AtomicUsize);
impl MessageSignature for Signature {
    fn sign_message(&self, signer: &dyn Signer, message: &str) -> crate::error::Result<Vec<u8>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        signer.sign_hash(&signed(message))
    }
    fn verify_message(
        &self,
        _: &str,
        _: &str,
        message: &str,
        signature: &[u8],
    ) -> crate::error::Result<()> {
        if signature == signed(message) {
            Ok(())
        } else {
            Err(Error::usage("bad signature"))
        }
    }
}

/// `at` as an RFC 3339 UTC time (civil date from days since 1970-01-01).
fn date(at: u64) -> String {
    let (days, seconds) = ((at / 86_400) as i64, at % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    )
}

fn message(nonce: &str, issued: u64) -> String {
    format!(
        "localhost:5173 wants you to sign in with your wallet account:\n{OWNER}\n\nSign in to AllowIt. This does not authorize spending or move funds.\n\nURI: {ORIGIN}\nVersion: 1\nNonce: {nonce}\nIssued At: {}\nExpiration Time: {}",
        date(issued),
        date(issued + 300)
    )
}

/// How the fake backend answers.
#[derive(Clone)]
struct Plan {
    challenge: Value,
    rotated: &'static str,
    route: (u16, Value),
}
impl Plan {
    fn new() -> Self {
        let nonce = "a".repeat(64);
        Self {
            challenge: json!({"id": nonce, "message": message(&nonce, NOW), "address": OWNER,
                "origin": ORIGIN, "expiresAt": date(NOW + 300)}),
            rotated: ROTATED,
            route: (200, json!({"ok": true})),
        }
    }
}

/// The path and the cookie of each request that the backend saw.
type Seen = Arc<Mutex<Vec<(String, String)>>>;

/// A backend that answers each request of the plan and records what it saw.
fn backend(plan: Plan) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let path = line
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_owned();
            let (mut length, mut cookie) = (0, String::new());
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                let header = header.trim_end();
                if header.is_empty() {
                    break;
                }
                let (name, value) = header.split_once(": ").unwrap_or((header, ""));
                match name.to_ascii_lowercase().as_str() {
                    "content-length" => length = value.parse().unwrap(),
                    "cookie" => cookie = value.to_owned(),
                    _ => {}
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            log.lock().unwrap().push((path.clone(), cookie.clone()));
            let (status, reply, set) = match path.as_str() {
                "/api/auth/challenge" => (200, plan.challenge.clone(), Some(GUEST)),
                "/api/auth/verify" => {
                    assert_eq!(cookie, GUEST);
                    let digest = signed(body["message"].as_str().unwrap());
                    assert_eq!(body["signature"], STANDARD.encode(digest));
                    (
                        200,
                        json!({"session": {"address": OWNER, "network": "wallet",
                            "expiresAt": date(NOW + 86_400)}}),
                        Some(plan.rotated),
                    )
                }
                _ => (plan.route.0, plan.route.1.clone(), None),
            };
            let bytes = serde_json::to_vec(&reply).unwrap();
            let set = set
                .map(|c| format!("Set-Cookie: {c}; Path=/; HttpOnly; SameSite=Lax\r\n"))
                .unwrap_or_default();
            write!(
                stream,
                "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{set}\r\n",
                bytes.len()
            )
            .unwrap();
            stream.write_all(&bytes).unwrap();
        }
    });
    (url, seen)
}

#[test]
fn dates_are_rfc3339() {
    assert_eq!(date(0), "1970-01-01T00:00:00Z");
    assert_eq!(date(1_800_000_000), "2027-01-15T08:00:00Z");
    assert_eq!(timestamp(&date(NOW + 301)).unwrap(), NOW + 301);
}

#[test]
fn the_sdk_signs_the_owner_in_and_posts_under_the_rotated_session() {
    let (url, seen) = backend(Plan::new());
    let signature = Signature::default();
    let session = Session::sign_in(&url, ORIGIN, NETWORK, &Owner, &signature, NOW).unwrap();
    assert_eq!(signature.0.load(Ordering::SeqCst), 1);
    let reply = session
        .post("/api/native/reputation", &json!({"policyId": "p"}), NOW)
        .unwrap();
    assert_eq!(reply, json!({"ok": true}));
    assert_eq!(
        seen.lock().unwrap().clone(),
        vec![
            ("/api/auth/challenge".to_owned(), String::new()),
            ("/api/auth/verify".to_owned(), GUEST.to_owned()),
            ("/api/native/reputation".to_owned(), ROTATED.to_owned()),
        ]
    );
    // A path that is no API route of the backend, such as one that names another host, sends
    // nothing and no cookie.
    for path in [
        ".attacker.example/api/collect",
        "@attacker.example/api/x",
        "/api/../auth/verify",
    ] {
        assert!(session.post(path, &json!({}), NOW).is_err(), "{path}");
    }
    assert_eq!(seen.lock().unwrap().len(), 3);
    // A session past its expiry sends nothing.
    assert!(
        session
            .post("/api/native/reputation", &json!({}), NOW + 86_400)
            .is_err()
    );
}

#[test]
fn the_owner_signs_nothing_but_the_exact_sign_in_message() {
    let nonce = "a".repeat(64);
    let altered = [
        // Another account, another origin, another nonce, an extra line, an old or a future
        // message, and a message that outlives five minutes.
        message(&nonce, NOW).replace(OWNER, "owner-account-2"),
        message(&nonce, NOW).replace(
            "URI: http://localhost:5173",
            "URI: http://elsewhere.example",
        ),
        message(&"b".repeat(64), NOW),
        format!("{}\nTransfer: everything", message(&nonce, NOW)),
        message(&nonce, NOW - 301),
        message(&nonce, NOW + 31),
    ];
    for text in altered {
        let mut plan = Plan::new();
        plan.challenge["message"] = json!(text);
        let (url, seen) = backend(plan);
        let signature = Signature::default();
        assert!(Session::sign_in(&url, ORIGIN, NETWORK, &Owner, &signature, NOW).is_err());
        assert_eq!(signature.0.load(Ordering::SeqCst), 0, "{text}");
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
    // A challenge for another owner, origin or with an expired time is refused before signing.
    for (field, value) in [
        ("address", json!("owner-account-2")),
        ("origin", json!("http://elsewhere.example")),
        ("id", json!("not-hex")),
        ("expiresAt", json!(date(NOW))),
    ] {
        let mut plan = Plan::new();
        plan.challenge[field] = value;
        let (url, _) = backend(plan);
        let signature = Signature::default();
        assert!(Session::sign_in(&url, ORIGIN, NETWORK, &Owner, &signature, NOW).is_err());
        assert_eq!(signature.0.load(Ordering::SeqCst), 0, "{field}");
    }
}

#[test]
fn an_empty_origin_is_refused_before_any_request() {
    let (url, seen) = backend(Plan::new());
    let signature = Signature::default();
    let error = Session::sign_in(&url, "", NETWORK, &Owner, &signature, NOW)
        .err()
        .unwrap();
    assert_eq!(error.message, "Set the explicit owner sign-in Origin");
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(signature.0.load(Ordering::SeqCst), 0);
}

#[test]
fn a_session_that_the_server_did_not_rotate_is_refused() {
    let mut plan = Plan::new();
    plan.rotated = GUEST;
    let (url, _) = backend(plan);
    let error = Session::sign_in(&url, ORIGIN, NETWORK, &Owner, &Signature::default(), NOW)
        .err()
        .unwrap();
    assert_eq!(
        error.message,
        "Owner session was not safely rotated for this account"
    );
}

#[test]
fn a_typed_refusal_keeps_its_code_and_details() {
    let mut plan = Plan::new();
    plan.route = (
        404,
        json!({"error": "This policy holds no request with this ID.",
            "code": "reputation_request_not_found", "details": {"requestId": "r9"}}),
    );
    let (url, _) = backend(plan);
    let session =
        Session::sign_in(&url, ORIGIN, NETWORK, &Owner, &Signature::default(), NOW).unwrap();
    let error = session
        .post("/api/native/reputation", &json!({}), NOW)
        .unwrap_err();
    assert_eq!(
        error.api_code.as_deref(),
        Some("reputation_request_not_found")
    );
    assert_eq!(error.status, Some(404));
    assert_eq!(error.detail("requestId"), Some("r9"));
}
