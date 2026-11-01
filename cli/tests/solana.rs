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
    let owner = Key::parse("AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9").unwrap();
    owned_bundle(network, custody, owner)
}

/// An executor bundle for a vault of OWNER.
fn owned_bundle(network: &str, custody: u8, owner: Key) -> Value {
    let p = Policy::generate(network, PROMPT).unwrap();
    let program = Key([2; 32]);
    let policy_data = Key::find_program_address(&[&program.0], Key::parse(LOADER).unwrap())
        .unwrap()
        .0;
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
    // The current commands keep the vault files in the config directory.
    let file = sandbox.config().join("vaults/solana-testnet/policy.json");
    assert!(!sandbox.work().join(".allowit").exists());
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
    let context = sandbox.config().join("vaults/solana-testnet/context.json");
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
    // A bundle of another network writes nothing under the selected rail.
    let other = isolated("solana-import-network");
    let source = other.work().join("executor.json");
    std::fs::write(
        &source,
        serde_json::to_vec(&executor_bundle("solana:devnet", 6)).unwrap(),
    )
    .unwrap();
    let output = other.run(&[
        "--rail",
        "solana:testnet",
        "keys",
        "import",
        "executor",
        source.to_str().unwrap(),
    ]);
    assert_eq!(code(&output), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("belongs to solana:devnet"));
    assert!(!other.config().join("vaults").exists());
    assert_eq!(code(&other.run(&["keys", "show", "executor"])), Some(3));
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
    let audit = audited
        .config()
        .join("vaults/solana-testnet/journal/audit.json");
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
    let mint = Key([4; 32]).to_string();
    // The rail's USDC is not the configured mint: a named asset or amount
    // never creates a vault for another token.
    for (asset, fund) in [("USDC", "1000000"), (mint.as_str(), "USDC:1")] {
        let output = sandbox.run(&[
            "wallet",
            "create",
            "--asset",
            asset,
            "--policy",
            policy.to_str().unwrap(),
            "--owner",
            &owner,
            "--agent",
            "agent",
            "--fund",
            fund,
        ]);
        assert_eq!(
            code(&output),
            Some(4),
            "{asset} {fund}: {}",
            stderr(&output)
        );
        assert!(stderr(&output).contains(&mint), "{}", stderr(&output));
    }
    let create = [
        "--json",
        "wallet",
        "create",
        "--asset",
        &mint,
        "--policy",
        policy.to_str().unwrap(),
        "--owner",
        &owner,
        "--agent",
        "agent",
        "--fund",
        "1000000",
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
    // A new ID replaces an expired unsigned preparation and binds the prior
    // one; the original journal entry stays.
    let mut replace = create.to_vec();
    replace.extend(["--request-id", "create-retry-0001"]);
    let output = sandbox.run(&replace);
    assert_eq!(code(&output), Some(10), "{}", stderr(&output));
    let replaced: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(replaced["requestId"], "create-retry-0001");
    assert_ne!(replaced["intentDigest"], value["intentDigest"]);
    let journal = sandbox.config().join("vaults/solana-devnet/journal");
    let prior = format!(
        "browser-deploy-{}.json",
        value["requestId"].as_str().unwrap()
    );
    assert!(journal.join(&prior).exists(), "{prior}");
    assert!(
        journal
            .join("browser-deploy-create-retry-0001.json")
            .exists()
    );
    // Each later ID replaces the preceding preparation, not the first one.
    let mut next = create.to_vec();
    next.extend(["--request-id", "create-retry-0002"]);
    let output = sandbox.run(&next);
    assert_eq!(code(&output), Some(10), "{}", stderr(&output));
    let second: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(replaced_of(&second), Some("create-retry-0001".to_owned()));
    assert_eq!(
        replaced_of(&replaced),
        value["requestId"].as_str().map(str::to_owned)
    );
    // A repeat keeps the predecessor that its intent binds.
    let output = sandbox.run(&replace);
    assert_eq!(code(&output), Some(10), "{}", stderr(&output));
    let repeat: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(repeat["intentDigest"], replaced["intentDigest"]);
    // A creation that starts with an explicit ID is replaceable too.
    let mut first = create.to_vec();
    let last = first.len() - 1;
    first[last] = "2000000";
    let mut explicit = first.clone();
    explicit.extend(["--request-id", "explicit-0001"]);
    let output = sandbox.run(&explicit);
    assert_eq!(code(&output), Some(10), "{}", stderr(&output));
    let opened: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(replaced_of(&opened), None);
    let mut later = first.clone();
    later.extend(["--request-id", "explicit-0002"]);
    let output = sandbox.run(&later);
    assert_eq!(code(&output), Some(10), "{}", stderr(&output));
    let later: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(replaced_of(&later), Some("explicit-0001".to_owned()));
    // The vault directory keeps the deployed policy for the later vault
    // commands, and a creation with another policy is refused.
    let vault = sandbox.config().join("vaults/solana-devnet");
    assert_eq!(
        std::fs::read(vault.join("policy.json")).unwrap(),
        std::fs::read(&policy).unwrap()
    );
    assert_eq!(mode(&vault.join("policy.json")), 0o600);
    let other = sandbox.work().join("other.json");
    sandbox.ok(&[
        "policy",
        "new",
        "Spend up to 6 test tokens per day",
        "-o",
        other.to_str().unwrap(),
    ]);
    let mut different = create.to_vec();
    different[6] = other.to_str().unwrap();
    let output = sandbox.run(&different);
    assert_eq!(code(&output), Some(3), "{}", stderr(&output));
    // A request ID cannot name the latest-preparation pointer of a fresh
    // creation, so the intent survives and its retry repeats it.
    let mut fresh = create.to_vec();
    fresh[last] = "3000000";
    let output = sandbox.run(&fresh);
    assert_eq!(code(&output), Some(10), "{}", stderr(&output));
    let derived: Value = serde_json::from_slice(&output.stdout).unwrap();
    std::fs::remove_dir_all(vault.join("journal")).unwrap();
    let pointer = format!("latest-{}", derived["requestId"].as_str().unwrap());
    fresh.extend(["--request-id", pointer.as_str()]);
    let output = sandbox.run(&fresh);
    assert_eq!(code(&output), Some(10), "{}", stderr(&output));
    let named: Value = serde_json::from_slice(&output.stdout).unwrap();
    let output = sandbox.run(&fresh);
    assert_eq!(code(&output), Some(10), "{}", stderr(&output));
    let again: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(again["intentDigest"], named["intentDigest"]);
    let mut short = create.to_vec();
    short.extend(["--request-id", "short"]);
    assert_eq!(code(&sandbox.run(&short)), Some(2));
    mock.assert_untouched();
    // --kind applies to Stellar wallets; local signing is not wired for creation.
    let mut kind = create.to_vec();
    kind.extend(["--kind", "simple"]);
    assert_eq!(code(&sandbox.run(&kind)), Some(2));
    let mut build = create.to_vec();
    build.push("--build-only");
    assert_eq!(code(&sandbox.run(&build)), Some(3));
    // No adapter waits for the owner yet; --wait is refused, never dropped.
    let mut wait = create.to_vec();
    wait.extend(["--wait", "5m"]);
    let output = sandbox.run(&wait);
    assert_eq!(code(&output), Some(3), "{}", stderr(&output));
    assert!(stderr(&output).contains("--wait"), "{}", stderr(&output));
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

#[test]
fn vault_operations_refuse_a_selected_wallet_of_another_vault() {
    let sandbox = isolated("solana-wallet");
    sandbox.ok(&["rail", "use", "solana:testnet"]);
    let owner = sandbox.json(&["keys", "generate", "owner"]);
    let owner = Key::parse(owner["address"].as_str().unwrap()).unwrap();
    let bundle = owned_bundle("solana:testnet", 7, owner);
    let source = sandbox.work().join("executor.json");
    std::fs::write(&source, bundle.to_string()).unwrap();
    sandbox.ok(&["keys", "import", "executor", source.to_str().unwrap()]);
    let id = allowit_native::client::hex32(bundle["policy"]["id"].as_str().unwrap()).unwrap();
    let vault = Key::find_program_address(&[b"allowit-vault-v2", &owner.0, &id], Key([7; 32]))
        .unwrap()
        .0
        .to_string();
    let other = Key([5; 32]).to_string();
    let mock = Mock::new();
    for args in [
        vec!["wallet", "show"],
        vec!["wallet", "fund", "1000000", "--sign-with-key", "owner"],
        vec!["wallet", "withdraw", "1000000", "--sign-with-key", "owner"],
        vec!["wallet", "pause", "--sign-with-key", "owner"],
        vec!["wallet", "close", "--sign-with-key", "owner", "--yes"],
        vec![
            "policy",
            "params",
            "set",
            "day_spent_cap=3000000",
            "--sign-with-key",
            "owner",
        ],
    ] {
        let mut all = vec!["--rpc-url", mock.url.as_str(), "--wallet", other.as_str()];
        all.extend(&args);
        let output = sandbox.run(&all);
        assert_eq!(code(&output), Some(3), "{args:?}: {}", stderr(&output));
        assert!(
            stderr(&output).contains(&format!("is for vault {vault}, not {other}")),
            "{args:?}: {}",
            stderr(&output)
        );
        // `wallet use` selects the same way.
        sandbox.ok(&["wallet", "use", &other]);
        let mut used = vec!["--rpc-url", mock.url.as_str()];
        used.extend(&args);
        assert_eq!(code(&sandbox.run(&used)), Some(3), "{args:?}");
        sandbox.ok(&["wallet", "use", &vault]);
    }
    // The imported context names the vault mint; the rail's USDC is another.
    let output = sandbox.run(&[
        "--rpc-url",
        &mock.url,
        "wallet",
        "fund",
        "USDC:1",
        "--sign-with-key",
        "owner",
    ]);
    assert_eq!(code(&output), Some(4), "{}", stderr(&output));
    mock.assert_untouched();
    // The matching wallet passes the check and reaches the RPC.
    let listener = mock.listener;
    let server = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let (_, request) = read_request(&mut conn);
        reply(
            &mut conn,
            200,
            &json!({"jsonrpc":"2.0","id":request["id"],"result":"wrong-genesis"}),
        );
    });
    let output = sandbox.run(&["--rpc-url", &mock.url, "--wallet", &vault, "wallet", "show"]);
    assert_eq!(code(&output), Some(3));
    assert!(stderr(&output).contains("genesis"), "{}", stderr(&output));
    server.join().unwrap();
    // The engine harness names no vault: a selected wallet is refused before
    // a request ID is spent.
    for args in [
        vec![
            "exec",
            "--op",
            "transfer",
            "--to",
            "9hSR6S7WPtxmTojgo6GG3k4yDPecgJY292j7xrsUGWBu",
            "--amount",
            "USDC:1",
        ],
        vec!["policy", "show"],
        vec!["status", "pay-00000001"],
    ] {
        let output = sandbox.run(&args);
        assert_eq!(code(&output), Some(3), "{args:?}: {}", stderr(&output));
        let error = stderr(&output);
        assert!(error.contains("cannot confirm wallet"), "{args:?}: {error}");
        assert_eq!(error.lines().count(), 1, "{args:?}: {error}");
    }
    // No owner verb waits for the owner yet.
    let output = sandbox.run(&[
        "wallet",
        "pause",
        "--sign-with-key",
        "owner",
        "--wait",
        "1m",
    ]);
    assert_eq!(code(&output), Some(3), "{}", stderr(&output));
    assert!(stderr(&output).contains("--wait"), "{}", stderr(&output));
    // A parameter change takes a new request ID; a retry passes the printed
    // one. Each change runs as ID.KEY, which must fit 100 characters.
    let set = [
        "--rpc-url",
        "http://127.0.0.1:1",
        "policy",
        "params",
        "set",
        "day_spent_cap=3000000",
        "--sign-with-key",
        "owner",
    ];
    let output = sandbox.run(&set);
    let first = stderr(&output)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    assert!(first.starts_with("req-"), "{}", stderr(&output));
    assert_ne!(
        stderr(&sandbox.run(&set)).lines().next(),
        Some(first.as_str())
    );
    let mut retry = set.to_vec();
    retry.extend(["--request-id", "caps-0001"]);
    assert!(!stderr(&sandbox.run(&retry)).contains("req-"));
    let long = "c".repeat(90);
    let mut too_long = set.to_vec();
    too_long.extend(["--request-id", long.as_str()]);
    let output = sandbox.run(&too_long);
    assert_eq!(code(&output), Some(2), "{}", stderr(&output));
    assert!(stderr(&output).contains("ID.KEY"), "{}", stderr(&output));
    // A refused signing mode prints no request ID.
    let output = sandbox.run(&["policy", "params", "set", "day_spent_cap=3000000"]);
    assert_eq!(code(&output), Some(3), "{}", stderr(&output));
    assert!(!stderr(&output).contains("req-"), "{}", stderr(&output));
}

/// The preparation that a printed creation intent replaces.
fn replaced_of(intent: &Value) -> Option<String> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let url = intent["signingUrl"].as_str().unwrap();
    let encoded = url.split("#allowit-deploy=").nth(1).unwrap();
    let payload: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).unwrap()).unwrap();
    payload["retryPreparationOf"].as_str().map(str::to_owned)
}
