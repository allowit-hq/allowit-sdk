//! Rail selection, keys and the public wallet profiles: the rail decides the
//! key curve, the address formats and the adapter, and a command never reads
//! a credential, a profile or the network that it does not need.
mod common;

use common::{Mock, Sandbox, code, read_request, reply, stderr, stdout};
use serde_json::{Value, json};

#[test]
fn rail_aliases_resolve_to_canonical_caip2_and_each_rail_keeps_its_own_settings() {
    let sandbox = Sandbox::new("rail");
    // No rail selected: a command that needs one exits 3 with a hint.
    let output = sandbox.run(&["keys", "generate", "agent"]);
    assert_eq!(code(&output), Some(3));
    assert!(
        stderr(&output).contains("allowit rail use"),
        "{}",
        stderr(&output)
    );
    for (alias, canonical) in [
        ("stellar:mainnet", "stellar:pubnet"),
        ("solana:devnet", "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1"),
        ("tempo:testnet", "eip155:42431"),
        ("eip155:42431", "eip155:42431"),
        ("stellar:testnet", "stellar:testnet"),
    ] {
        assert_eq!(
            sandbox.json(&["rail", "use", alias]),
            json!({"rail": canonical})
        );
        let shown = sandbox.json(&["rail", "show"]);
        assert_eq!(shown["rail"], canonical);
    }
    let output = sandbox.run(&["rail", "use", "stellar:futurenet"]);
    assert_eq!(code(&output), Some(2));
    assert!(stderr(&output).contains("allowit rail ls"));
    let rails = sandbox.json(&["rail", "ls"]);
    let current: Vec<_> = rails
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["current"] == true)
        .collect();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0]["rail"], "stellar:testnet");
    let text = sandbox.ok(&["rail", "ls"]);
    assert!(
        text.lines()
            .any(|l| l.starts_with('*') && l.contains("stellar:testnet"))
    );
    // A wallet chosen on one rail never becomes active on another.
    let native = allowit_stellar_wallet::asset_from_identity("XLM", None).unwrap();
    let contract = allowit_stellar_wallet::sac_address("stellar:testnet", &native).unwrap();
    sandbox.ok(&["wallet", "use", &contract]);
    sandbox.ok(&["rail", "use", "solana:devnet"]);
    assert_eq!(sandbox.json(&["whoami"])["wallet"], Value::Null);
    let output = sandbox.run(&["wallet", "use", &contract]);
    assert_eq!(code(&output), Some(2));
    assert!(
        stderr(&output).contains("is a stellar address, but the rail is solana:"),
        "{}",
        stderr(&output)
    );
    sandbox.ok(&["rail", "use", "stellar:testnet"]);
    assert_eq!(sandbox.json(&["whoami"])["wallet"], contract);
    let config = sandbox.json(&["config", "show"]);
    assert_eq!(config["rail"], "stellar:testnet");
    // Without --json the nested settings print as text lines.
    let text = sandbox.ok(&["config", "show"]);
    assert!(serde_json::from_str::<Value>(&text).is_err(), "{text}");
    assert!(
        text.lines()
            .any(|l| l.starts_with("config.rails.stellar:testnet.wallet") && l.ends_with(&contract)),
        "{text}"
    );
    assert_eq!(config["railSource"], "config.toml");
    assert_eq!(
        config["config"]["rails"]["stellar:testnet"]["wallet"],
        contract
    );
    assert_eq!(
        sandbox.ok(&["config", "path"]).trim(),
        sandbox.config().join("config.toml").to_str().unwrap()
    );
    // Flag, then environment, then config.toml.
    let mut sandbox = sandbox;
    sandbox.set("ALLOWIT_RAIL", "solana:devnet");
    assert_eq!(
        sandbox.json(&["rail", "show"])["rail"],
        "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1"
    );
    assert_eq!(
        sandbox.json(&["--rail", "tempo:testnet", "rail", "show"])["rail"],
        "eip155:42431"
    );
}

#[test]
fn keys_use_the_rail_curve_and_print_only_public_metadata() {
    let sandbox = Sandbox::new("keys");
    let tempo = sandbox.json(&["--rail", "eip155:42431", "keys", "generate", "executor"]);
    let mut fields: Vec<_> = tempo.as_object().unwrap().keys().collect();
    fields.sort();
    assert_eq!(
        fields,
        ["address", "curve", "name", "publicKey", "rail", "secret"]
    );
    assert_eq!(tempo["rail"], "eip155:42431");
    assert_eq!(tempo["curve"], "secp256k1");
    assert!(tempo["address"].as_str().unwrap().starts_with("0x"));
    assert_eq!(sandbox.json(&["keys", "show", "executor"]), tempo);
    let stellar = sandbox.json(&["--rail", "stellar:testnet", "keys", "generate", "agent"]);
    assert_eq!(stellar["curve"], "ed25519");
    let address = stellar["address"].as_str().unwrap();
    // Stellar keys print as G strkeys.
    assert!(address.starts_with('G') && address.len() == 56);
    assert_eq!(stellar["publicKey"], stellar["address"]);
    let solana = sandbox.json(&["--rail", "solana:devnet", "keys", "generate", "payer"]);
    assert_eq!(solana["curve"], "ed25519");
    assert_eq!(solana["publicKey"], solana["address"]);
    // Private keys stay in secrets/, mode 0600 in a 0700 directory.
    let secret = sandbox.ok(&["keys", "export", "agent"]);
    let secret = secret.trim();
    assert!(secret.starts_with('S') && secret.len() == 56);
    for name in ["executor", "agent", "payer"] {
        let metadata =
            std::fs::read_to_string(sandbox.config().join(format!("keys/{name}.toml"))).unwrap();
        assert!(!metadata.contains(secret));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode =
            |p: std::path::PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(sandbox.config().join("secrets")), 0o700);
        assert_eq!(mode(sandbox.config().join("secrets/agent")), 0o600);
    }
    let listed = sandbox.json(&["keys", "ls"]);
    assert_eq!(listed.as_array().unwrap().len(), 3);
    assert!(!listed.to_string().contains(secret));
    // A key from one chain family cannot act on another.
    let output = sandbox.run(&["--rail", "solana:devnet", "setup", "--agent", "agent"]);
    assert_eq!(code(&output), Some(3));
    assert!(stderr(&output).contains("belongs to stellar:testnet"));
    // Re-import of the exported key under another name; the address is the same.
    let file = sandbox.private_file("agent.key", secret);
    let copy = sandbox.json(&[
        "--rail",
        "stellar:testnet",
        "keys",
        "import",
        "copy",
        file.to_str().unwrap(),
    ]);
    assert_eq!(copy["address"], stellar["address"]);
    // keys add records a public address that cannot sign.
    let owner = sandbox.json(&[
        "--rail",
        "stellar:testnet",
        "keys",
        "add",
        "owner",
        "--public-key",
        address,
    ]);
    assert_eq!(owner["secret"], false);
    let bad = sandbox.run(&[
        "--rail",
        "stellar:testnet",
        "keys",
        "add",
        "bad",
        "--public-key",
        tempo["address"].as_str().unwrap(),
    ]);
    assert_eq!(code(&bad), Some(2));
    let output = sandbox.run(&["keys", "export", "owner"]);
    assert_eq!(code(&output), Some(3));
    // Removal is destructive: it needs --yes without a terminal.
    let output = sandbox.run(&["keys", "rm", "copy"]);
    assert_eq!(code(&output), Some(2));
    assert!(stderr(&output).contains("--yes"));
    sandbox.ok(&["keys", "rm", "copy", "--yes"]);
    assert!(!sandbox.config().join("secrets/copy").exists());
    assert_eq!(sandbox.json(&["keys", "ls"]).as_array().unwrap().len(), 4);
}

#[test]
fn faucet_funding_goes_through_the_rail_vendor() {
    let mut sandbox = Sandbox::new("fund");
    let mock = Mock::new();
    sandbox.set("ALLOWIT_FRIENDBOT_URL", &mock.url);
    sandbox.ok(&["rail", "use", "stellar:testnet"]);
    let key = sandbox.json(&["keys", "generate", "agent"]);
    let address = key["address"].as_str().unwrap().to_owned();
    let listener = mock.listener;
    let expected = address.clone();
    let server = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let (head, _) = common::read_request(&mut conn);
        assert!(
            head.starts_with(&format!("GET /?addr={expected} ")),
            "{head}"
        );
        common::reply(&mut conn, 200, &json!({"hash": "ab".repeat(32)}));
    });
    let funded = sandbox.json(&["keys", "fund", "agent"]);
    assert_eq!(funded["address"], address);
    assert_eq!(funded["transactionHash"], "ab".repeat(32));
    server.join().unwrap();
    // Mainnet has no faucet; Tempo has none in the vendor yet.
    for rail in ["stellar:pubnet", "tempo:testnet"] {
        let output = sandbox.run(&["--rail", rail, "keys", "fund", "agent"]);
        assert_eq!(code(&output), Some(3), "{rail}: {}", stderr(&output));
        // generate --fund refuses before the key exists, so a retry is clean.
        let output = sandbox.run(&["--rail", rail, "--json", "keys", "generate", "pk", "--fund"]);
        assert_eq!(code(&output), Some(3), "{rail}: {}", stderr(&output));
        assert!(output.stdout.is_empty(), "{rail}");
        assert_ne!(
            code(&sandbox.run(&["keys", "show", "pk"])),
            Some(0),
            "{rail}"
        );
    }
}

#[test]
fn setup_keeps_the_exit_code_of_a_registration_that_did_not_complete() {
    let mut sandbox = Sandbox::new("setup-denied");
    let mock = Mock::new();
    sandbox.ok(&["rail", "use", "stellar:testnet"]);
    sandbox.ok(&["keys", "generate", "agent"]);
    sandbox
        .set("ALLOWIT_ACCOUNT_TOKEN", "account-capability-0123456789")
        .set("ALLOWIT_URL", &mock.url);
    let listener = mock.listener;
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut conn, _) = listener.accept().unwrap();
            let (head, request) = common::read_request(&mut conn);
            assert!(head.starts_with("POST /api/gateway/setup "), "{head}");
            common::reply(
                &mut conn,
                200,
                &json!({"operationId": request["operationId"], "state": "denied"}),
            );
        }
    });
    let output = sandbox.run(&["--json", "setup", "--agent", "agent"]);
    assert_eq!(code(&output), Some(20), "{}", stderr(&output));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["registered"], false);
    assert_eq!(value["registration"]["state"], "denied");
    // Text mode: data lines, and the operation ID alone on stderr, also
    // with -q.
    let output = sandbox.run(&["-q", "setup", "--agent", "agent"]);
    server.join().unwrap();
    assert_eq!(code(&output), Some(20), "{}", stderr(&output));
    common::assert_data(&stdout(&output), "setup");
    assert!(
        stdout(&output)
            .lines()
            .any(|l| l.starts_with("registration.state ") && l.ends_with("  denied")),
        "{}",
        stdout(&output)
    );
    let id = stderr(&output);
    assert_eq!(id.lines().count(), 1, "{id}");
    assert!(id.trim().starts_with("allowit-"), "{id}");
}

#[test]
fn cross_rail_input_fails_before_any_settings_bundle_or_network_access() {
    let mut sandbox = Sandbox::new("cross");
    let mock = Mock::new();
    sandbox
        .set("ALLOWIT_URL", &mock.url)
        .set("ALLOWIT_TEMPO_BUNDLE", "/must-not-be-read")
        .set("ALLOWIT_STELLAR_BUNDLE", "/must-not-be-read");
    let solana = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    let stellar = "GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5";
    for (args, needle) in [
        (
            vec![
                "--rail",
                "eip155:42431",
                "exec",
                "--op",
                "transfer",
                "--to",
                solana,
                "--amount",
                "1",
            ],
            "not a tempo recipient",
        ),
        (
            vec![
                "--rail",
                "stellar:testnet",
                "exec",
                "--op",
                "transfer",
                "--to",
                solana,
                "--amount",
                "XLM:1",
            ],
            "not a stellar recipient",
        ),
        (
            vec![
                "--rail",
                "stellar:testnet",
                "exec",
                "--op",
                "transfer",
                "--to",
                stellar,
                "--amount",
                "SOL:1",
            ],
            "SOL",
        ),
        (
            vec![
                "--rail",
                "solana:devnet",
                "exec",
                "--op",
                "transfer",
                "--to",
                solana,
                "--amount",
                "USDC:1.0000001",
            ],
            "more than 6 decimal places",
        ),
    ] {
        let output = sandbox.run(&args);
        assert_eq!(code(&output), Some(2), "{args:?}: {}", stderr(&output));
        assert!(
            stderr(&output).contains(needle),
            "{args:?}: {}",
            stderr(&output)
        );
    }
    assert!(!sandbox.config().exists(), "input errors created settings");
    let output = sandbox.run(&[
        "--rail",
        "stellar:testnet",
        "--wallet",
        "0x00",
        "wallet",
        "show",
    ]);
    assert_eq!(code(&output), Some(2));
    mock.assert_untouched();
}

#[test]
fn local_commands_never_read_broken_rail_or_wallet_settings() {
    let mut sandbox = Sandbox::new("local");
    std::fs::create_dir_all(sandbox.config()).unwrap();
    std::fs::write(sandbox.config().join("config.toml"), "rail = [broken").unwrap();
    sandbox
        .set("ALLOWIT_RAIL", "not-a-rail")
        .set("ALLOWIT_WALLET", "not-a-wallet")
        .set("ALLOWIT_TEMPO_BUNDLE", "/must-not-be-read")
        .set("ALLOWIT_STELLAR_BUNDLE", "/must-not-be-read");
    let policy = format!(
        "{}/../docs/examples/research.rs",
        env!("CARGO_MANIFEST_DIR")
    );
    for args in [
        vec!["help"],
        vec!["--help"],
        vec!["version"],
        vec!["guide"],
        vec!["completion", "--shell", "bash"],
        vec!["policy", "check", &policy],
        vec!["policy", "functions"],
    ] {
        let output = sandbox.run(&args);
        assert_eq!(code(&output), Some(0), "{args:?}: {}", stderr(&output));
    }
    // A command that needs the configuration reports it.
    let output = sandbox.run(&["keys", "ls"]);
    assert_eq!(code(&output), Some(3));
    assert!(stderr(&output).contains("config.toml is invalid"));
    let output = sandbox.run(&["rail", "show"]);
    assert_eq!(code(&output), Some(2));
    assert!(stderr(&output).contains("unknown rail"));
}

#[test]
fn json_errors_are_one_object_on_stderr() {
    let sandbox = Sandbox::new("json");
    let output = sandbox.run(&["--json", "keys", "show", "missing"]);
    assert_eq!(code(&output), Some(3));
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "config");
    assert_eq!(error["error"]["exit"], 3);
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("missing")
    );
    let output = sandbox.run(&["keys", "show", "missing"]);
    assert_eq!(
        stderr(&output),
        "error: no key \"missing\"; run allowit keys ls\n"
    );
}

#[test]
fn verbs_without_a_backend_exit_3_as_not_available() {
    let mut sandbox = Sandbox::new("unavailable");
    let mock = Mock::new();
    sandbox
        .set("ALLOWIT_URL", &mock.url)
        .set("ALLOWIT_TOKEN", "t".repeat(40));
    for (rail, args) in [
        ("stellar:testnet", vec!["wallet", "close", "--yes"]),
        ("stellar:testnet", vec!["wallet", "pause"]),
        ("stellar:testnet", vec!["tx", "send", "-"]),
        ("solana:devnet", vec!["wallet", "update", "--x402", "on"]),
        ("eip155:42431", vec!["policy", "params"]),
        (
            "stellar:pubnet",
            vec!["policy", "new", "Spend up to 5 USDC per day"],
        ),
        // The engine serves no paid-service route, so nothing is sent.
        ("stellar:testnet", vec!["skills", "search", "weather"]),
        ("solana:devnet", vec!["call", "SERVICE_ID"]),
        ("eip155:42431", vec!["call", "SERVICE_ID"]),
        ("stellar:testnet", vec!["call", "https://api.example/v1"]),
        ("solana:devnet", vec!["curl", "https://api.example/v1"]),
        (
            "solana:devnet",
            vec!["http", "GET", "https://api.example/v1"],
        ),
        ("stellar:testnet", vec!["mcp"]),
        ("stellar:testnet", vec!["claude", "--", "-p", "hi"]),
        ("stellar:testnet", vec!["codex", "--", "exec", "hi"]),
    ] {
        let mut all = vec!["--rail", rail];
        all.extend(&args);
        let output = sandbox
            .command()
            .args(&all)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert_eq!(code(&output), Some(3), "{all:?}: {}", stderr(&output));
        assert!(
            stderr(&output).contains("not available on"),
            "{all:?}: {}",
            stderr(&output)
        );
        assert!(output.stdout.is_empty(), "{all:?}");
    }
    mock.assert_untouched();
    // rail show lists what works and what does not.
    let shown = sandbox.json(&["rail", "show", "stellar:testnet"]);
    let available = shown["available"].as_array().unwrap();
    assert!(available.contains(&json!("exec")));
    assert!(
        shown["notAvailable"]
            .as_array()
            .unwrap()
            .contains(&json!("wallet close"))
    );
    // Stellar has no `policy params set` backend, so it lists no keys.
    assert_eq!(shown["paramKeys"], json!([]));
    let solana = sandbox.json(&["rail", "show", "solana:devnet"]);
    assert_eq!(
        solana["paramKeys"],
        json!(["transaction_cap", "day_spent_cap"])
    );
}

fn stellar_approval_fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/stellar-approval.json")).unwrap()
}

fn public_stellar_profile(fixture: &Value) -> Value {
    use allowit_stellar_wallet::{self as sdk, WalletConfig, WalletRoute};
    let policy = &fixture["policy"];
    let config = WalletConfig {
        network: "stellar:testnet".into(),
        policy_instance: [7; 32],
        wallet: fixture["intent"]["owner"].as_str().unwrap().into(),
        owner: fixture["intent"]["owner"].as_str().unwrap().into(),
        agent: sdk::account_for_seed(&[2; 32]),
        server: sdk::account_for_seed(&[3; 32]),
        route: WalletRoute::Native,
        policy_engine: None,
        asset: policy["mint"].as_str().unwrap().into(),
        asset_symbol: "XLM".into(),
        asset_issuer: None,
        policy_source: policy["code"].as_str().unwrap().into(),
        source_hash: sdk::unhex(policy["sourceHash"].as_str().unwrap()).unwrap(),
        module_digest: [5; 32],
        max_fee_stroops: 100,
    };
    json!({"version":1,"profile":sdk::PROFILE,"network":config.network,
        "policyId":policy["id"],"sourceHash":policy["sourceHash"],"irHash":policy["irHash"],
        "moduleDigest":sdk::hex(&config.module_digest),"wallet":config.wallet,
        "route":"native","config":config,
        "endpoints":{"execute":"/api/native/execute","status":"/api/native/execute/status"}})
}

fn stellar_sandbox(profile: &Value) -> Sandbox {
    let mut sandbox = Sandbox::new("stellar-profile");
    let path = sandbox.work().join("public-stellar.json");
    std::fs::write(&path, serde_json::to_vec(profile).unwrap()).unwrap();
    sandbox
        .set("ALLOWIT_STELLAR_BUNDLE", &path)
        .set("ALLOWIT_STELLAR_SIGNER", "/must-never-be-opened")
        .set("ALLOWIT_URL", "not-a-server-origin");
    sandbox
}

#[test]
fn public_stellar_profile_shows_without_agent_key_or_engine() {
    let fixture = stellar_approval_fixture();
    let profile = public_stellar_profile(&fixture);
    let sandbox = stellar_sandbox(&profile);
    let shown = sandbox.json(&["--rail", "stellar:testnet", "wallet", "show"]);
    assert_eq!(shown["network"], "stellar:testnet");
    assert_eq!(shown["rail"], "stellar:testnet");
    assert_eq!(shown["agent"], profile["config"]["agent"]);
    assert_eq!(shown["wallet"], profile["wallet"]);
    assert!(shown.get("signingSeed").is_none());
    assert!(shown.get("endpoints").is_none());
    assert!(shown.get("policy").is_none());
    let summary = sandbox.json(&["--rail", "stellar:testnet", "policy", "show"]);
    assert_eq!(summary["policyId"], fixture["policy"]["id"]);
    assert_eq!(summary["asset"], "XLM");
    let source = sandbox.ok(&["--rail", "stellar:testnet", "policy", "show", "--source"]);
    assert_eq!(
        source.trim_end(),
        fixture["policy"]["code"].as_str().unwrap().trim_end()
    );
    assert!(!source.contains('\x1b'));
    // A wallet selection is a C… contract. The classic route's G account
    // needs no selection and cannot be selected.
    let classic = profile["wallet"].as_str().unwrap();
    assert!(classic.starts_with('G'));
    let output = sandbox.run(&[
        "--rail",
        "stellar:testnet",
        "--wallet",
        classic,
        "wallet",
        "show",
    ]);
    assert_eq!(code(&output), Some(2), "{}", stderr(&output));
    assert!(stderr(&output).contains("C… contract strkey"));
}

#[test]
fn rail_conflict_and_secret_material_in_a_public_profile_fail_before_transport() {
    let fixture = stellar_approval_fixture();
    let mut profile = public_stellar_profile(&fixture);
    let sandbox = stellar_sandbox(&profile);
    let output = sandbox.run(&["--rail", "stellar:pubnet", "wallet", "show"]);
    assert_eq!(code(&output), Some(3));
    assert!(stderr(&output).contains("belongs to stellar:testnet"));
    assert!(output.stdout.is_empty());
    let native = allowit_stellar_wallet::asset_from_identity("XLM", None).unwrap();
    let other = allowit_stellar_wallet::sac_address("stellar:testnet", &native).unwrap();
    let output = sandbox.run(&[
        "--rail",
        "stellar:testnet",
        "--wallet",
        &other,
        "wallet",
        "show",
    ]);
    assert_eq!(code(&output), Some(3), "{}", stderr(&output));
    assert!(stderr(&output).contains("the wallet profile is for"));
    profile["signingSeed"] = json!("must-remain-local");
    std::fs::write(
        sandbox.work().join("public-stellar.json"),
        serde_json::to_vec(&profile).unwrap(),
    )
    .unwrap();
    let output = sandbox.run(&["--rail", "stellar:testnet", "wallet", "show"]);
    assert_eq!(code(&output), Some(3));
    assert!(stderr(&output).contains("must not contain signing material"));
    assert!(output.stdout.is_empty());
    assert!(!stdout(&output).contains("must-remain-local"));
}

#[cfg(unix)]
#[test]
fn hidden_wallet_approve_exports_only_the_public_exact_intent() {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let fixture = stellar_approval_fixture();
    let mut sandbox = Sandbox::new("approve");
    let file = sandbox.work().join("policy.json");
    let mut policy = fixture["policy"].clone();
    policy["signingSeed"] = json!("must-remain-local");
    std::fs::write(&file, serde_json::to_vec(&policy).unwrap()).unwrap();
    let owner = fixture["intent"]["owner"].as_str().unwrap();
    let request = fixture["intent"]["requestId"].as_str().unwrap();
    sandbox
        .set("ALLOWIT_URL", "https://app.allowit.xyz")
        .set("ALLOWIT_OWNER_KEYPAIR", "/must-never-be-opened")
        .set("ALLOWIT_STELLAR_SIGNER", "/must-never-be-opened");
    let output = sandbox.run(&[
        "--network",
        "stellar:testnet",
        "wallet",
        "approve",
        "--policy-file",
        file.to_str().unwrap(),
        "--owner",
        owner,
        "--request-id",
        request,
    ]);
    assert_eq!(code(&output), Some(10), "{}", stderr(&output));
    let shown: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(shown["state"], "awaiting_owner_approval");
    assert_eq!(shown["executed"], false);
    assert_eq!(shown["movesFunds"], false);
    let url = shown["signingUrl"].as_str().unwrap();
    let encoded = url.split_once("#allowit-stellar-approval=").unwrap().1;
    let public: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).unwrap()).unwrap();
    assert_eq!(public, fixture["intent"]);
    assert!(!stdout(&output).contains("must-remain-local"));
    assert!(!stdout(&output).contains("must-never-be-opened"));
    // The link flow serves Stellar testnet only.
    let output = sandbox.run(&[
        "--network",
        "stellar:pubnet",
        "wallet",
        "approve",
        "--policy-file",
        file.to_str().unwrap(),
        "--owner",
        owner,
        "--request-id",
        request,
    ]);
    assert_eq!(code(&output), Some(3));
}

#[cfg(unix)]
#[test]
fn hidden_wallet_configure_keeps_an_expired_handoff_for_browser_recovery() {
    use allowit_stellar_wallet::{self as sdk, AccountState};
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut sandbox = Sandbox::new("configure");
    sandbox.set("ALLOWIT_URL", "https://app.allowit.xyz");
    let journal = sandbox.work().join(".allowit/stellar-owner-journal");
    std::fs::create_dir_all(&journal).unwrap();
    for path in [sandbox.work().join(".allowit"), journal.clone()] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let state = AccountState {
        address: sdk::account_for_seed(&[1; 32]),
        sequence: 10,
        master_weight: 1,
        low_threshold: 0,
        medium_threshold: 0,
        high_threshold: 0,
        signers: vec![],
    };
    let agent = sdk::account_for_seed(&[2; 32]);
    let engine = sdk::account_for_seed(&[3; 32]);
    let id = "cli-expired-stellar-setup-001";
    let original =
        sdk::owner::prepare_native_setup(&state, &agent, &engine, id, "stellar:testnet", 100, 1)
            .unwrap();
    let path = journal.join(format!("request-{:x}.json", Sha256::digest(id.as_bytes())));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    serde_json::to_writer(file, &original).unwrap();
    let original_bytes = std::fs::read(&path).unwrap();
    let configure = |agent: &str| {
        sandbox.run(&[
            "--rail",
            "stellar:testnet",
            "wallet",
            "configure",
            "--owner",
            &state.address,
            "--agent",
            agent,
            "--engine",
            &engine,
            "--request-id",
            id,
        ])
    };
    let output = configure(&agent);
    assert_eq!(code(&output), Some(10), "{}", stderr(&output));
    let shown: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(shown["state"], "owner_intent_expired");
    assert_eq!(shown["recoveryOnly"], true);
    assert_eq!(shown["executed"], false);
    assert_eq!(shown["transactionHash"], original.transaction_hash);
    let encoded = shown["signingUrl"]
        .as_str()
        .unwrap()
        .split_once("#allowit-stellar-owner=")
        .unwrap()
        .1;
    let recovered: sdk::owner::NativeSetup =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).unwrap()).unwrap();
    assert_eq!(recovered, original);
    assert_eq!(std::fs::read(&path).unwrap(), original_bytes);
    let conflict = configure(&sdk::account_for_seed(&[4; 32]));
    assert!(!conflict.status.success());
    assert_ne!(code(&conflict), Some(10));
    assert!(conflict.stdout.is_empty());
    assert!(stderr(&conflict).contains("already binds another intent"));
    assert_eq!(std::fs::read(&path).unwrap(), original_bytes);
}

#[cfg(unix)]
#[test]
fn tempo_bundle_serves_only_its_rail_and_vault() {
    let mut sandbox = Sandbox::new("tempo");
    // The bundle binds its service origin.
    sandbox.set("ALLOWIT_URL", "https://service.invalid");
    let addr = |n: u8| format!("0x{}", format!("{n:02x}").repeat(20));
    let policy_id = "0f".repeat(32);
    let bundle = json!({"version":1,"profile":"tempo-native-v1","origin":"https://service.invalid","capability":format!("tempo-executor.{}","a".repeat(32)),"config":{"network":"tempo:testnet","chainId":42431,"rpcUrl":"https://rpc.invalid","factory":addr(11),"factoryCodeHash":format!("0x{}","0c".repeat(32)),"token":addr(13),"executor":addr(6),"authority":addr(7),"feeToken":addr(14)},"policy":{"id":policy_id,"profile":"tempo-native-v1","network":"tempo:testnet","chainId":42431,"owner":addr(5),"token":addr(13),"executor":addr(6),"authority":addr(7),"factory":addr(11),"vault":addr(16),"policyDigest":format!("0x{}","11".repeat(32)),"requirementsDigest":format!("0x{}","12".repeat(32)),"semanticRequired":true,"prompt":"Pay for approved research","dailyLimit":"5","actionLimit":"1","totalLimit":"20","expiresAt":"18446744073709551615","recipient":addr(0)}});
    let source = sandbox.private_file("executor.json", &bundle.to_string());
    sandbox.ok(&["rail", "use", "tempo:testnet"]);
    // keys import prints the key view, as JSON with --json.
    let imported = sandbox.json(&["keys", "import", "executor", source.to_str().unwrap()]);
    assert_eq!(imported["policy"], policy_id);
    assert_eq!(imported["address"], addr(6));
    assert_eq!(imported["rail"], "eip155:42431");
    assert!(!imported.to_string().contains("tempo-executor."));
    let key = sandbox.json(&["keys", "show", "executor"]);
    assert_eq!(key["address"], addr(6));
    assert_eq!(key["secret"], false);
    for args in [["wallet", "show"], ["policy", "show"]] {
        let text = sandbox.ok(&args);
        common::assert_data(&text, &args.join(" "));
        assert!(text.contains("network"), "{text}");
    }
    // A second import finds the stored bundle: one error line, and none of
    // the crate's text.
    let output = sandbox.run(&["keys", "import", "executor2", source.to_str().unwrap()]);
    assert_eq!(code(&output), Some(3), "{}", stderr(&output));
    assert!(stdout(&output).is_empty(), "{}", stdout(&output));
    assert!(
        stderr(&output).starts_with("error: "),
        "{}",
        stderr(&output)
    );
    assert_eq!(stderr(&output).lines().count(), 1, "{}", stderr(&output));
    let shown = sandbox.json(&["wallet", "show"]);
    assert_eq!(shown["vault"], addr(16));
    assert_eq!(shown["network"], "eip155:42431");
    assert!(!shown.to_string().contains("tempo-executor."));
    let output = sandbox.run(&["--wallet", &addr(17), "wallet", "show"]);
    assert_eq!(code(&output), Some(3));
    assert!(stderr(&output).contains("is for vault"));
    let output = sandbox.run(&["--rail", "tempo:mainnet", "wallet", "show"]);
    assert_eq!(code(&output), Some(3));
    assert!(stderr(&output).contains("not available on eip155:4217"));
    // Paid calls and the policy follow the selected wallet too.
    let (other, recipient) = (addr(17), addr(9));
    for args in [
        vec![
            "call",
            "https://service.invalid/data",
            "--max-amount",
            "0.05",
        ],
        vec!["policy", "show"],
    ] {
        let mut all = vec!["--wallet", other.as_str()];
        all.extend(&args);
        let output = sandbox.run(&all);
        assert_eq!(code(&output), Some(3), "{args:?}: {}", stderr(&output));
        assert!(stderr(&output).contains("is for vault"), "{args:?}");
    }
    // status follows the wallet in the journal and refuses another one.
    sandbox.ok(&["wallet", "use", &addr(16)]);
    let output = sandbox.run(&[
        "exec",
        "--op",
        "transfer",
        "--to",
        &recipient,
        "--amount",
        "pathUSD:1",
        "--request-id",
        "tempo-pay-0001",
    ]);
    assert_eq!(code(&output), Some(4), "{}", stderr(&output));
    let output = sandbox.run(&["--wallet", &other, "status", "tempo-pay-0001"]);
    assert_eq!(code(&output), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("belongs to wallet"),
        "{}",
        stderr(&output)
    );
    // An amount of another token exits 4 before anything is sent.
    for args in [
        vec![
            "exec",
            "--op",
            "transfer",
            "--to",
            recipient.as_str(),
            "--amount",
            "pathUSD:1",
        ],
        vec![
            "exec",
            "--op",
            "transfer",
            "--to",
            recipient.as_str(),
            "--amount",
            "AlphaUSD:1",
            "--build-only",
        ],
        vec![
            "call",
            "https://service.invalid/data",
            "--max-amount",
            "pathUSD:0.05",
        ],
    ] {
        let output = sandbox.run(&args);
        assert_eq!(code(&output), Some(4), "{args:?}: {}", stderr(&output));
        assert!(
            stderr(&output).contains("names another asset"),
            "{args:?}: {}",
            stderr(&output)
        );
    }
    // A payment signs with the agent key that setup chose; without one it
    // stops before the crate runs.
    let pay = [
        "exec",
        "--op",
        "transfer",
        "--to",
        recipient.as_str(),
        "--amount",
        "1000000",
        "--request-id",
        "tempo-pay-0002",
    ];
    let output = sandbox.run(&pay);
    assert_eq!(code(&output), Some(3), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("setup --agent"),
        "{}",
        stderr(&output)
    );
    sandbox.ok(&["keys", "generate", "agent"]);
    sandbox.ok(&["setup", "--agent", "agent"]);
    // --rpc-url reaches the crate over an invalid ambient RPC setting, and
    // the crate's own progress lines never reach the terminal.
    sandbox.set("ALLOWIT_TEMPO_RPC_URL", "not a url");
    let mut overridden = vec!["--rpc-url", "http://127.0.0.1:1"];
    overridden.extend(pay);
    let output = sandbox.run(&overridden);
    assert_ne!(code(&output), Some(0));
    assert!(
        !stderr(&output).contains("Invalid Tempo RPC URL"),
        "{}",
        stderr(&output)
    );
    assert!(
        !stderr(&output).contains("evaluating the vault policy"),
        "{}",
        stderr(&output)
    );
    let mut build = pay.to_vec();
    build[8] = "tempo-plan-0001";
    build.push("--build-only");
    let output = sandbox.run(&build);
    assert!(
        !stderr(&output).contains("requesting a wallet plan"),
        "{}",
        stderr(&output)
    );
    let mut long = pay.to_vec();
    long.extend(["--wait", "11m"]);
    assert_eq!(code(&sandbox.run(&long)), Some(2));
    // eval is not available on Tempo, and nothing is sent.
    let output = sandbox.run(&[
        "eval",
        "--op",
        "transfer",
        "--to",
        &addr(9),
        "--amount",
        "AlphaUSD:1",
    ]);
    assert_eq!(code(&output), Some(3));
    // The hidden tempo group of earlier releases still reads the bundle.
    let output = sandbox.run(&["tempo", "status", "not-a-request"]);
    assert_ne!(code(&output), Some(0));
    assert!(!stderr(&output).contains("Unknown Tempo command"));
    // Help on a subverb prints the group usage.
    for args in [["tempo", "call", "--help"], ["tempo", "pay", "-h"]] {
        let output = sandbox.run(&args);
        assert_eq!(code(&output), Some(0), "{args:?}: {}", stderr(&output));
        assert!(!output.stdout.is_empty(), "{args:?}");
    }
    // `rail use` never selects the rail of the hidden group; only --rail does.
    sandbox.ok(&["rail", "use", "stellar:testnet"]);
    let output = sandbox.run(&["tempo", "status", "not-a-request"]);
    assert!(
        !stderr(&output).contains("selected rail differs"),
        "{}",
        stderr(&output)
    );
    let output = sandbox.run(&["--rail", "stellar:testnet", "tempo", "status", "x"]);
    assert!(
        stderr(&output).contains("selected rail differs"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn solana_status_reads_the_request_when_the_policy_description_fails() {
    let mut sandbox = Sandbox::new("solana-status");
    let mock = Mock::new();
    sandbox
        .set("ALLOWIT_URL", &mock.url)
        .set("ALLOWIT_TOKEN", format!("owner.policy.{}", "s".repeat(32)));
    let listener = mock.listener.try_clone().unwrap();
    let server = std::thread::spawn(move || {
        let mut paths = vec![];
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let (head, _) = read_request(&mut stream);
            let path = head
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_owned();
            if path.ends_with("/status") {
                reply(
                    &mut stream,
                    200,
                    &json!({"requestId":"pay-0001","outcome":"fail","status":"denied","kind":"transaction","reason":"over the cap"}),
                );
                paths.push(path);
                return paths;
            }
            reply(&mut stream, 503, &json!({"error":"unavailable"}));
            paths.push(path);
        }
        paths
    });
    let output = sandbox.run(&["--rail", "solana:devnet", "status", "pay-0001"]);
    assert_eq!(code(&output), Some(20), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("denied"), "{text}");
    // Text output is key-value data, without a prose note.
    assert!(!text.contains("state: "), "{text}");
    common::assert_data(&text, "solana status");
    common::assert_data(&stderr(&output), "solana status");
    assert!(text.contains("network    unknown\n"), "{text}");
    let paths = server.join().unwrap();
    assert!(paths.iter().any(|p| p.ends_with("/skill")), "{paths:?}");
}

#[cfg(unix)]
#[test]
fn arguments_that_are_not_utf8_are_usage_errors() {
    use std::os::unix::ffi::OsStrExt;
    let sandbox = Sandbox::new("utf8");
    let output = sandbox
        .command()
        .arg(std::ffi::OsStr::from_bytes(b"\xff"))
        .output()
        .unwrap();
    assert_eq!(code(&output), Some(2), "{}", stderr(&output));
    assert!(stderr(&output).contains("not valid UTF-8"));
    let output = sandbox
        .command()
        .args([
            std::ffi::OsStr::new("--json"),
            std::ffi::OsStr::from_bytes(b"\xff"),
        ])
        .output()
        .unwrap();
    assert_eq!(code(&output), Some(2));
    let error: Value = serde_json::from_str(stderr(&output).trim()).unwrap();
    assert_eq!(error["error"]["code"], "usage");
}

#[test]
fn solana_harness_results_print_only_data_lines() {
    let mut sandbox = Sandbox::new("solana-harness");
    let mock = Mock::new();
    sandbox
        .set("ALLOWIT_URL", &mock.url)
        .set("ALLOWIT_TOKEN", format!("owner.policy.{}", "s".repeat(32)));
    let listener = mock.listener.try_clone().unwrap();
    let server = std::thread::spawn(move || {
        let skill = json!({
            "policyId": "policy",
            "owner": "owner",
            "title": "Research spend",
            "network": "solana:devnet",
            "status": "active",
            "policy": "fn policy() {}",
            "workflow": [{"label": "cap", "description": "per-day cap"}],
        });
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let (head, _) = read_request(&mut stream);
            let path = head
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_owned();
            if path.ends_with("/status") {
                reply(
                    &mut stream,
                    200,
                    &json!({"requestId":"pay-0001","outcome":"fail","status":"denied","kind":"transaction","reason":"over the cap"}),
                );
                return;
            }
            reply(&mut stream, 200, &skill);
        }
    });
    let shown = sandbox.run(&["--rail", "solana:devnet", "policy", "show"]);
    assert_eq!(code(&shown), Some(0), "{}", stderr(&shown));
    let text = stdout(&shown);
    common::assert_data(&text, "solana policy show");
    assert!(text.contains("title"), "{text}");
    assert!(!text.contains("Allowit policy:"), "{text}");
    let source = sandbox.run(&["--rail", "solana:devnet", "policy", "show", "--source"]);
    assert_eq!(stdout(&source), "fn policy() {}\n");
    let status = sandbox.run(&["--rail", "solana:devnet", "status", "pay-0001"]);
    assert_eq!(code(&status), Some(20), "{}", stderr(&status));
    common::assert_data(&stdout(&status), "solana status");
    common::assert_data(&stderr(&status), "solana status");
    assert!(!stdout(&status).contains("network"), "{}", stdout(&status));
    server.join().unwrap();
}
