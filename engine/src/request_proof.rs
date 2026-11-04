//! Request-proof transport framing. Native adapters supply identity and signature semantics.
use crate::error::{Error, Result};
use sha2::{Digest, Sha256};

pub(crate) struct Protocol {
    pub domain: &'static str,
    pub scheme: &'static str,
    pub signature_bytes: usize,
    #[cfg_attr(not(feature = "stellar"), allow(dead_code))]
    pub validate_account: fn(&str) -> Result<()>,
    pub verify: fn(&str, &[u8; 32], &[u8]) -> Result<()>,
}

fn digest(protocol: &Protocol, issued_at: u64, path: &str, body: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(format!("{}\n{issued_at}\n{path}\n", protocol.domain));
    hash.update(body);
    hash.finalize().into()
}

#[cfg_attr(not(feature = "stellar"), allow(dead_code))]
fn encode(protocol: &Protocol, account: &str, issued_at: u64, signature: &[u8]) -> Result<String> {
    (protocol.validate_account)(account)?;
    if signature.len() != protocol.signature_bytes {
        return Err(Error::config("Invalid native signature size"));
    }
    let signature: String = signature.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!(
        "Bearer {}.{account}.{issued_at}.{signature}",
        protocol.scheme
    ))
}

#[cfg_attr(not(feature = "stellar"), allow(dead_code))]
pub(crate) fn authorize(
    protocol: Protocol,
    account: &str,
    issued_at: u64,
    path: &str,
    body: &[u8],
    sign: impl FnOnce(&[u8; 32]) -> Result<Vec<u8>>,
) -> Result<String> {
    let digest = digest(&protocol, issued_at, path, body);
    let signature = sign(&digest)?;
    (protocol.verify)(account, &digest, &signature)?;
    encode(&protocol, account, issued_at, &signature)
}

/// Verify identity only. The caller independently admits the signer for this operation.
pub(crate) fn verify(
    protocol: Protocol,
    proof: &str,
    now: u64,
    path: &str,
    body: &[u8],
) -> Result<String> {
    if proof.len() > 512 {
        return Err(Error::config("Native request proof exceeds limit"));
    }
    let parts: Vec<_> = proof.split('.').collect();
    if parts.len() != 4 || parts[0] != protocol.scheme {
        return Err(Error::config("Invalid native request proof"));
    }
    let issued_at = parts[2]
        .parse::<u64>()
        .map_err(|_| Error::config("Invalid request time"))?;
    if now.abs_diff(issued_at) > 120 {
        return Err(Error::config("Native request proof is not current"));
    }
    let encoded = parts[3];
    if encoded.len() != protocol.signature_bytes * 2
        || !encoded.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error::config("Invalid hexadecimal identity"));
    }
    let signature = encoded
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            // The ASCII and size checks above make each pair valid UTF-8 hexadecimal.
            u8::from_str_radix(std::str::from_utf8(pair).expect("ASCII hexadecimal"), 16)
                .expect("checked hexadecimal")
        })
        .collect::<Vec<_>>();
    (protocol.verify)(
        parts[1],
        &digest(&protocol, issued_at, path, body),
        &signature,
    )?;
    Ok(parts[1].into())
}

#[cfg(all(test, feature = "stellar"))]
mod tests {
    use super::*;
    use allowit_stellar_wallet::{self as native, signing};
    fn protocol() -> Protocol {
        crate::vendors::runtime_request_proof_protocol("stellar:testnet").unwrap()
    }
    fn signer() -> signing::LocalSigner {
        signing::LocalSigner::from_entropy(&[7; 32]).unwrap()
    }
    fn proof(at: u64, path: &str, body: &[u8]) -> String {
        let signer = signer();
        authorize(protocol(), signer.account(), at, path, body, |digest| {
            signer.sign_digest(digest).map_err(Error::config)
        })
        .unwrap()
    }
    #[test]
    fn wire_bytes_preserve_the_original_native_service_protocol() {
        let signer = signer();
        let body = br#"{"network":"stellar:testnet","requestId":"one"}"#;
        let digest = digest(&protocol(), 42, "/api/native/execute", body);
        let mut old = b"allowit-stellar-request-v1\n42\n/api/native/execute\n".to_vec();
        old.extend_from_slice(body);
        assert_eq!(digest, native::hash(old));
        assert_eq!(
            encode(&protocol(), signer.account(), 42, &[1; 64]).unwrap(),
            format!(
                "Bearer stellar-proof.{}.42.{}",
                signer.account(),
                native::hex(&[1; 64])
            )
        );
        assert!(encode(&protocol(), signer.account(), 42, &[1; 63]).is_err());
        assert!(encode(&protocol(), "untrusted\nheader", 42, &[1; 64]).is_err());
        let original = format!(
            "Bearer stellar-proof.{}.42.{}",
            signer.account(),
            native::hex(&signer.sign_digest(&digest).unwrap())
        );
        assert_eq!(proof(42, "/api/native/execute", body), original);
    }
    #[test]
    fn verification_binds_exact_body_path_identity_and_inclusive_time_window() {
        let now = 1_800_000_000;
        let path = "/api/native/prepare";
        let body = br#"{"requestId":"original","amount":"5"}"#;
        let bearer = proof(now, path, body);
        let token = bearer.strip_prefix("Bearer ").unwrap();
        for at in [now - 120, now, now + 120] {
            assert_eq!(
                verify(protocol(), token, at, path, body).unwrap(),
                signer().account()
            );
        }
        for (at, route, data) in [
            (now - 121, path, body.as_slice()),
            (now + 121, path, body.as_slice()),
            (now, "/api/native/submit", body.as_slice()),
            (now, path, b"{}".as_slice()),
            (
                now,
                path,
                br#"{ "requestId":"original","amount":"5"}"#.as_slice(),
            ),
        ] {
            assert!(verify(protocol(), token, at, route, data).is_err());
        }
        assert!(verify(protocol(), &bearer, now, path, body).is_err());
        for malformed in [
            "",
            "stellar-proof",
            "other.account.1.ab",
            "stellar-proof.a.not-time.ab",
            "stellar-proof.a.1800000000.zz",
            &"a".repeat(513),
        ] {
            assert!(verify(protocol(), malformed, now, path, body).is_err());
        }
        let wrong = token.replacen(signer().account(), &native::account_for_seed(&[8; 32]), 1);
        assert!(verify(protocol(), &wrong, now, path, body).is_err());
    }
    #[test]
    fn signing_capability_must_return_a_valid_signature_before_header_encoding() {
        assert!(
            authorize(
                protocol(),
                signer().account(),
                42,
                "/api/native/execute",
                b"{}",
                |_| Ok(vec![0; 64])
            )
            .is_err()
        );
    }
}
