//! Compile consumer code against public paths that predate the provider boundary.
use allowit_sdk::{
    owner_session::Challenge,
    profiles::{Description, Profile},
    runtime::{Installation, PacketInfo},
};

fn consume_profile(profile: Profile) -> String {
    let _ = Profile::description(&profile);
    let _ = Profile::server_origin(&profile);
    profile.version
}

#[test]
fn retained_public_paths_and_owned_fields_compile_for_consumers() {
    let _: fn(Profile) -> String = consume_profile;
    let _: fn(&Installation) -> &Description = Installation::description;
    let _: fn(&Installation) -> &[u8] = Installation::native_document;
    let info = PacketInfo {
        operation_id: [0; 32],
        transaction_hash: [1; 32],
        wallet: "example-wallet".into(),
        expires_at: 7,
    };
    assert!(format!("{info:?}").contains("example-wallet"));
    let challenge: Challenge = serde_json::from_str(
        r#"{"id":"challenge","message":"sign","address":"owner","origin":"http://localhost:8080","expiresAt":"2026-11-05T22:00:00Z"}"#,
    ).unwrap();
    assert_eq!(challenge.origin, "http://localhost:8080");
}

#[test]
fn public_generation_validation_preserves_network_error_order() {
    use allowit_sdk::generation::{GenerateRequest, Target};
    let mut request = GenerateRequest {
        prompt: "allow a payment".into(),
        network: "unsupported:network".into(),
        profile: "".into(),
        target: Target::NewWallet,
    };
    let error = request.validate().unwrap_err();
    assert!(error.message.contains("unknown rail"));
    request.prompt.clear();
    assert!(request.validate().unwrap_err().message.contains("5–3000"));
}

/// The npm package shows the same wallet kinds as Core: `npm/src/wallet-kinds.js` holds the exact
/// JSON of `WALLET_KINDS`. Regenerate it with `ALLOWIT_WRITE_WALLET_KINDS=1 cargo test`.
#[test]
fn npm_wallet_kinds_equal_the_core_description() {
    let kinds = serde_json::to_string_pretty(&allowit_sdk::kinds::WALLET_KINDS).unwrap();
    let module = format!(
        "// Generated from Core `vendor::kinds::WALLET_KINDS` by the SDK test `npm_wallet_kinds_equal_the_core_description`.\nexport default Object.freeze({kinds}.map((kind) => Object.freeze({{...kind, supportedParams: Object.freeze(kind.supportedParams)}})));\n"
    );
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../npm/src/wallet-kinds.js");
    if std::env::var_os("ALLOWIT_WRITE_WALLET_KINDS").is_some() {
        std::fs::write(path, &module).unwrap();
    }
    assert_eq!(std::fs::read_to_string(path).unwrap(), module);
    let ids: Vec<_> = allowit_sdk::kinds::WALLET_KINDS
        .iter()
        .map(|k| k.id.id())
        .collect();
    assert_eq!(ids, ["native", "simple", "standard", "custom"]);
}
