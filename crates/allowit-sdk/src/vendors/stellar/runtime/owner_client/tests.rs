//! Synthetic protocol/native fixtures verify orchestration; no model, Wasm execution or chain run is claimed.
#[path = "../../../../../tests/support/owner_chain.rs"]
mod chain;
#[path = "../../../../../tests/support/owner_server.rs"]
mod wire;
use allowit_core::client::store::Store;
use allowit_sdk::vendors::stellar::runtime::{
    Runtime,
    owner::SetupRequest,
    owner_client::{
        OwnerClient,
        workflow::{self, OwnerCapabilities},
    },
};
use chain::*;
fn authority(origin: &str) -> allowit_sdk::config::ServerAuthority {
    allowit_sdk::config::ServerAuthority::new("stellar:testnet", origin, &account(13)).unwrap()
}

use serde_json::{Value, json};
use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
#[test]
fn native_setup_retains_before_sign_and_recovers_lost_reply_without_another_signature() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap().join("proof");
    let store = Store::open(&path).unwrap();
    let c = selection().config;
    let mut chain = Chain::new(c.clone());
    chain.lose_direct = true;
    let chain = Arc::new(chain);
    let runtime = Runtime::from_rpc(chain.clone());
    let request = SetupRequest {
        network: c.network.clone(),
        owner: c.owner.clone(),
        agent: c.agent.clone(),
        server: c.server.clone(),
        request_id: "setup-test-001".into(),
        fee_per_operation: 100,
    };
    let clock = Cell::new(wire::now());
    let before = || {
        let saved: Value = store.load(&request.request_id).unwrap().unwrap();
        assert!(saved["setup"]["transactionXdr"].is_string());
        assert!(saved["signedXdr"].is_null());
    };
    let signer = Key {
        calls: AtomicUsize::new(0),
        bad: false,
        before: Some(&before),
    };
    let error =
        workflow::configure_native(&runtime, &store, &request, &signer, &|| Ok(clock.get()))
            .unwrap_err();
    assert_eq!(error.code, 5);
    assert_eq!(signer.calls.load(Ordering::SeqCst), 1);
    let original: Value = store.load(&request.request_id).unwrap().unwrap();
    assert_eq!(original["submitted"], true);
    assert_eq!(
        original["signedXdr"],
        chain.direct_signed.lock().unwrap().clone().unwrap()
    );
    drop(store);
    clock.set(clock.get() + 301);
    let reopened = Store::open(&path).unwrap();
    let signer = key();
    let out =
        workflow::configure_native(&runtime, &reopened, &request, &signer, &|| Ok(clock.get()))
            .unwrap();
    assert_eq!(out["state"], "configured");
    assert_eq!(signer.calls.load(Ordering::SeqCst), 0);
    let recovered: Value = reopened.load(&request.request_id).unwrap().unwrap();
    assert_eq!(recovered["signedXdr"], original["signedXdr"]);
    assert_eq!(
        chain
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.as_str() == "sendTransaction")
            .count(),
        1
    );
}
#[test]
fn changed_fund_packets_never_reach_the_signer_and_rebound_records_are_not_signing_authority() {
    for tamper_retained in [false, true] {
        let selected = selection();
        let now = wire::now();
        let mut plan = wire::Plan::new(&account(11));
        let step = fund_step(
            &selected.config,
            if tamper_retained {
                10_000_000
            } else {
                20_000_000
            },
            now,
        );
        plan.owner_prepare = Some(json!({"policyId":selected.policy.id,"step":step}));
        let server = wire::Server::new(plan);
        let mut client = OwnerClient::with_authority(
            &authority(&server.url),
            "http://localhost:5173",
            &account(11),
        )
        .unwrap();
        let auth = key();
        client.authenticate(&auth, now).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().canonicalize().unwrap().join("proof")).unwrap();
        let mut chain = Chain::new(selected.config.clone());
        chain.server = Some(server.state.clone());
        let bad = Key {
            calls: AtomicUsize::new(0),
            bad: true,
            before: None,
        };
        let caps = OwnerCapabilities {
            store: &store,
            rpc: &chain,
            signer: &bad,
            clock: &|| Ok(now),
        };
        assert!(
            client
                .fund(caps, &selected, "fund-test-001", "1", 10_000)
                .is_err()
        );
        assert_eq!(
            bad.calls.load(Ordering::SeqCst),
            usize::from(tamper_retained)
        );
        assert_eq!(server.calls("/api/native/owner/submit"), 0);
        if tamper_retained {
            let mut saved: Value = store.load("fund-test-001").unwrap().unwrap();
            saved["work"]["fund"]["amount"] = json!("2");
            saved["work"]["fund"]["units"] = json!(20_000_000);
            saved["steps"][0]["packet"] = fund_step(&selected.config, 20_000_000, now);
            store.save("fund-test-001", &saved).unwrap();
            let signer = Key {
                calls: AtomicUsize::new(0),
                bad: true,
                before: None,
            };
            let caps = OwnerCapabilities {
                store: &store,
                rpc: &chain,
                signer: &signer,
                clock: &|| Ok(now),
            };
            assert!(
                client
                    .fund(caps, &selected, "fund-test-001", "1", 10_000)
                    .is_err()
            );
            assert_eq!(
                signer.calls.load(Ordering::SeqCst),
                0,
                "modified retained business fields reached the owner signer"
            );
        }
    }
}
#[test]
fn funded_wallet_receipt_and_restart_preserve_exact_signed_bytes_and_single_owner_authorization() {
    let selected = selection();
    let now = wire::now();
    let mut plan = wire::Plan::new(&account(11));
    plan.owner_prepare = Some(
        json!({"policyId":selected.policy.id,"step":fund_step(&selected.config,10_000_000,now)}),
    );
    plan.lose_owner_reply = true;
    let server = wire::Server::new(plan);
    let mut client = OwnerClient::with_authority(
        &authority(&server.url),
        "http://localhost:5173",
        &account(11),
    )
    .unwrap();
    let signer = key();
    client.authenticate(&signer, now).unwrap();
    let prior = signer.calls.load(Ordering::SeqCst);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap().join("proof");
    let store = Store::open(&path).unwrap();
    let mut chain = Chain::new(selected.config.clone());
    chain.server = Some(server.state.clone());
    let caps = OwnerCapabilities {
        store: &store,
        rpc: &chain,
        signer: &signer,
        clock: &|| Ok(now),
    };
    assert_eq!(
        client
            .fund(caps, &selected, "fund-lost-001", "1", 10_000)
            .unwrap_err()
            .code,
        5
    );
    assert_eq!(signer.calls.load(Ordering::SeqCst), prior + 1);
    let saved: Value = store.load("fund-lost-001").unwrap().unwrap();
    assert_eq!(saved["submitted"], Value::Null);
    assert_eq!(saved["steps"][0]["submitted"], true);
    let signed = saved["steps"][0]["signedXdr"].clone();
    drop(store);
    let reopened = Store::open(&path).unwrap();
    let caps = OwnerCapabilities {
        store: &reopened,
        rpc: &chain,
        signer: &signer,
        clock: &|| Ok(now + 301),
    };
    let out = client
        .fund(caps, &selected, "fund-lost-001", "1", 10_000)
        .unwrap();
    assert_eq!(out.state, "funded");
    assert_eq!(signer.calls.load(Ordering::SeqCst), prior + 1);
    let state = server.state.lock().unwrap();
    assert_eq!(state.owner_submissions.len(), 2);
    assert!(
        state
            .owner_submissions
            .iter()
            .all(|s| s["transactionXdr"] == signed)
    );
}

#[test]
fn source_consent_checks_exact_payload_and_current_native_authority_before_owner_callback() {
    for (changed, required, poisoned_export) in [
        (false, true, false),
        (true, true, false),
        (false, false, false),
        (false, false, true),
    ] {
        let selected = native_selection();
        let now = wire::now();
        let mut plan = wire::Plan::new(&account(11));
        let mut consent = consent(&selected);
        if changed {
            consent["payload"]["params"]["payment_limit"] = json!({"Int":"20000000"});
        }
        consent["required"] = json!(required);
        plan.approval = Some(consent);
        plan.activation = Some(json!({"active":true}));
        let mut exported = descriptor(&selected);
        if poisoned_export {
            exported["sourceApproval"] = json!({"revision":selected.policy.revision,"signatures":[{"signer":account(11),"signature":vec![0u8;64]}]});
        }
        plan.export = Some(exported);
        let server = wire::Server::new(plan);
        let mut client = OwnerClient::with_authority(
            &authority(&server.url),
            "http://localhost:5173",
            &account(11),
        )
        .unwrap();
        let signer = key();
        client.authenticate(&signer, now).unwrap();
        let before = signer.calls.load(Ordering::SeqCst);
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(
            &dir.path()
                .canonicalize()
                .unwrap()
                .join("stellar-owner-workflows"),
        )
        .unwrap();
        let chain = Chain::new(selected.config.clone());
        *chain.source.lock().unwrap() = configured_source(&selected.config);
        let caps = OwnerCapabilities {
            store: &store,
            rpc: &chain,
            signer: &signer,
            clock: &|| Ok(now),
        };
        let result = client.activate(caps, &selected, "approve-native-001");
        assert_eq!(result.is_ok(), !changed);
        assert_eq!(
            signer.calls.load(Ordering::SeqCst),
            before + usize::from(!changed)
        );
        assert_eq!(
            server.calls("/api/micro/allocation/confirm"),
            usize::from(!changed)
        );
        if !changed {
            if poisoned_export {
                assert!(
                    client
                        .export(&store, &selected, &chain, "approve-native-001", now)
                        .is_err()
                );
                continue;
            }
            let saved: Value = store.load("approve-native-001").unwrap().unwrap();
            let mut forged = saved.clone();
            forged["signature"] = Value::Null;
            store.save("approve-native-001", &forged).unwrap();
            assert!(
                client
                    .export(&store, &selected, &chain, "approve-native-001", now)
                    .is_err()
            );
            store.save("approve-native-001", &saved).unwrap();
            let out = client
                .export(&store, &selected, &chain, "approve-native-001", now)
                .unwrap();
            assert_eq!(
                out["config"],
                serde_json::to_value(&selected.config).unwrap()
            );
            // Exercise the common export-to-cache transition using the actual retained activation.
            drop(store);
            let root = dir.path().canonicalize().unwrap();
            let rpc = RpcServer::new(chain);
            let key_store = Store::open(&root.join("secrets")).unwrap();
            drop(key_store);
            let key_path = root.join("secrets/owner.key");
            {
                use std::io::Write;
                use std::os::unix::fs::OpenOptionsExt;
                let secret = allowit_stellar_wallet::signing::LocalSigner::from_entropy(&[11; 32])
                    .unwrap()
                    .encoded_secret();
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&key_path)
                    .unwrap();
                file.write_all(secret.as_bytes()).unwrap();
            }
            let signer =
                allowit_sdk::wallet::local_signer(&selected.config.network, &key_path).unwrap();
            let document = serde_json::to_vec(&workflow::Outcome {
                request_id: "approve-native-001".into(),
                state: "active".into(),
                selection: selected.clone(),
                transactions: vec![],
            })
            .unwrap();
            let clock = || Ok(now);
            let cx = allowit_sdk::owner::Context {
                network: &selected.config.network,
                backend: &server.url,
                authority: &authority(&server.url),
                origin: "http://localhost:5173",
                rpc_url: &rpc.url,
                root: &root,
                signer: &signer,
                clock: &clock,
            };
            let profile = allowit_sdk::owner::export(&cx, &document, "approve-native-001").unwrap();
            assert_eq!(profile.description().source, selected.policy.code);
            let held = allowit_sdk::profiles::Cache::open(&root).unwrap();
            assert!(allowit_sdk::owner::export(&cx, &document, "approve-native-001").is_err());
            drop(held);
            let activation = Store::open(&root.join("stellar-owner-workflows")).unwrap();
            assert_eq!(
                activation
                    .load::<Value>("approve-native-001")
                    .unwrap()
                    .unwrap()["completed"],
                true
            );
            drop(activation);
            let again = allowit_sdk::owner::export(&cx, &document, "approve-native-001").unwrap();
            assert_eq!(again.version, profile.version);
            let cache = allowit_sdk::profiles::Cache::open(&root).unwrap();
            assert_eq!(
                cache
                    .resolve(&allowit_sdk::profiles::Selector {
                        network: selected.config.network.clone(),
                        wallet: Some(selected.config.wallet.clone()),
                        agent: Some(selected.config.agent.clone()),
                        ..Default::default()
                    })
                    .unwrap()
                    .version,
                profile.version
            );
        }
    }
}
#[cfg(unix)]
#[test]
fn failed_signed_retention_prevents_native_transmission() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap().join("proof");
    let store = Store::open(&path).unwrap();
    let c = selection().config;
    let chain = Arc::new(Chain::new(c.clone()));
    let runtime = Runtime::from_rpc(chain.clone());
    let now = wire::now();
    let request = SetupRequest {
        network: c.network,
        owner: c.owner,
        agent: c.agent,
        server: c.server,
        request_id: "native-retain-001".into(),
        fee_per_operation: 100,
    };
    let fail_retention =
        || std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500)).unwrap();
    let signer = Key {
        calls: AtomicUsize::new(0),
        bad: false,
        before: Some(&fail_retention),
    };
    let result = workflow::configure_native(&runtime, &store, &request, &signer, &|| Ok(now));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result.is_err());
    assert_eq!(signer.calls.load(Ordering::SeqCst), 1);
    assert!(
        !chain
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|s| s == "sendTransaction")
    );
    let saved: Value = store.load(&request.request_id).unwrap().unwrap();
    assert!(saved["signedXdr"].is_null());
    assert_eq!(saved["submitted"], false);
}

#[test]
fn portable_native_import_checks_source_consent_current_authority_and_exact_profile_selection() {
    use allowit_sdk::profiles::{Cache, Selector};
    use ed25519_dalek::{Signer, SigningKey};
    let selected = native_selection();
    let mut profile = descriptor(&selected);
    let digest = allowit_stellar_wallet::signing::signed_message_digest(
        consent(&selected)["message"].as_str().unwrap(),
    );
    let signature = SigningKey::from_bytes(&[11; 32]).sign(&digest).to_bytes();
    profile["sourceApproval"] = json!({"revision":selected.policy.revision,"signatures":[{"signer":selected.config.owner,"signature":signature.to_vec()}]});
    let chain = Chain::new(selected.config.clone());
    *chain.source.lock().unwrap() = configured_source(&selected.config);
    let rpc = RpcServer::new(chain);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let cache = Cache::open(&root).unwrap();
    let selector = Selector {
        network: selected.config.network.clone(),
        wallet: Some(selected.config.wallet.clone()),
        agent: Some(selected.config.agent.clone()),
        ..Default::default()
    };
    for variant in 0..15 {
        let mut bad = profile.clone();
        match variant {
            0 => {
                bad.as_object_mut().unwrap().remove("sourceApproval");
            }
            1 => bad["sourceApproval"]["revision"] = json!(selected.policy.revision + 1),
            2 => bad["sourceApproval"]["signatures"][0]["signature"][0] = json!(signature[0] ^ 1),
            3 => {
                let repeated = bad["sourceApproval"]["signatures"][0].clone();
                bad["sourceApproval"]["signatures"]
                    .as_array_mut()
                    .unwrap()
                    .push(repeated);
            }
            4 => bad["config"]["agent"] = json!(account(14)),
            5 => bad["portable"]["params"]["payment_limit"] = json!({"Int":"20000000"}),
            6 => bad["config"]["policySource"] = json!("changed source"),
            7 => bad["signingSeed"] = json!("private-sentinel"),
            8 => bad["sourceApproval"]["privateKey"] = json!("private-sentinel"),
            9 => bad["config"]["owner"] = json!(account(14)),
            10 => bad["config"]["server"] = json!(account(14)),
            11 => {
                bad["config"]["agent"] = json!(selected.config.server);
                bad["config"]["server"] = json!(selected.config.agent);
            }
            12 => {
                let extra = SigningKey::from_bytes(&[14; 32]).sign(&digest).to_bytes();
                bad["sourceApproval"]["signatures"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"signer":account(14),"signature":extra.to_vec()}));
            }
            13 => bad["config"]["rules"] = json!(["invalid-rule"]),
            _ => bad["config"]["sourceDigest"] = json!(vec![1u8; 32]),
        }
        let error = cache
            .import(
                &authority("http://localhost:8080"),
                &rpc.url,
                &serde_json::to_vec(&bad).unwrap(),
            )
            .err()
            .expect("tampered import must fail");
        assert!(!error.message.contains("private-sentinel"));
        assert!(cache.resolve(&selector).is_err());
    }
    let raw = serde_json::to_string(&profile).unwrap();
    let duplicate = raw.replacen(
        "\"sourceApproval\":{",
        "\"sourceApproval\":{\"revision\":999,",
        1,
    );
    assert!(
        cache
            .import(
                &authority("http://localhost:8080"),
                &rpc.url,
                duplicate.as_bytes()
            )
            .is_err()
    );
    let first = cache
        .import(
            &authority("http://localhost:8080"),
            &rpc.url,
            raw.as_bytes(),
        )
        .unwrap();
    let pretty = cache
        .import(
            &authority("http://localhost:8080"),
            &rpc.url,
            &serde_json::to_vec_pretty(&profile).unwrap(),
        )
        .unwrap();
    assert_eq!(first.version, pretty.version);
    assert_eq!(cache.resolve(&selector).unwrap().version, first.version);
    assert_eq!(
        serde_json::to_value(&first.description().params).unwrap(),
        serde_json::to_value(&selected.config.params).unwrap()
    );
    profile["portable"]["build"]["attempts"] = json!(2);
    let second = cache
        .import(
            &authority("http://localhost:8080"),
            &rpc.url,
            &serde_json::to_vec(&profile).unwrap(),
        )
        .unwrap();
    assert_ne!(first.version, second.version);
    assert!(cache.resolve(&selector).is_err());
    let mut exact = selector.clone();
    exact.version = Some(first.version.clone());
    cache.select(&exact).unwrap();
    drop(cache);
    let reopened = Cache::open(&root).unwrap();
    assert_eq!(reopened.resolve(&selector).unwrap().version, first.version);
    let mut wrong = selector;
    wrong.agent = Some(account(14));
    assert!(reopened.resolve(&wrong).is_err());
}

#[test]
fn owner_funding_renews_only_the_independently_observed_relative_lifetime_target() {
    for target in [999, 1119] {
        let selected = selection();
        let now = wire::now();
        let mut plan = wire::Plan::new(&account(11));
        plan.owner_prepare = Some(
            json!({"policyId":selected.policy.id,"step":extend_step(&selected.config,target,now)}),
        );
        let mut fund = fund_step(&selected.config, 10_000_000, now);
        change_step(&mut fund, |tx| tx.seq_num.0 = 43);
        plan.owner_next_prepare = Some(json!({"policyId":selected.policy.id,"step":fund}));
        let server = wire::Server::new(plan);
        let mut client = OwnerClient::with_authority(
            &authority(&server.url),
            "http://localhost:5173",
            &account(11),
        )
        .unwrap();
        let signer = key();
        client.authenticate(&signer, now).unwrap();
        let before = signer.calls.load(Ordering::SeqCst);
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().canonicalize().unwrap().join("proof")).unwrap();
        let mut chain = Chain::new(selected.config.clone());
        chain.server = Some(server.state.clone());
        let out = client.fund(
            OwnerCapabilities {
                store: &store,
                rpc: &chain,
                signer: &signer,
                clock: &|| Ok(now),
            },
            &selected,
            "fund-renew-001",
            "1",
            10_000,
        );
        if target == 1119 {
            assert!(out.is_err());
            assert_eq!(signer.calls.load(Ordering::SeqCst), before);
            assert!(server.state.lock().unwrap().owner_submissions.is_empty());
        } else {
            assert_eq!(out.unwrap().state, "funded");
            assert_eq!(signer.calls.load(Ordering::SeqCst), before + 2);
            let saved: Value = store.load("fund-renew-001").unwrap().unwrap();
            assert_eq!(saved["steps"][0]["extendTo"], 999);
            assert_eq!(saved["steps"][1]["packet"]["action"], "fund");
            let bytes = saved["steps"][0]["signedXdr"].clone();
            client
                .fund(
                    OwnerCapabilities {
                        store: &store,
                        rpc: &chain,
                        signer: &signer,
                        clock: &|| Ok(now + 301),
                    },
                    &selected,
                    "fund-renew-001",
                    "1",
                    10_000,
                )
                .unwrap();
            assert_eq!(signer.calls.load(Ordering::SeqCst), before + 2);
            let retained: Value = store.load("fund-renew-001").unwrap().unwrap();
            assert_eq!(retained["steps"][0]["signedXdr"], bytes);
        }
    }
}

#[test]
fn stale_funding_destination_never_receives_an_owner_signature_or_submission() {
    let p = selection();
    let now = wire::now();
    let mut plan = wire::Plan::new(&account(11));
    plan.owner_prepare =
        Some(json!({"policyId":p.policy.id,"step":fund_step(&p.config,10_000_000,now)}));
    let server = wire::Server::new(plan);
    let mut client = OwnerClient::with_authority(
        &authority(&server.url),
        "http://localhost:5173",
        &account(11),
    )
    .unwrap();
    let signer = key();
    client.authenticate(&signer, now).unwrap();
    let before = signer.calls.load(Ordering::SeqCst);
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().canonicalize().unwrap().join("proof")).unwrap();
    let mut chain = Chain::new(p.config.clone());
    chain.config.agent = account(14);
    chain.server = Some(server.state.clone());
    assert!(
        client
            .fund(
                OwnerCapabilities {
                    store: &store,
                    rpc: &chain,
                    signer: &signer,
                    clock: &|| Ok(now)
                },
                &p,
                "fund-stale-001",
                "1",
                10_000
            )
            .is_err()
    );
    assert_eq!(signer.calls.load(Ordering::SeqCst), before);
    assert!(server.state.lock().unwrap().owner_submissions.is_empty());
}
