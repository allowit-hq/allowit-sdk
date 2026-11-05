//! Core-owned bounded request ingress and canonical operation identities.
#[cfg(test)]
use crate::error::Result;
#[cfg(test)]
use allowit_sdk::vendor::typed_request::validate_json;
pub use allowit_sdk::vendor::typed_request::{decode_bytes, operation_nonce, read_request_file};
#[cfg(test)]
const MAX_REQUEST_BYTES: usize = 65_536;
#[cfg(test)]
const MAX_JSON_DEPTH: usize = 48;
#[cfg(test)]
const MAX_JSON_ITEMS: usize = 4096;
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
        decode_bytes(raw.as_bytes())
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
