//! The native Solana vault through the new commands: `policy new`,
//! `keys import` of an owner-issued executor bundle, `wallet show` and
//! `wallet create`. No Node, signer or network unless a test supplies one.
#![cfg(unix)]
mod common;

use allowit_native::{client::LOADER, crypto::Key, policy::Policy, release};
use common::{Mock, Sandbox, code, read_request, reply, stderr, stdout};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;

const PROMPT: &str = "Spend up to 5 test tokens per day";

fn mode(path: &std::path::Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// An executor bundle that an owner exports after deploying a vault.
fn executor_bundle(network: &str, custody: u8) -> Value {
    let p = Policy::generate(network, PROMPT).unwrap();
    let program = Key([2; 32]);
    let policy_data = Key::find_program_address(&[&program.0], Key::parse(LOADER).unwrap())
        .unwrap()
        .0;
    let owner = Key::parse("AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9").unwrap();
    let executor = Key::parse("9hSR6S7WPtxmTojgo6GG3k4yDPecgJY292j7xrsUGWBu").unwrap();
    let authority = Key::parse("GyGKxMyg1p9SsHfm15MkNUu1u9TN2JtTspcdmrtGUdse").unwrap();
    json!({"version":2,"policy":p,"context":{"policyId":p.id,"owner":owner,"network":p.network,
        "mint":Key([4;32]),"executor":executor,"authority":authority,
        "deployment":{"network":p.network,"sourceBundle":release().source_bundle,
            "policy":program,"policyData":policy_data,"custody":Key([custody;32])}}})
}

fn isolated(name: &str) -> Sandbox {
    let mut s = Sandbox::new(name);
    // A Node runtime or SDK script must never be needed.
    s.set("PATH", "")
        .set("ALLOWIT_NODE", "/missing/node")
        .set("ALLOWIT_SDK_CLI", "/missing/cli.mjs");
    s
}

#[test]
fn policy_new_is_offline_and_never_replaces_the_policy() {
    let mut sandbox = isolated("solana-new");
    sandbox
        .set("ALLOWIT_MINT", "unused-invalid-mint")
        .set("ALLOWIT_EXECUTOR", "unused-invalid-executor")
        .set("ALLOWIT_AUTHORITY", "unused-invalid-authority")
        .set("ALLOWIT_RPC_URL", "unused-invalid-rpc");
    let policy: Policy = serde_json::from_value(sandbox.json(&[
        "--rail",
        "solana:testnet",
        "policy",
        "new",
        PROMPT,
    ]))
    .unwrap();
    policy.validate().unwrap();
    let file = sandbox.work().join(".allowit/policy.json");
    let before = std::fs::read(&file).unwrap();
    assert_eq!(mode(&file), 0o600);
    let retry = sandbox.run(&[
        "--rail",
        "solana:testnet",
        "policy",
        "new",
        "Spend up to 6 test tokens per day",
    ]);
    assert_eq!(code(&retry), Some(3));
    assert_eq!(std::fs::read(&file).unwrap(), before);
    // Text output is the generated Rust source alone.
    let text = isolated("solana-text").ok(&["--rail", "solana:devnet", "policy", "new", PROMPT]);
    assert!(text.contains("fn "), "{text}");
    assert!(!text.contains("Saved"));
}

#[test]
fn explicit_devnet_selects_native_generation_without_rpc() {
    let mut sandbox = isolated("solana-devnet");
    let mock = Mock::new();
    sandbox.set("ALLOWIT_RPC_URL", &mock.url);
    let policy = sandbox.json(&[
        "--rail",
        "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
        "policy",
        "new",
        PROMPT,
    ]);
    assert!(
        matches!(
            policy["network"].as_str(),
            Some("solana:devnet" | "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1")
        ),
        "{policy}"
    );
    mock.assert_untouched();
    // Mainnet has no native lifecycle in the vendor yet.
    let output = sandbox.run(&["--rail", "solana:mainnet", "policy", "new", PROMPT]);
    assert_eq!(code(&output), Some(3));
}

#[test]
fn keys_import_keeps_the_bundle_public_and_never_replaces_an_occupied_policy() {
    let sandbox = isolated("solana-import");
    let bundle = executor_bundle("solana:testnet", 6);
    let source = sandbox.work().join("executor.json");
    std::fs::write(&source, serde_json::to_vec(&bundle).unwrap()).unwrap();
    let imported = sandbox.json(&[
        "--rail",
        "solana:testnet",
        "keys",
        "import",
        "executor",
        source.to_str().unwrap(),
    ]);
    assert_eq!(imported["imported"], true);
    let key = sandbox.json(&["keys", "show", "executor"]);
    assert_eq!(
        key["address"],
        "9hSR6S7WPtxmTojgo6GG3k4yDPecgJY292j7xrsUGWBu"
    );
    assert_eq!(key["secret"], false);
    let context = sandbox.work().join(".allowit/context.json");
    let saved = std::fs::read(&context).unwrap();
    let again = sandbox.run(&[
        "--rail",
        "solana:testnet",
        "keys",
        "import",
        "executor2",
        source.to_str().unwrap(),
    ]);
    assert_eq!(code(&again), Some(3));
    assert_eq!(std::fs::read(&context).unwrap(), saved);
    let output = sandbox.run(&["keys", "show", "executor2"]);
    assert_eq!(code(&output), Some(3), "a failed import left a key");
    // The audit capability is stored privately and never printed.
    let audited = isolated("solana-audit");
    let token = format!("native-report.{}", "a".repeat(32));
    let mut bundle = bundle;
    bundle["audit"] = json!({"origin":"https://staging.example.invalid","token":token});
    let source = audited.work().join("executor.json");
    std::fs::write(&source, serde_json::to_vec(&bundle).unwrap()).unwrap();
    // The bundle arrives on stdin, as from a password manager.
    let output = audited
        .command()
        .args([
            "--rail",
            "solana:testnet",
            "--json",
            "keys",
            "import",
            "executor",
            "-",
        ])
        .stdin(std::fs::File::open(&source).unwrap())
        .output()
        .unwrap();
    assert_eq!(code(&output), Some(0), "{}", stderr(&output));
    assert!(!stdout(&output).contains(&token));
    assert!(!stderr(&output).contains(&token));
    let audit = audited.work().join(".allowit/journal/audit.json");
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&audit).unwrap()).unwrap()["token"],
        token
    );
    assert_eq!(mode(&audit), 0o600);
    // The temporary copy of stdin is gone.
    for entry in std::fs::read_dir(audited.config()).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        assert!(!name.starts_with(".import-"), "{name}");
    }
}

#[test]
fn wallet_show_uses_the_public_context_without_loading_any_signing_key() {
    let mut sandbox = isolated("solana-show");
    let bundle = executor_bundle("solana:testnet", 7);
    let source = sandbox.work().join("executor.json");
    std::fs::write(&source, bundle.to_string()).unwrap();
    sandbox.ok(&["rail", "use", "solana:testnet"]);
    sandbox.ok(&["keys", "import", "executor", source.to_str().unwrap()]);
    let mock = Mock::new();
    let listener = mock.listener;
    let server = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let (_, request) = read_request(&mut conn);
        assert_eq!(request["method"], "getGenesisHash");
        reply(
            &mut conn,
            200,
            &json!({"jsonrpc":"2.0","id":request["id"],"result":"wrong-genesis"}),
        );
    });
    sandbox
        .set("ALLOWIT_OWNER_KEYPAIR", "/nonexistent-owner-key")
        .set("ALLOWIT_EXECUTOR_KEYPAIR", "/nonexistent-executor-key");
    let result = sandbox.run(&["--json", "--rpc-url", &mock.url, "wallet", "show"]);
    assert_eq!(code(&result), Some(3));
    let error = stderr(&result);
    assert!(error.contains("genesis"), "{error}");
    assert!(!error.contains("key file"));
    server.join().unwrap();
}

#[test]
fn wallet_create_prints_the_bound_owner_link_without_an_owner_key_or_rpc() {
    let mut sandbox = isolated("solana-create");
    let mock = Mock::new();
    sandbox.ok(&["rail", "use", "solana:devnet"]);
    let policy = sandbox.work().join("policy.json");
    sandbox.ok(&["policy", "new", PROMPT, "-o", policy.to_str().unwrap()]);
    assert!(policy.exists());
    let manifest = sandbox.work().join("deployment.json");
    std::fs::write(
        &manifest,
        serde_json::to_vec(
            &json!({"network":"solana:devnet","sourceBundle":release().source_bundle,
            "policy":Key([7;32]),"policyData":Key([8;32]),"custody":Key([9;32])}),
        )
        .unwrap(),
    )
    .unwrap();
    sandbox.ok(&[
        "keys",
        "add",
        "agent",
        "--public-key",
        &Key([2; 32]).to_string(),
    ]);
    sandbox
        .set("ALLOWIT_URL", "https://app.allowit.xyz")
        .set("ALLOWIT_MINT", Key([4; 32]).to_string())
        .set("ALLOWIT_AUTHORITY", Key([3; 32]).to_string())
        .set("ALLOWIT_DEPLOYMENT_FILE", &manifest)
        .set("ALLOWIT_OWNER_KEYPAIR", "/must-never-be-opened")
        .set("ALLOWIT_RPC_URL", &mock.url);
    let owner = Key([1; 32]).to_string();
    let create = [
        "--json",
        "wallet",
        "create",
        "--asset",
        "USDC",
        "--policy",
        policy.to_str().unwrap(),
        "--owner",
        &owner,
        "--agent",
        "agent",
        "--fund",
        "USDC:1",
    ];
    let output = sandbox.run(&create);
    assert_eq!(code(&output), Some(10), "{}", stderr(&output));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["executed"], false);
    assert_eq!(value["state"], "awaiting_owner_signature");
    assert_eq!(value["intentDigest"].as_str().unwrap().len(), 64);
    assert!(
        value["signingUrl"]
            .as_str()
            .unwrap()
            .contains("#allowit-deploy=")
    );
    // A retry of the same creation is the same intent.
    let again = sandbox.run(&create);
    assert_eq!(code(&again), Some(10), "{}", stderr(&again));
    let repeated: Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(repeated["intentDigest"], value["intentDigest"]);
    mock.assert_untouched();
    // --kind applies to Stellar wallets; local signing is not wired for creation.
    let mut kind = create.to_vec();
    kind.extend(["--kind", "simple"]);
    assert_eq!(code(&sandbox.run(&kind)), Some(2));
    let mut build = create.to_vec();
    build.push("--build-only");
    assert_eq!(code(&sandbox.run(&build)), Some(3));
}

#[test]
fn owner_verbs_need_a_local_key_and_a_supported_parameter() {
    let sandbox = isolated("solana-owner");
    sandbox.ok(&["rail", "use", "solana:devnet"]);
    for args in [
        vec!["wallet", "fund", "USDC:1"],
        vec!["wallet", "fund", "USDC:1", "--build-only"],
        vec!["wallet", "pause", "--sign-with-link"],
        vec!["wallet", "resume", "--sign-with-key", "owner"],
        vec!["wallet", "update", "--agent", "owner"],
        vec!["policy", "params", "set", "call_cap=5"],
        vec!["policy", "deploy", "policy.rs"],
    ] {
        let output = sandbox.run(&args);
        assert_eq!(code(&output), Some(3), "{args:?}: {}", stderr(&output));
    }
    // The vault holds only its token.
    sandbox.ok(&[
        "keys",
        "add",
        "owner",
        "--public-key",
        "AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9",
    ]);
    let output = sandbox.run(&["wallet", "fund", "SOL:1", "--sign-with-key", "owner"]);
    assert_eq!(code(&output), Some(3), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("cannot sign"),
        "{}",
        stderr(&output)
    );
    let output = sandbox.run(&["policy", "params", "set", "daily_limit=USDC:3"]);
    assert_eq!(code(&output), Some(2));
    assert!(stderr(&output).contains("transaction_cap"));
}
