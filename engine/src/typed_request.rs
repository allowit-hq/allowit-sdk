//! Bounded file ingress. Callers decode directly into canonical SDK DTOs.
use crate::error::{Error, Result};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

pub(crate) const MAX_REQUEST_BYTES: usize = 65_536;
pub(crate) const MAX_JSON_DEPTH: usize = 48;
pub(crate) const MAX_JSON_ITEMS: usize = 4096;

/// Read an explicitly selected local regular file. Never resolve paths embedded in JSON.
pub fn read_request_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let initial =
        std::fs::metadata(path).map_err(|_| Error::usage("Cannot read request file metadata."))?;
    if !initial.is_file() {
        return Err(Error::usage(
            "The request path must name a local regular file.",
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    // A FIFO swapped in after the metadata check must not block the open.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    let file = options
        .open(path)
        .map_err(|_| Error::usage("Cannot open the request file."))?;
    let metadata = file
        .metadata()
        .map_err(|_| Error::usage("Cannot read request file metadata."))?;
    if !metadata.is_file() {
        return Err(Error::usage(
            "The request path must name a local regular file.",
        ));
    }
    if metadata.len() > MAX_REQUEST_BYTES as u64 {
        return Err(Error::usage("The request file exceeds 65536 bytes."));
    }
    decode_reader(file)
}

/// Decode an in-memory document with the same bounds as a request file.
pub fn decode_bytes<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    decode_reader(bytes)
}

/// Exact version-1 operation nonce. Preserve the caller's policy and request ID bytes.
pub fn operation_nonce(policy_id: &str, request_id: &str) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"allowit-workflow-operation-v1");
    hash.update([0]);
    hash.update(policy_id.as_bytes());
    hash.update([0]);
    hash.update(request_id.as_bytes());
    hash.finalize().into()
}

fn decode_reader<T: DeserializeOwned>(mut reader: impl Read) -> Result<T> {
    // The stream cap also protects against a file that grows after the metadata check.
    // A fixed buffer never reallocates a partially filled secret-bearing input.
    let mut bytes = zeroize::Zeroizing::new(vec![0u8; MAX_REQUEST_BYTES + 1]);
    let mut length = 0;
    while length < bytes.len() {
        match reader.read(&mut bytes[length..]) {
            Ok(0) => break,
            Ok(count) => length += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(Error::usage("Cannot read the request file.")),
        }
    }
    if length > MAX_REQUEST_BYTES {
        return Err(Error::usage("The request file exceeds 65536 bytes."));
    }
    let input = &bytes[..length];
    validate_json(input, MAX_REQUEST_BYTES)?;
    // Do not collapse objects into Value. Canonical struct deserialization rejects duplicate fields.
    // Suppress deserializer details: they can contain private request field values.
    let mut decoder = serde_json::Deserializer::from_slice(input);
    let request = T::deserialize(&mut decoder)
        .map_err(|_| Error::usage("The request file does not match the typed request schema."))?;
    decoder
        .end()
        .map_err(|_| Error::usage("The request file must contain exactly one JSON object."))?;
    Ok(request)
}

/// Bound containers and field/entry separators before any recursive JSON deserialization.
fn structural_bounds(bytes: &[u8]) -> Result<()> {
    let mut depth = 0usize;
    let mut items = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    let mut first = None;
    for &byte in bytes {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        if !byte.is_ascii_whitespace() && first.is_none() {
            first = Some(byte);
        }
        match byte {
            b'"' => quoted = true,
            b'{' | b'[' => {
                depth += 1;
                items += 1;
                if depth > MAX_JSON_DEPTH {
                    return Err(Error::usage("The request file exceeds 48 nesting levels."));
                }
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
            }
            b':' | b',' => {
                items += 1;
            }
            _ => {}
        }
        if items > MAX_JSON_ITEMS {
            return Err(Error::usage(
                "The request file contains too many JSON entries.",
            ));
        }
    }
    if first != Some(b'{') {
        return Err(Error::usage("The request file must contain a JSON object."));
    }
    Ok(())
}

/// Reject ambiguous object keys before any typed map or JSON projection can collapse them.
pub(crate) fn validate_json(bytes: &[u8], limit: usize) -> Result<()> {
    if bytes.len() > limit {
        return Err(Error::usage("JSON document exceeds its size limit"));
    }
    structural_bounds(bytes)?;
    struct Unique;
    impl<'de> serde::Deserialize<'de> for Unique {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
            struct Visitor;
            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = Unique;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("unambiguous JSON")
                }
                fn visit_bool<E: serde::de::Error>(
                    self,
                    _: bool,
                ) -> std::result::Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_i64<E: serde::de::Error>(self, _: i64) -> std::result::Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_u64<E: serde::de::Error>(self, _: u64) -> std::result::Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_f64<E: serde::de::Error>(self, _: f64) -> std::result::Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_str<E: serde::de::Error>(self, _: &str) -> std::result::Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_seq<A: serde::de::SeqAccess<'de>>(
                    self,
                    mut a: A,
                ) -> std::result::Result<Unique, A::Error> {
                    while a.next_element::<Unique>()?.is_some() {}
                    Ok(Unique)
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut a: A,
                ) -> std::result::Result<Unique, A::Error> {
                    let mut keys = std::collections::BTreeSet::new();
                    while let Some(key) = a.next_key::<String>()? {
                        if !keys.insert(key) {
                            return Err(serde::de::Error::custom("Duplicate JSON key"));
                        }
                        a.next_value::<Unique>()?;
                    }
                    Ok(Unique)
                }
            }
            d.deserialize_any(Visitor)
        }
    }
    let mut d = serde_json::Deserializer::from_slice(bytes);
    let _: Unique = serde::Deserialize::deserialize(&mut d)
        .map_err(|_| Error::usage("JSON document has invalid or duplicate fields"))?;
    d.end()
        .map_err(|_| Error::usage("JSON document must contain one object"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    // Test-only shape: production will use the canonical SDK DTO without Value preprocessing.
    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct Request {
        name: String,
        child: Option<Box<Request>>,
    }
    fn parse(raw: &str) -> Result<Request> {
        decode_reader(raw.as_bytes())
    }
    #[test]
    fn preserves_unicode_and_escaped_container_characters() {
        let value = parse(r#"{"name":"雪 🌟 [{\\\"","child":null}"#).unwrap();
        assert!(value.name.starts_with("雪 🌟"));
    }
    #[test]
    fn canonical_deserializer_rejects_duplicate_fields_before_value_collapse() {
        assert!(validate_json(br#"{"params":{"limit":1,"limit":2}}"#, 1024).is_err());
        assert!(validate_json(br#"{"items":[{"limit":1,"limit":2}]}"#, 1024).is_err());
        validate_json(
            br#"{"params":{"limit":123456789012345678901234567890}}"#,
            1024,
        )
        .unwrap();
        assert!(parse(r#"{"name":"a","name":"b","child":null}"#).is_err());
        assert!(parse(r#"{"name":"a","child":{"name":"b","name":"c","child":null}}"#).is_err());
    }
    #[test]
    fn rejects_unknown_fields_without_echoing_private_values() {
        let error = parse(r#"{"name":"a","child":null,"outcome":"private-sentinel"}"#).unwrap_err();
        assert!(!error.message.contains("private-sentinel"));
    }
    #[test]
    fn rejects_trailing_objects_and_nonobjects() {
        assert!(parse(r#"{"name":"a","child":null}{}"#).is_err());
        assert!(parse("[]").is_err());
    }
    #[test]
    fn bounds_bytes_even_without_file_metadata() {
        let raw = format!(
            "{{\"name\":\"{}\",\"child\":null}}",
            "x".repeat(MAX_REQUEST_BYTES)
        );
        assert!(parse(&raw).unwrap_err().message.contains("65536"));
    }
    #[test]
    fn bounds_nesting_before_recursive_decode() {
        let raw = format!(
            "{{\"name\":{}{}{} }}",
            "[".repeat(MAX_JSON_DEPTH),
            "0",
            "]".repeat(MAX_JSON_DEPTH)
        );
        assert!(parse(&raw).unwrap_err().message.contains("nesting"));
    }
    #[test]
    fn bounds_large_flat_arrays_before_decode() {
        let raw = format!("{{\"name\":[{}]}}", vec!["0"; MAX_JSON_ITEMS + 1].join(","));
        assert!(parse(&raw).unwrap_err().message.contains("entries"));
    }
    #[test]
    fn exact_backend_golden_nonce_preserves_case_and_separators() {
        // Canonical backend encoding vector v1; public synthetic identity, not authorization.
        let expected = [
            12, 57, 238, 90, 29, 91, 223, 216, 248, 190, 15, 1, 76, 25, 43, 108, 77, 109, 38, 121,
            244, 115, 120, 219, 133, 162, 59, 202, 151, 160, 251, 18,
        ];
        assert_eq!(
            operation_nonce("owner-policy_Example-A", "cli-native-transfer-01"),
            expected
        );
        assert_ne!(
            operation_nonce("owner-policy_example-a", "cli-native-transfer-01"),
            expected
        );
        assert_ne!(
            operation_nonce("owner-policy_Example-A", "cli-native-transfer-01 "),
            expected
        );
    }
    #[test]
    fn reads_only_explicit_local_regular_file() {
        let path = std::env::temp_dir().join(format!(
            "allowit-request-reader-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, r#"{"name":"retained","child":null}"#).unwrap();
        assert_eq!(
            read_request_file::<Request>(&path).unwrap().name,
            "retained"
        );
        std::fs::remove_file(path).unwrap();
        assert!(read_request_file::<Request>(&std::env::temp_dir()).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn fifo_open_does_not_block() {
        let path =
            std::env::temp_dir().join(format!("allowit-request-fifo-{}", std::process::id()));
        let c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        // The metadata check rejects it; a swap after that check reaches the nonblocking open.
        assert!(read_request_file::<Request>(&path).is_err());
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
        let file = options.open(&path).unwrap();
        assert!(!file.metadata().unwrap().is_file());
        std::fs::remove_file(path).unwrap();
    }
}
