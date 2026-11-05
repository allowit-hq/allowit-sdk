use allowit_sdk_engine::{
    generation::{self, GenerateRequest, Target},
    wallet,
};
use std::sync::atomic::{AtomicU64, Ordering};

#[test]
fn another_network_stops_before_owner_clock_or_transport() {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().canonicalize().unwrap().join(format!(
        "allowit-generic-owner-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let path = dir.join("owner.key");
    wallet::generate_wallet("stellar:testnet", &path).unwrap();
    let signer = wallet::local_signer("stellar:testnet", &path).unwrap();
    let request = GenerateRequest {
        prompt: "Use only the approved recipient".into(),
        network: "stellar:pubnet".into(),
        profile: "advertised-profile".into(),
        target: Target::NewWallet,
    };
    let mut clock_calls = 0;
    let result = generation::generate(
        "invalid backend",
        "invalid origin",
        &signer,
        &request,
        || {
            clock_calls += 1;
            Ok(1)
        },
    );
    assert!(result.is_err());
    assert_eq!(clock_calls, 0);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_owner_document_stops_before_opening_the_provider_store() {
    use allowit_sdk_engine::{config::ServerAuthority, owner};
    let dir = tempfile::tempdir().unwrap();
    let directory = dir.path().canonicalize().unwrap();
    let path = directory.join("keys").join("owner.key");
    let public = wallet::generate_wallet("stellar:testnet", &path).unwrap();
    let signer = wallet::local_signer("stellar:testnet", &path).unwrap();
    let root = directory.join("unopened");
    let authority =
        ServerAuthority::new("stellar:testnet", "http://localhost:8080", &public.address).unwrap();
    let cx = owner::Context {
        network: "stellar:testnet",
        backend: authority.origin(),
        authority: &authority,
        origin: "http://localhost:5173",
        rpc_url: "http://localhost:8000",
        root: &root,
        signer: &signer,
        clock: &|| panic!("Invalid documents must not start owner authentication"),
    };
    let error = owner::fund(&cx, b"{}", "invalid-doc-001", "1", "100").unwrap_err();
    assert!(error.to_string().contains("Invalid public owner document"));
    assert!(
        !root.exists(),
        "Native preflight must precede caller-owned storage effects"
    );
}
