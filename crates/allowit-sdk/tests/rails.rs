//! Shared SDK behavior, run on every registered rail. No assertion names a vendor: values come
//! from each rail's fixtures, and a rail skips a feature it declares unsupported.
use allowit_sdk::{
    config::ServerAuthority,
    execution,
    generation::{self, GenerateRequest, Target},
    input::address,
    kinds::{self, WalletKind},
    owner,
    plugins::{self, Feature, LinkKind, testkit},
    routing,
    runtime::{self, Runtime},
    wallet,
};

#[test]
fn every_rail_has_a_unique_id_and_canonical_networks() {
    let mut ids = Vec::new();
    for rail in plugins::rails() {
        assert!(!ids.contains(&rail.id()), "duplicate rail id {}", rail.id());
        ids.push(rail.id());
        assert!(
            rail.id()
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        );
        assert!(!rail.networks().is_empty());
        let support = rail.test_support();
        let test = routing::info(support.network).unwrap();
        assert!(test.testnet, "{} tests on a testnet", rail.id());
        for network in rail.networks() {
            assert_eq!(plugins::for_network(network.id).unwrap().id(), rail.id());
            for alias in network.aliases {
                assert_eq!(routing::normalize(alias).unwrap(), network.id);
            }
        }
        assert_eq!(plugins::by_id(rail.id()).unwrap().id(), rail.id());
    }
    assert_eq!(plugins::for_network("unknown:none").err().unwrap().code, 2);
}

#[test]
fn every_rail_advertises_wallet_kinds_and_reports_the_kind_of_its_profile() {
    for rail in plugins::rails() {
        for network in rail.networks() {
            let offered = kinds::wallet_kinds(network.id).unwrap();
            assert!(!offered.is_empty(), "{} offers no wallet kind", network.id);
            let ids: Vec<_> = offered.iter().map(|kind| kind.id).collect();
            let mut ordered = ids.clone();
            ordered.sort();
            ordered.dedup();
            assert_eq!(ids, ordered, "{} kinds are in selection order", network.id);
            for alias in network.aliases {
                assert_eq!(kinds::wallet_kinds(alias).unwrap(), offered);
            }
        }
        let support = rail.test_support();
        let fixtures = (support.fixtures)();
        let bound = kinds::wallet_kind(support.network, &fixtures.public_profile).unwrap();
        assert!(rail.wallet_kinds(support.network).contains(&bound));
        assert!(kinds::wallet_kind(support.network, b"{}").is_err());
    }
    assert_eq!(kinds::wallet_kinds("unknown:none").unwrap_err().code, 2);
    assert_eq!(WalletKind::parse("standard"), Some(WalletKind::Standard));
}

#[test]
fn every_rail_supplies_the_unit_of_its_native_asset_and_tokens() {
    for rail in plugins::rails() {
        for network in rail.networks() {
            let native = kinds::native_asset(network.id).unwrap();
            assert!(native.native && !native.symbol.is_empty() && !native.name.is_empty());
            let one = 10i128.pow(u32::from(native.decimals));
            assert_eq!(native.format(5 * one), format!("5 {}", native.symbol));
            for token in rail.tokens(network.id) {
                let shown = kinds::asset_display(network.id, token.code).unwrap();
                assert_eq!(
                    (shown.symbol.as_str(), shown.decimals),
                    (token.code, token.decimals)
                );
                assert_eq!(shown.native, token.native);
                let text = kinds::format_amount(network.id, token.code, 1).unwrap();
                assert!(text.ends_with(&format!(" {}", token.code)), "{text}");
                assert!(!text.contains("UNIT"));
            }
            // An unknown asset has no unit. It is an error, never a placeholder.
            let fixtures = (rail.test_support().fixtures)();
            assert!(kinds::asset_display(network.id, &fixtures.invalid_address).is_err());
            assert!(kinds::format_amount(network.id, "", 1).is_err());
        }
    }
    assert!(kinds::native_asset("unknown:none").is_err());
}

#[test]
fn input_validation_uses_the_rail_formats() {
    testkit::each(Feature::Transfer, |rail, fx| {
        let network = rail.test_support().network;
        let family = routing::family(network).unwrap();
        address::check_account(family, &fx.account).unwrap();
        address::check_recipient(family, &fx.recipient).unwrap();
        address::check_wallet(family, &fx.wallet).unwrap();
        assert!(address::check_recipient(family, &fx.invalid_address).is_err());
        assert!(address::valid_wallet(&fx.wallet));
        let tokens = execution::tokens(network).unwrap();
        assert!(
            tokens.iter().any(|t| t.code == fx.token.code),
            "{} publishes its fixture token",
            rail.id()
        );
        execution::check(network, "transfer", &fx.recipient, None, false).unwrap();
        let capabilities = execution::capabilities(network).unwrap();
        assert!(capabilities.operations.contains(&"transfer"));
    });
}

#[test]
fn generated_keys_round_trip_through_the_private_file() {
    testkit::each(Feature::Transfer, |rail, _| {
        let network = rail.test_support().network;
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .canonicalize()
            .unwrap()
            .join("keys")
            .join("agent.key");
        let public = wallet::generate_wallet(network, &path).unwrap();
        let signer = wallet::local_signer(network, &path).unwrap();
        assert_eq!(signer.public(), public);
        assert_eq!(wallet::inspect_wallet(network, &path).unwrap(), public);
        let family = routing::family(network).unwrap();
        address::check_account(family, &public.address).unwrap();
        assert_eq!(
            runtime::signer_identity(network, &[7; 32]).unwrap(),
            runtime::signer_identity(network, &[7; 32]).unwrap()
        );
        assert!(
            wallet::generate_wallet(network, &path).is_err(),
            "never replaces a key"
        );
    });
}

#[test]
fn explorer_links_name_the_id() {
    testkit::each(Feature::ExplorerLinks, |rail, fx| {
        let network = rail.test_support().network;
        for (kind, id) in [
            (LinkKind::Address, fx.account.as_str()),
            (LinkKind::Contract, fx.wallet.as_str()),
        ] {
            let link = runtime::owner::explorer_link(network, kind, id).unwrap();
            assert!(link.starts_with("https://") && link.contains(id), "{link}");
        }
        assert!(runtime::owner::explorer_link("unknown:none", LinkKind::Address, "x").is_none());
    });
}

#[test]
fn each_rail_imports_its_public_profile_fixture() {
    testkit::each(Feature::Export, |rail, fx| {
        let network = rail.test_support().network;
        rail.decode_profile(network, &fx.public_profile)
            .unwrap_or_else(|e| panic!("{}: {e}", rail.id()));
    });
}

#[test]
fn a_malformed_public_profile_is_refused() {
    testkit::each(Feature::Export, |rail, _| {
        let network = rail.test_support().network;
        assert!(rail.decode_profile(network, b"{}").is_err());
        assert!(rail.decode_profile(network, b"not json").is_err());
    });
}

#[test]
fn the_offline_chain_serves_the_runtime() {
    testkit::each(Feature::Transfer, |rail, fx| {
        let Some(chain) = fx.chain.clone() else {
            eprintln!("skip {}: no offline chain fixture", rail.id());
            return;
        };
        let network = rail.test_support().network;
        Runtime::with_transport(network, chain).unwrap();
    });
}

#[test]
fn generation_on_another_network_stops_before_the_clock() {
    testkit::each(Feature::OwnerSetup, |rail, _| {
        let network = rail.test_support().network;
        let Some(other) = rail.networks().iter().find(|n| n.id != network) else {
            eprintln!("skip {}: one network", rail.id());
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .canonicalize()
            .unwrap()
            .join("keys")
            .join("owner.key");
        wallet::generate_wallet(network, &path).unwrap();
        let signer = wallet::local_signer(network, &path).unwrap();
        let request = GenerateRequest {
            prompt: "Use only the approved recipient".into(),
            network: other.id.into(),
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
    });
}

#[test]
fn an_invalid_owner_document_stops_before_the_store_opens() {
    testkit::each(Feature::Fund, |rail, _| {
        let network = rail.test_support().network;
        let dir = tempfile::tempdir().unwrap();
        let directory = dir.path().canonicalize().unwrap();
        let path = directory.join("keys").join("owner.key");
        let public = wallet::generate_wallet(network, &path).unwrap();
        let signer = wallet::local_signer(network, &path).unwrap();
        let root = directory.join("unopened");
        let authority =
            ServerAuthority::new(network, "http://localhost:8080", &public.address).unwrap();
        let cx = owner::Context {
            network,
            backend: authority.origin(),
            authority: &authority,
            origin: "http://localhost:5173",
            rpc_url: "http://localhost:8000",
            root: &root,
            signer: &signer,
            clock: &|| panic!("Invalid documents must not start owner authentication"),
        };
        assert!(owner::fund(&cx, b"{}", "invalid-doc-001", "1", "100").is_err());
        assert!(!root.exists(), "native preflight precedes storage effects");
    });
}

#[test]
fn server_hooks_offer_profiles_and_reject_bad_sign_ins() {
    testkit::each(Feature::OwnerSetup, |rail, fx| {
        let network = rail.test_support().network;
        let hooks = rail.server();
        assert!(
            !hooks.profiles().is_empty(),
            "{} offers a profile",
            rail.id()
        );
        assert!(
            hooks
                .verify_sign_in(network, &fx.account, b"message", &[0; 64])
                .is_err()
        );
        let deadlines = rail.deadlines(network);
        assert!(deadlines.max_validity_secs > 0 && deadlines.owner_step_ttl_secs > 0);
    });
}

#[test]
fn every_published_token_parses_and_resolves() {
    use allowit_sdk::input::asset::{self, Use};
    testkit::each(Feature::Transfer, |rail, _| {
        let network = rail.test_support().network;
        let family = routing::family(network).unwrap();
        let tokens = execution::tokens(network).unwrap();
        for token in tokens.iter().filter(|token| !token.native) {
            let amount =
                asset::parse(family, tokens, &format!("{}:1", token.code), Use::Spend).unwrap();
            assert_eq!(amount.decimals, Some(token.decimals), "{}", token.code);
            assert_eq!(amount.units(), 10u128.pow(token.decimals.into()));
            let resolved = asset::resolve(network, token.code, token.issuer, true).unwrap();
            assert_eq!(resolved.symbol, token.code);
            assert!(!resolved.address.is_empty());
        }
        if let Some(native) = tokens.iter().find(|token| token.native) {
            let amount = asset::parse(family, tokens, "native:2", Use::Spend).unwrap();
            assert_eq!(amount.units(), 2 * 10u128.pow(native.decimals.into()));
        }
        assert!(asset::parse(family, tokens, "UNKNOWNCODE:1", Use::Spend).is_err());
        assert!(asset::parse(family, tokens, "0", Use::Spend).is_err());
    });
}

#[test]
fn an_undeclared_operation_is_refused() {
    testkit::each(Feature::Transfer, |rail, fx| {
        let network = rail.test_support().network;
        let op = "undeclared-conformance-operation";
        assert!(
            !execution::capabilities(network)
                .unwrap()
                .operations
                .contains(&op)
        );
        let error = execution::check(network, op, &fx.recipient, None, false).unwrap_err();
        assert_eq!(error.code, 3);
        assert!(error.message.contains(network));
    });
}

/// Every registered policy package answers its samples through the registry dispatch. A package
/// that keeps a feature closed is skipped with its reason.
#[test]
fn every_registered_policy_package_answers_its_samples() {
    use allowit_sdk::plugins::{self, PackageFeature, testkit};
    for feature in [PackageFeature::AuthenticatedFacts, PackageFeature::LiveCall] {
        testkit::each_package(feature, |package, sample| {
            let reply = plugins::invoke_policy_function(
                package.namespace,
                sample.function,
                &sample.args,
                &sample.facts,
            );
            assert_eq!(
                reply.ok(),
                sample.reply,
                "{}::{}",
                package.namespace,
                sample.function
            );
        });
    }
}
