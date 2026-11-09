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

