//! Bound operation request identities shared by host callers.
/// Request IDs contain 8 to 100 ASCII letters, digits, dots, underscores, colons or hyphens.
pub fn request_id_valid(value: &str) -> bool {
    (8..=100).contains(&value.len())
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
}
