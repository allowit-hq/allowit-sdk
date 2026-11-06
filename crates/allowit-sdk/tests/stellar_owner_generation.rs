//! Current owner authentication/generation HTTP invariants. All model output is recorded replay.
#[path = "support/owner_chain.rs"]
mod chain;
#[path = "support/owner_server.rs"]
mod wire;
use allowit_sdk::vendors::stellar::runtime::owner_client::{
    GenerateRequest, OwnerClient, OwnerSigner, Target,
};
use allowit_stellar_wallet as native;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Key {
    account: String,
    calls: AtomicUsize,
    bad: bool,
}
impl Key {
    fn new() -> Self {
        Self {
            account: native::account_for_seed(&[11; 32]),
            calls: AtomicUsize::new(0),
            bad: false,
        }
    }
}
impl OwnerSigner for Key {
    fn account(&self) -> &str {
        &self.account
    }
    fn sign_hash(&self, value: &[u8; 32]) -> native::Result<[u8; 64]> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(if self.bad {
            [0; 64]
        } else {
            SigningKey::from_bytes(&[11; 32]).sign(value).to_bytes()
        })
    }
}
fn request() -> GenerateRequest {
    GenerateRequest {
        prompt: wire::fixture()["prompt"].as_str().unwrap().into(),
        network: "stellar:testnet".into(),
        profile: "allowit-stellar-policy-wallet-v1".into(),
        target: Target::NewWallet,
    }
}

#[test]
fn exact_owner_signin_rotates_cookie_and_generation_preserves_the_complete_prompt() {
    let key = Key::new();
    let mut plan = wire::Plan::new(&key.account);
    plan.policy["serviceSecret"] = json!("must-not-be-exported");
    let origin = plan.origin.clone();
    // A matching existing wallet does not change the explicit template selection.
    plan.installations = json!({"installations":[{"notSelected":"existing"}]});
    let server = wire::Server::new(plan);
    let mut client = OwnerClient::new(&server.url, &origin, &key.account).unwrap();
    client.authenticate(&key, wire::now()).unwrap();
    let mut input = request();
    input.prompt = format!("\n{}\nKeep my exact wording: é 😀.", input.prompt);
    let policy = client.generate(&input, wire::now()).unwrap();
    assert_eq!(policy.original_intent, input.prompt);
    assert_eq!(policy.status, "draft");
    assert!(policy.runtime.instance_binding.template.unwrap());
    assert!(
        !serde_json::to_string(&policy)
            .unwrap()
            .contains("must-not-be-exported")
    );
    assert_eq!(key.calls.load(Ordering::SeqCst), 1);
    assert_eq!(server.calls("/api/micro/generate"), 1);
    assert_eq!(server.calls("/api/native/policies"), 0);
    let state = server.state.lock().unwrap();
    let sent = &state.generated[0];
    assert_eq!(sent["prompt"], input.prompt);
    assert_eq!(sent["network"], input.network);
    assert_eq!(sent["stellarInstallationId"], "");
    assert_eq!(
        sent["runtime"],
        json!({"sourceProfile":"allowit-stellar-transfer-v1","wallet":{"kind":"contract-wallet","profile":input.profile}})
    );
    assert_eq!(
        sent["operation"],
        json!({"function":"stellar::sep41_transfer"})
    );
}
#[test]
fn altered_challenges_and_unsafe_guest_cookies_never_reach_the_owner_signer() {
    for variant in 0..11 {
        let key = Key::new();
        let mut plan = wire::Plan::new(&key.account);
        let origin = plan.origin.clone();
        match variant {
            0 => plan.challenge["address"] = json!(native::account_for_seed(&[12; 32])),
            1 => plan.challenge["origin"] = json!("https://other.example"),
            2 => plan.challenge["id"] = json!("cd".repeat(32)),
            3 => plan.challenge = wire::challenge(&key.account, &origin, wire::now() - 301),
            4 => plan.challenge = wire::challenge(&key.account, &origin, wire::now() + 60),
            5 => {
                plan.challenge["message"] = json!(
                    plan.challenge["message"]
                        .as_str()
                        .unwrap()
                        .replace("does not authorize spending", "authorizes spending")
                )
            }
            6 => {
                plan.challenge["message"] = json!(
                    plan.challenge["message"]
                        .as_str()
                        .unwrap()
                        .replace("localhost:5173", "attacker.test")
                )
            }
            7 => plan.guest_cookie.push_str("; Domain=attacker.test"),
            8 => plan.guest_cookie = plan.guest_cookie.replace("Path=/", "Path=/api"),
            9 => plan.guest_cookie.push_str("; Path=/"),
            _ => {
                plan.challenge["message"] = json!(format!(
                    "{}\nextra",
                    plan.challenge["message"].as_str().unwrap()
                ))
            }
        }
        let server = wire::Server::new(plan);
        let mut client = OwnerClient::new(&server.url, &origin, &key.account).unwrap();
        assert!(client.authenticate(&key, wire::now()).is_err(), "{variant}");
        assert_eq!(key.calls.load(Ordering::SeqCst), 0, "{variant}");
        assert_eq!(server.calls("/api/auth/verify"), 0);
        assert_eq!(server.calls("/api/micro/generate"), 0);
    }
    let key = Key::new();
    let expected = native::account_for_seed(&[12; 32]);
    let server = wire::Server::new(wire::Plan::new(&expected));
    let mut client = OwnerClient::new(&server.url, "http://localhost:5173", &expected).unwrap();
    assert!(client.authenticate(&key, wire::now()).is_err());
    assert_eq!(server.calls("/api/auth/challenge"), 0);
    assert_eq!(key.calls.load(Ordering::SeqCst), 0);
}
#[test]
fn invalid_signatures_unrotated_sessions_and_redirects_do_not_become_owner_authority() {
    let mut key = Key::new();
    key.bad = true;
    let plan = wire::Plan::new(&key.account);
    let server = wire::Server::new(plan);
    let mut client = OwnerClient::new(&server.url, "http://localhost:5173", &key.account).unwrap();
    assert!(client.authenticate(&key, wire::now()).is_err());
    assert_eq!(server.calls("/api/auth/verify"), 0);
    let key = Key::new();
    let mut plan = wire::Plan::new(&key.account);
    plan.rotate_cookie = false;
    let server = wire::Server::new(plan);
    let mut client = OwnerClient::new(&server.url, "http://localhost:5173", &key.account).unwrap();
    assert!(client.authenticate(&key, wire::now()).is_err());
    assert!(client.generate(&request(), wire::now()).is_err());
    assert_eq!(server.calls("/api/micro/generate"), 0);
    let attacker = wire::Server::new(wire::Plan::new(&key.account));
    let mut plan = wire::Plan::new(&key.account);
    plan.redirect_challenge = Some(format!("{}/api/auth/challenge", attacker.url));
    let server = wire::Server::new(plan);
    let mut client = OwnerClient::new(&server.url, "http://localhost:5173", &key.account).unwrap();
    let prior = key.calls.load(Ordering::SeqCst);
    assert!(client.authenticate(&key, wire::now()).is_err());
    assert_eq!(key.calls.load(Ordering::SeqCst), prior);
    assert_eq!(attacker.calls("/api/auth/challenge"), 0);
    let mut plan = wire::Plan::new(&key.account);
    plan.redirect_verify = Some(format!("{}/api/auth/verify", attacker.url));
    let server = wire::Server::new(plan);
    let mut client = OwnerClient::new(&server.url, "http://localhost:5173", &key.account).unwrap();
    assert!(client.authenticate(&key, wire::now()).is_err());
    assert_eq!(server.calls("/api/auth/verify"), 1);
    assert_eq!(attacker.calls("/api/auth/verify"), 0);
    assert!(client.generate(&request(), wire::now()).is_err());
    assert_eq!(server.calls("/api/micro/generate"), 0);
}
#[test]
fn missing_template_or_unadvertised_selection_never_creates_or_falls_back_to_an_owned_wallet() {
    for variant in 0..4 {
        let key = Key::new();
        let mut plan = wire::Plan::new(&key.account);
        match variant {
            0 => plan.templates = json!({"templates":[]}),
            1 => plan.discovery["wallets"][0]["capabilities"]["generate"] = json!(false),
            2 => plan.discovery["wallets"][0]["networks"] = json!(["stellar:mainnet"]),
            _ => plan.discovery["wallets"][0]["operations"] = json!(["other::operation"]),
        }
        let server = wire::Server::new(plan);
        let mut client =
            OwnerClient::new(&server.url, "http://localhost:5173", &key.account).unwrap();
        client.authenticate(&key, wire::now()).unwrap();
        assert!(client.generate(&request(), wire::now()).is_err());
        assert_eq!(server.calls("/api/micro/generate"), 0);
        assert_eq!(server.calls("/api/native/policies"), 0);
    }
}
#[test]
fn ambiguous_generation_is_sent_once_and_keeps_credentials_out_of_errors() {
    let key = Key::new();
    let mut plan = wire::Plan::new(&key.account);
    plan.lost_generation = true;
    let server = wire::Server::new(plan);
    let mut client = OwnerClient::new(&server.url, "http://localhost:5173", &key.account).unwrap();
    client.authenticate(&key, wire::now()).unwrap();
    let error = client.generate(&request(), wire::now()).unwrap_err();
    assert_eq!(error.code, 5);
    assert_eq!(server.calls("/api/micro/generate"), 1);
    assert!(!format!("{error:?}").contains(&"s".repeat(32)));
}
#[test]
fn returned_source_and_portable_identity_are_verified_without_claiming_deployment() {
    for pointer in [
        "/code",
        "/sourceHash",
        "/language",
        "/network",
        "/runtime/instanceBinding/portable/paramsHash",
        "/runtime/instanceBinding/asset",
    ] {
        let key = Key::new();
        let mut plan = wire::Plan::new(&key.account);
        *plan.policy.pointer_mut(pointer).unwrap() = Value::String("changed".into());
        let server = wire::Server::new(plan);
        let mut client =
            OwnerClient::new(&server.url, "http://localhost:5173", &key.account).unwrap();
        client.authenticate(&key, wire::now()).unwrap();
        assert!(
            client.generate(&request(), wire::now()).is_err(),
            "{pointer}"
        );
        assert_eq!(server.calls("/api/micro/generate"), 1);
    }
}

fn existing(plan: &mut wire::Plan, owner: &str) -> native::WalletConfig {
    let fixture: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/stellar-portable-descriptors.json")).unwrap();
    let mut config: native::WalletConfig =
        serde_json::from_value(fixture[2]["bundle"]["config"].clone()).unwrap();
    let d = &plan.policy["runtime"]["instanceBinding"]["portable"];
    config.owner = owner.into();
    config.agent = native::account_for_seed(&[12; 32]);
    config.server = native::account_for_seed(&[13; 32]);
    config.wallet = stellar_strkey::Contract([3; 32]).to_string();
    config.policy_instance = [7; 32];
    config.module_digest = [44; 32];
    config.asset = d["asset"]["address"].as_str().unwrap().into();
    config.asset_symbol = "USDC".into();
    config.asset_issuer = Some("GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5".into());
    config.policy_source = plan.policy["code"].as_str().unwrap().into();
    config.source_hash = native::hash(config.policy_source.as_bytes());
    config.params = serde_json::from_value(d["params"].clone()).unwrap();
    config.rules = serde_json::from_value(d["rules"].clone()).unwrap();
    config.tier = Some(native::PolicyTier::Common);
    config.max_fee_stroops = 5_000_000;
    config.source_digest = native::policy::standard_prefix_digest(
        native::PolicyTier::Common,
        &config.module_digest,
        &config.asset,
        &config.rules,
    )
    .unwrap();
    native::validate_config(&config).unwrap();
    plan.installations = json!({"installations":[{"policyId":"07".repeat(32),"profile":"allowit-stellar-policy-wallet-v1","config":config}]});
    let b = &mut plan.policy["runtime"]["instanceBinding"];
    b["template"] = json!(false);
    b["wallet"] = json!(config.wallet);
    b["moduleDigest"] = json!(native::hex(&config.module_digest));
    config
}
#[test]
fn explicit_existing_installation_is_forwarded_and_never_becomes_a_template_request() {
    for wrong_owner in [false, true] {
        let key = Key::new();
        let mut plan = wire::Plan::new(&key.account);
        existing(&mut plan, &key.account);
        if wrong_owner {
            plan.installations["installations"][0]["config"]["owner"] =
                json!(native::account_for_seed(&[99; 32]));
        }
        let server = wire::Server::new(plan);
        let mut client =
            OwnerClient::new(&server.url, "http://localhost:5173", &key.account).unwrap();
        client.authenticate(&key, wire::now()).unwrap();
        let mut input = request();
        input.target = Target::Installation("07".repeat(32));
        let out = client.generate(&input, wire::now());
        assert_eq!(out.is_ok(), !wrong_owner);
        assert_eq!(server.calls("/api/native/templates"), 0);
        assert_eq!(
            server.calls("/api/micro/generate"),
            usize::from(!wrong_owner)
        );
        if !wrong_owner {
            assert_eq!(
                server.state.lock().unwrap().generated[0]["stellarInstallationId"],
                "07".repeat(32)
            );
        }
    }
}
#[test]
fn mainnet_alias_uses_the_exact_advertised_network_and_never_testnet_terms() {
    let key = Key::new();
    let mut plan = wire::Plan::new(&key.account);
    let issuer = "GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN";
    let asset = native::sac_address(
        "stellar:mainnet",
        &native::asset_from_identity("USDC", Some(issuer)).unwrap(),
    )
    .unwrap();
    plan.discovery["wallets"][0]["networks"] = json!(["stellar:mainnet"]);
    plan.templates["templates"][0]["network"] = json!("stellar:mainnet");
    plan.templates["templates"][0]["asset"] = json!(asset);
    plan.templates["templates"][0]["assetIssuer"] = json!(issuer);
    plan.policy["network"] = json!("stellar:mainnet");
    plan.policy["mint"] = json!(asset);
    let b = &mut plan.policy["runtime"]["instanceBinding"];
    b["asset"] = json!(asset);
    b["assetIssuer"] = json!(issuer);
    b["portable"]["asset"]["address"] = json!(asset);
    b["portable"]["params"]["tokens"]["List"][0]["Addr"] = json!(asset);
    b["portable"]["paramsHash"] = json!(allowit_core::digest(
        &serde_json_canonicalizer::to_vec(&b["portable"]["params"]).unwrap()
    ));
    let server = wire::Server::new(plan);
    let mut client = OwnerClient::new(&server.url, "http://localhost:5173", &key.account).unwrap();
    client.authenticate(&key, wire::now()).unwrap();
    let mut input = request();
    input.network = "stellar:pubnet".into();
    let out = client.generate(&input, wire::now()).unwrap();
    assert_eq!(out.network, "stellar:mainnet");
    // The next common owner operation accepts the canonical local alias without changing the draft literal.
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let secret_dir = allowit_core::client::store::Store::open(&root.join("secrets")).unwrap();
    drop(secret_dir);
    let path = root.join("secrets/owner");
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let secret = native::signing::LocalSigner::from_entropy(&[11; 32])
            .unwrap()
            .encoded_secret();
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        f.write_all(secret.as_bytes()).unwrap();
    }
    let signer = allowit_sdk::wallet::local_signer("stellar:pubnet", &path).unwrap();
    let authority = allowit_sdk::config::ServerAuthority::new(
        "stellar:pubnet",
        &server.url,
        &chain::account(13),
    )
    .unwrap();
    let stop = || {
        Err(allowit_sdk::error::Error::config(
            "alias reached authenticated owner workflow",
        ))
    };
    let context = allowit_sdk::owner::Context {
        network: "stellar:pubnet",
        backend: &server.url,
        authority: &authority,
        origin: "http://localhost:5173",
        rpc_url: "http://127.0.0.1:1",
        root: &root,
        signer: &signer,
        clock: &stop,
    };
    let draft = serde_json::to_vec(&out).unwrap();
    let executable = chain::wasm();
    let error = allowit_sdk::owner::deploy(
        &context,
        allowit_sdk::owner::Deploy {
            draft: &draft,
            agent: &chain::account(12),
            server: &chain::account(13),
            executable: &executable,
            request_id: "mainnet-alias-deploy-001",
            fee_limit: "10000",
        },
    )
    .unwrap_err();
    assert_eq!(error.message, "alias reached authenticated owner workflow");

    assert_eq!(
        server.state.lock().unwrap().generated[0]["network"],
        "stellar:mainnet"
    );
}

#[test]
fn existing_native_generation_binds_the_new_canonical_source_identity() {
    for tampered in [false, true] {
        let key = Key::new();
        let mut plan = wire::Plan::new(&key.account);
        let mut config = existing(&mut plan, &key.account);
        config.route = native::WalletRoute::Native;
        config.wallet = native::account_for_seed(&[14; 32]);
        config.tier = None;
        config.max_fee_stroops = 100_000;
        native::validate_config(&config).unwrap();
        plan.installations = json!({"installations":[{"policyId":"07".repeat(32),"profile":"allowit-stellar-native-v1","config":config}]});
        plan.discovery["wallets"][0]["profile"] = json!("allowit-stellar-native-v1");
        plan.discovery["wallets"][0]["kind"] = json!("owner-wallet");
        plan.policy["runtime"]["wallet"] =
            json!({"kind":"owner-wallet","profile":"allowit-stellar-native-v1"});
        let b = plan.policy["runtime"]["instanceBinding"]
            .as_object_mut()
            .unwrap();
        b.remove("tier");
        b.remove("template");
        b.insert("route".into(), json!("native"));
        b.insert("wallet".into(), json!(config.wallet));
        b.insert("maxFeeStroops".into(), json!(config.max_fee_stroops));
        let d = &b["portable"];
        let identity = json!({"language":d["language"],"tier":d["tier"],"rules":d["rules"],"sourceHash":d["sourceHash"],"paramsHash":d["paramsHash"]});
        b.insert(
            "moduleDigest".into(),
            json!(if tampered {
                "44".repeat(32)
            } else {
                allowit_core::digest(&serde_json_canonicalizer::to_vec(&identity).unwrap())
            }),
        );
        let server = wire::Server::new(plan);
        let mut client =
            OwnerClient::new(&server.url, "http://localhost:5173", &key.account).unwrap();
        client.authenticate(&key, wire::now()).unwrap();
        let mut input = request();
        input.profile = "allowit-stellar-native-v1".into();
        input.target = Target::Installation("07".repeat(32));
        let out = client.generate(&input, wire::now());
        assert_eq!(out.is_ok(), !tampered);
        assert_eq!(server.calls("/api/micro/generate"), 1);
        assert_eq!(server.calls("/api/native/templates"), 0);
    }
}

#[test]
fn an_empty_standard_prefix_can_use_either_advertised_standard_wallet_tier() {
    for tier in ["simple", "common"] {
        let key = Key::new();
        let mut plan = wire::Plan::new(&key.account);
        let source = "use allowit::prelude::*;\npub fn execute(ctx: &AllowitContext) -> allowit::Result { Ok(()) }\n";
        let digest = allowit_core::digest(source.as_bytes());
        plan.policy["code"] = json!(source);
        plan.policy["sourceHash"] = json!(digest);
        let b = &mut plan.policy["runtime"]["instanceBinding"];
        b["tier"] = json!(tier);
        b["portable"]["sourceHash"] = json!(digest);
        b["portable"]["rules"] = json!([]);
        b["portable"]["build"]["prefix"] = json!([]);
        let server = wire::Server::new(plan);
        let mut client =
            OwnerClient::new(&server.url, "http://localhost:5173", &key.account).unwrap();
        client.authenticate(&key, wire::now()).unwrap();
        let out = client.generate(&request(), wire::now()).unwrap();
        assert_eq!(
            serde_json::to_value(out.runtime.instance_binding.tier).unwrap(),
            tier
        );
        assert_eq!(server.calls("/api/micro/generate"), 1);
    }
}
