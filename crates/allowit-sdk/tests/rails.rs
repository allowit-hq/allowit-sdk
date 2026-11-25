//! Shared SDK behavior, run on every registered rail. No assertion names a vendor: values come
//! from each rail's fixtures, and a rail skips a feature it declares unsupported.
use allowit_sdk::{
    config::ServerAuthority,
    custom_build, execution,
    generation::{self, GenerateRequest, Target},
    input::address,
    kinds::{self, WalletKind},
    owner, paid_service,
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

/// A keypair wallet is the agent's own account. A rail that sets one up advertises the kind on
/// its network with no precondition for an owner, and its installation names a wallet that is
/// neither its owner nor its agent nor its server: one account per agent, never a shared one.
#[test]
fn a_keypair_wallet_is_its_own_account_on_every_rail_that_sets_one_up() {
    testkit::each(Feature::OwnerSetup, |rail, f| {
        let network = rail.test_support().network;
        let advertised: Vec<_> = kinds::wallet_kinds(network)
            .unwrap()
            .into_iter()
            .map(|info| info.id)
            .collect();
        assert!(
            advertised.contains(&WalletKind::Native),
            "{} sets up owner wallets without the kind",
            rail.id()
        );
        if kinds::wallet_kind(network, &f.public_profile).unwrap() != WalletKind::Native {
            return;
        }
        let profile: serde_json::Value = serde_json::from_slice(&f.public_profile).unwrap();
        let configuration: runtime::owner::Configuration =
            serde_json::from_value(profile["config"].clone()).unwrap();
        assert_eq!(configuration.custody, runtime::owner::Custody::Owner);
        for role in [
            &configuration.owner,
            &configuration.agent,
            &configuration.server,
        ] {
            assert_ne!(&configuration.wallet, role, "{}", rail.id());
        }
    });
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
        // A key that can sign: the Ed25519 base point. A rail may refuse 32 bytes that are no
        // point of the curve, because no signer holds such a key.
        let mut point = [0x66; 32];
        point[0] = 0x58;
        assert_eq!(
            runtime::signer_identity(network, &point).unwrap(),
            runtime::signer_identity(network, &point).unwrap()
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

/// A server runtime takes the server's own sending accounts on every rail.
#[test]
fn a_server_runtime_takes_its_sending_accounts() {
    struct Sending;
    impl runtime::SendingAccounts for Sending {
        fn account(&self) -> allowit_sdk::error::Result<String> {
            Err(allowit_sdk::error::Error::unsupported(
                "no sending account in this test",
            ))
        }
        fn sign(&self, _: &str, _: &[u8; 32]) -> allowit_sdk::error::Result<Vec<u8>> {
            Err(allowit_sdk::error::Error::unsupported(
                "no sending account in this test",
            ))
        }
    }
    testkit::each(Feature::Transfer, |rail, fx| {
        let Some(chain) = fx.chain.clone() else {
            eprintln!("skip {}: no offline chain fixture", rail.id());
            return;
        };
        let network = rail.test_support().network;
        Runtime::with_sender(network, chain.clone(), std::sync::Arc::new(Sending)).unwrap();
        assert!(
            Runtime::with_sender("unknown:network", chain, std::sync::Arc::new(Sending)).is_err()
        );
    });
}

/// A transport that counts the calls and answers none, as an unreachable network.
#[derive(Default)]
struct Unreachable(std::sync::atomic::AtomicUsize);
impl runtime::Transport for Unreachable {
    fn call(&self, _: &str, _: serde_json::Value) -> allowit_sdk::error::Result<serde_json::Value> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(allowit_sdk::error::Error::config(
            "The network is unreachable in this test",
        ))
    }
}

/// Core's refusals of a rail without code transactions.
const NO_CODE_TRANSACTIONS: [&str; 3] = [
    "This rail does not prepare wallet code transactions",
    "This rail does not renew wallet code",
    "This rail does not say whether a code transaction is still needed",
];

/// The server's code upload at deployment and its renewal of shared code reach the rail's own
/// code transactions through the registered sending runtime, with the rail's own wallet code: the
/// preparation reaches what the server gives it, its network or its sending accounts, in whatever
/// order the rail reads them, and none answers Core's refusal of a rail without code
/// transactions. A renewal that the rail finds not due answers nothing. The rail's own tests
/// follow a code transaction through a model of its network.
#[test]
fn a_rail_that_deploys_prepares_code_uploads_and_renewals_itself() {
    /// Sending accounts that count the requests and give none.
    #[derive(Default)]
    struct Sending(std::sync::atomic::AtomicUsize);
    impl runtime::SendingAccounts for Sending {
        fn account(&self) -> allowit_sdk::error::Result<String> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(allowit_sdk::error::Error::unsupported(
                "no sending account in this test",
            ))
        }
        fn sign(&self, _: &str, _: &[u8; 32]) -> allowit_sdk::error::Result<Vec<u8>> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(allowit_sdk::error::Error::unsupported(
                "no sending account in this test",
            ))
        }
    }
    let refused_by_core =
        |error: &allowit_sdk::error::Error| NO_CODE_TRANSACTIONS.contains(&error.message.as_str());
    let reached = |network: &Unreachable, sending: &Sending| {
        network.0.load(std::sync::atomic::Ordering::SeqCst)
            + sending.0.load(std::sync::atomic::Ordering::SeqCst)
            > 0
    };
    testkit::each(Feature::Deploy, |rail, fx| {
        let network = rail.test_support().network;
        if fx.wallet_code.is_empty() {
            eprintln!("skip {}: no wallet code fixture", rail.id());
            return;
        }
        let code = custom_build::code_hash(network, &fx.wallet_code).unwrap();
        let poll = runtime::Poll {
            attempts: 1,
            interval: std::time::Duration::ZERO,
        };
        let (transport, sender) = (
            std::sync::Arc::new(Unreachable::default()),
            std::sync::Arc::new(Sending::default()),
        );
        let sending = Runtime::with_sender(network, transport.clone(), sender.clone()).unwrap();
        let mut journal = runtime::MemoryOperations::default();
        match sending.renew_code(&mut journal, &code, 1_000_000, poll, 1, &|| true) {
            Ok(renewed) => assert!(renewed.is_none(), "{}", rail.id()),
            Err(refused) => {
                assert!(!refused_by_core(&refused), "{}: {refused:?}", rail.id());
                assert!(reached(&transport, &sender), "{}", rail.id());
            }
        }

        let (transport, sender) = (
            std::sync::Arc::new(Unreachable::default()),
            std::sync::Arc::new(Sending::default()),
        );
        let sending = Runtime::with_sender(network, transport.clone(), sender.clone()).unwrap();
        // A new installation from the profile's description: its wallet comes from the
        // deployment, as the server derives it.
        let (mut description, _) = rail.decode_profile(network, &fx.public_profile).unwrap();
        description.wallet = String::new();
        // The profile's own asset, by its symbol.
        let asset = allowit_sdk::input::asset::Asset::Native {
            code: description.asset_symbol.clone(),
        };
        let installation = runtime::Installation::policy_wallet(runtime::PolicyInstallation {
            description,
            policy_instance: [1; 32],
            tier: runtime::Tier::Simple,
            asset,
            max_fee_units: 1_000_000,
        })
        .unwrap();
        let refused = sending
            .make_code_live(
                &mut journal,
                &installation,
                &fx.wallet_code,
                1_000_000,
                poll,
                1,
            )
            .unwrap_err();
        assert!(!refused_by_core(&refused), "{}: {refused:?}", rail.id());
        assert!(reached(&transport, &sender), "{}", rail.id());
        assert!(!journal.pending());
    });
}

/// A server holds the seed of each of its keys, such as a sending account's, and the rail signs
/// with it: one seed is one key of the rail.
#[test]
fn a_signer_from_a_seed_is_the_rails_key_of_that_seed() {
    testkit::each(Feature::Transfer, |rail, fx| {
        let network = rail.test_support().network;
        // Seeds of the rail's own key size, from the fixture's agent seed.
        let size = rail.keys().entropy_bytes();
        let seed: Vec<u8> = fx.agent_seed.iter().cycle().take(size).copied().collect();
        let other: Vec<u8> = seed.iter().map(|b| b ^ 1).collect();
        let signer = wallet::LocalSigner::from_seed(network, &seed).unwrap();
        let again = wallet::LocalSigner::from_seed(network, &seed).unwrap();
        assert_eq!(signer.public(), again.public(), "{}", rail.id());
        assert_eq!(signer.network(), network);
        signer.public().validate().unwrap();
        assert!(!signer.sign_hash(&[5; 32]).unwrap().is_empty());
        let other = wallet::LocalSigner::from_seed(network, &other).unwrap();
        assert_ne!(other.address(), signer.address());
        assert!(wallet::LocalSigner::from_seed(network, &seed[1..]).is_err());
        assert!(wallet::LocalSigner::from_seed("unknown:network", &seed).is_err());
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
            wallet_kind: None,
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

#[test]
fn a_generation_request_names_only_a_wallet_kind_that_the_rail_and_the_profile_offer() {
    testkit::each(Feature::OwnerSetup, |rail, _| {
        let network = rail.test_support().network;
        let offered = rail.wallet_kinds(network);
        let request = |profile: &str, wallet_kind| GenerateRequest {
            prompt: "Use only the approved recipient".into(),
            network: network.into(),
            profile: profile.into(),
            target: Target::NewWallet,
            wallet_kind,
        };
        for profile in rail.server().profiles() {
            // Without a choice the request is valid, as before.
            request(profile.id, None).validate().unwrap();
            // The kinds that the profiles with this id install; an id may serve several.
            let installed: Vec<WalletKind> = rail
                .server()
                .profiles()
                .iter()
                .filter(|other| other.id == profile.id)
                .filter_map(|other| other.wallet_kind())
                .collect();
            if installed.is_empty() {
                continue;
            }
            for kind in WalletKind::ALL {
                let result = request(profile.id, Some(kind)).validate();
                let served = offered.contains(&kind) && installed.contains(&kind);
                assert_eq!(result.is_ok(), served, "{} {}", rail.id(), kind.id());
                if let Err(error) = result {
                    assert_eq!(error.code, 2, "{} {}", rail.id(), kind.id());
                }
            }
        }
        for kind in WalletKind::ALL {
            let result = request("another-advertised-profile", Some(kind)).validate();
            assert_eq!(result.is_ok(), offered.contains(&kind), "{}", kind.id());
        }
    });
}

/// A rail that advertises a wallet kind must be able to install it. Generation refuses a kind
/// that no profile of the rail installs, so each advertised kind needs a profile, and a request
/// for that profile and kind must pass on every network of the rail.
#[test]
fn generation_accepts_every_wallet_kind_that_a_rail_advertises() {
    testkit::each(Feature::OwnerSetup, |rail, _| {
        let profiles = rail.server().profiles();
        for network in rail.networks() {
            let advertised = rail.wallet_kinds(network.id);
            for kind in WalletKind::ALL {
                let installs = profiles
                    .iter()
                    .find(|profile| profile.wallet_kind() == Some(kind));
                assert_eq!(
                    installs.is_some(),
                    advertised.contains(&kind),
                    "{}: the {} kind is advertised {} and a profile installs it {}",
                    network.id,
                    kind.id(),
                    advertised.contains(&kind),
                    installs.is_some(),
                );
                let Some(profile) = installs else { continue };
                GenerateRequest {
                    prompt: "Use only the approved recipient".into(),
                    network: network.id.into(),
                    profile: profile.id.into(),
                    target: Target::NewWallet,
                    wallet_kind: Some(kind),
                }
                .validate()
                .unwrap_or_else(|error| panic!("{} {}: {error}", network.id, kind.id()));
            }
        }
    });
}

#[test]
fn a_generated_draft_names_its_wallet_kind_at_the_top_level() {
    for kind in WalletKind::ALL {
        let draft = generation::GeneratedDraft {
            document: serde_json::json!({"walletKind": kind.id()}),
            unresolved: Vec::new(),
        };
        assert_eq!(draft.wallet_kind(), Some(kind));
        assert_eq!(generation::wallet_kind_of(&draft.document), Some(kind));
    }
    assert_eq!(
        generation::wallet_kind_of(&serde_json::json!({"walletKind": "unknown-kind"})),
        None
    );
    assert_eq!(generation::wallet_kind_of(&serde_json::json!({})), None);
}

#[test]
fn a_draft_prints_its_params_in_human_units() {
    testkit::each(Feature::DraftDisplay, |rail, fx| {
        let network = rail.test_support().network;
        // The fixture is a generation reply; its draft is the document under `policy`.
        let draft = fx
            .owner_generation
            .get("policy")
            .unwrap_or(&fx.owner_generation);
        let read = rail.draft_params(network, draft).unwrap();
        let lines = kinds::param_lines(network, draft).unwrap();
        assert_eq!(lines.len(), read.params.len());
        assert!(!lines.is_empty(), "{}: no params in the fixture", rail.id());
        let registry = allowit_sdk::policy_metadata::canonical_function_registry().unwrap();
        for line in &lines {
            assert!(!line.label.is_empty() && !line.value.is_empty(), "{line:?}");
            assert!(!line.value.contains(['{', '"']), "{line:?}");
            let Some(function) = registry.functions.iter().find(|f| f.name == line.key) else {
                assert_eq!(line.label, line.key);
                continue;
            };
            assert_eq!(line.label, function.label);
            match function.parameter.semantic_type.as_str() {
                "amount" => assert!(
                    line.value.ends_with(&format!(" {}", read.asset.symbol)),
                    "{line:?}"
                ),
                "timestamp" => assert!(line.value.ends_with('Z'), "{line:?}"),
                _ => {}
            }
        }
        assert!(kinds::param_lines(network, &serde_json::json!({})).is_err());
        assert!(kinds::param_lines("unknown:network", draft).is_err());
    });
}

#[test]
fn an_imported_profile_prints_its_params_in_human_units() {
    testkit::each(Feature::DraftDisplay, |rail, fx| {
        let network = rail.test_support().network;
        let Ok((description, _)) = rail.decode_profile(network, &fx.public_profile) else {
            eprintln!("skip {}: no public profile fixture", rail.id());
            return;
        };
        // `kinds::profile_param_lines` passes the same document of an imported profile.
        let document: serde_json::Value = serde_json::from_slice(&fx.public_profile).unwrap();
        let read = rail.draft_params(network, &document).unwrap();
        let lines = kinds::param_lines(network, &document).unwrap();
        assert_eq!(lines.len(), description.params.len());
        let registry = allowit_sdk::policy_metadata::canonical_function_registry().unwrap();
        for line in &lines {
            assert!(description.params.contains_key(&line.key), "{line:?}");
            assert!(!line.label.is_empty() && !line.value.is_empty(), "{line:?}");
            assert!(!line.value.contains(['{', '"']), "{line:?}");
            let amount = registry
                .functions
                .iter()
                .any(|f| f.name == line.key && f.parameter.semantic_type == "amount");
            if amount {
                assert!(
                    line.value.ends_with(&format!(" {}", read.asset.symbol)),
                    "{line:?}"
                );
            }
        }
    });
}

#[test]
fn a_profile_document_gives_its_per_payment_limits_in_the_installed_asset() {
    testkit::each(Feature::DraftDisplay, |rail, fx| {
        let network = rail.test_support().network;
        if rail.decode_profile(network, &fx.public_profile).is_err() {
            eprintln!("skip {}: no public profile fixture", rail.id());
            return;
        }
        let document: serde_json::Value = serde_json::from_slice(&fx.public_profile).unwrap();
        let read = rail.draft_params(network, &document).unwrap();
        let limits = execution::payment_limits_of(network, &document).unwrap();
        // One entry for each per-payment param of the document, and no counted limit.
        let expected: Vec<_> = read
            .params
            .iter()
            .filter(|(key, _)| key == "payment_limit")
            .collect();
        assert_eq!(limits.len(), expected.len());
        for (limit, (key, value)) in limits.iter().zip(expected) {
            assert_eq!(&limit.param, key);
            assert_eq!(limit.limit.asset, read.asset);
            let units = &value["Int"];
            assert_eq!(
                limit.limit.units,
                units
                    .as_str()
                    .map_or_else(|| units.to_string(), String::from)
            );
            assert!(
                limit
                    .limit
                    .text
                    .ends_with(&format!(" {}", read.asset.symbol)),
                "{limit:?}"
            );
        }
        // A document carries them under their own key, beside the counted limits.
        let report = execution::WalletLimits {
            limits: None,
            payment_limits: limits.clone(),
        };
        let document = serde_json::to_value(&report).unwrap();
        assert_eq!(document[execution::Limits::KEY], serde_json::Value::Null);
        assert_eq!(execution::WalletLimits::payment_of(&document), Some(limits));
        assert_eq!(execution::Limits::of(&document), None);
        let plain = serde_json::json!({"limits": []});
        assert_eq!(execution::WalletLimits::payment_of(&plain), None);
        assert!(execution::payment_limits_of(network, &serde_json::json!({})).is_err());
        assert!(execution::payment_limits_of("unknown:network", &plain).is_err());
    });
}

#[test]
fn an_outcome_document_gives_its_limits_charged_fee_and_explorer_link() {
    testkit::each(Feature::Limits, |rail, fx| {
        let network = rail.test_support().network;
        let asset = kinds::asset_display(network, fx.token.code).unwrap();
        let quantity = |units| execution::Quantity::new(units, asset.clone());
        let limits = vec![execution::LimitReport {
            param: "daily_spending_limit".into(),
            limit: quantity(500),
            spent: quantity(120),
            remaining: quantity(380),
        }];
        let link = rail.explorer(network, LinkKind::Transaction, "ab");
        let mut document = serde_json::json!({"requestId": "r",
            execution::Limits::KEY: limits, execution::FEE_CHARGED_KEY: quantity(7)});
        if let Some(link) = &link {
            document[execution::EXPLORER_KEY] = link.clone().into();
        }
        assert_eq!(execution::Limits::of(&document), Some(limits));
        assert_eq!(execution::fee_charged(&document), Some(quantity(7)));
        assert_eq!(execution::explorer(&document), link);
        assert!(
            execution::fee_charged(&document)
                .unwrap()
                .text
                .ends_with(&asset.symbol)
        );
        let plain = serde_json::json!({"requestId": "r"});
        assert_eq!(execution::Limits::of(&plain), None);
        assert_eq!(execution::fee_charged(&plain), None);
        assert_eq!(execution::explorer(&plain), None);
    });
}

/// Every registered policy package answers its samples through the registry dispatch. A package
/// that keeps a feature closed is skipped with its reason.
#[test]
fn every_registered_policy_package_answers_its_samples() {
    use allowit_sdk::plugins::PackageFeature;
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

#[test]
fn a_rail_reads_no_challenge_from_a_reply_that_is_none() {
    testkit::each(Feature::PaidService, |rail, _| {
        let network = rail.test_support().network;
        assert!(paid_service::supported(network), "{}", rail.id());
        let resource = "https://seller.example/report";
        let reply = |status, body: &'static [u8]| paid_service::SellerReply {
            status,
            headers: &[],
            body,
        };
        // A delivered resource, an empty challenge and text that is no challenge.
        // Each is the seller's failure: its own code, apart from usage and payment errors.
        for (status, body) in [(200, &b"{}"[..]), (402, b""), (402, b"<html>pay</html>")] {
            let error = paid_service::challenge(network, resource, &reply(status, body));
            assert_eq!(
                error.unwrap_err().code,
                paid_service::SELLER_FAILED,
                "{} {status}",
                rail.id()
            );
        }
        // A resource that is no http or https URL never reaches the rail.
        for resource in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "seller.example",
        ] {
            let error = paid_service::challenge(network, resource, &reply(402, b"{}"));
            assert_eq!(error.unwrap_err().code, 2, "{} {resource}", rail.id());
        }
    });
}

#[test]
fn a_rail_skips_paid_services_exactly_when_it_has_no_hook() {
    for rail in plugins::rails() {
        let support = rail.test_support();
        assert_eq!(
            support.skip(Feature::PaidService).is_some(),
            rail.paid_service().is_none(),
            "{}",
            rail.id()
        );
        assert_eq!(
            paid_service::supported(support.network),
            rail.paid_service().is_some(),
            "{}",
            rail.id()
        );
    }
    assert!(!paid_service::supported("unknown:network"));
}

/// The owner context of a rail's test network with a new local owner key. The clock panics:
/// a refusal of an invalid document must precede owner authentication.
fn with_owner_context(network: &str, run: impl FnOnce(&owner::Context<'_>, &std::path::Path)) {
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
    run(&cx, &root);
}

/// The asset of a new Native wallet is the rail's lookup, which refuses what the rail's setup
/// refuses before the owner signs in: here an owner whose key is the server authority's.
#[test]
fn the_native_wallet_asset_is_refused_where_the_setup_refuses_before_the_sign_in() {
    testkit::each(Feature::OwnerSetup, |rail, _| {
        with_owner_context(rail.test_support().network, |cx, root| {
            // The rail's own refusal, not Core's default refusal of a rail without the lookup.
            let refused = owner::native_wallet_asset(cx).unwrap_err();
            assert_ne!(refused.code, 3, "{}: {}", rail.id(), refused.message);
            assert!(!root.exists());
        });
    });
}

#[test]
fn an_invalid_document_stops_a_params_update_before_the_store_opens() {
    testkit::each(Feature::UpdateParams, |rail, _| {
        with_owner_context(rail.test_support().network, |cx, root| {
            let changes = std::collections::BTreeMap::new();
            let request = |document| owner::UpdateParams {
                document,
                changes: &changes,
                request_id: "invalid-doc-002",
                expected_revision: None,
                fee_limit: "100",
            };
            assert!(owner::update_params(cx, request(b"{}")).is_err());
            assert!(owner::update_params(cx, request(b"not json")).is_err());
            assert!(!root.exists(), "native preflight precedes storage effects");
        });
    });
}

#[test]
fn an_invalid_document_is_not_verified_and_an_invalid_draft_is_not_edited() {
    testkit::each(Feature::Verify, |rail, _| {
        with_owner_context(rail.test_support().network, |cx, root| {
            assert!(owner::verify(cx, b"{}").is_err());
            assert!(!root.exists());
        });
    });
    testkit::each(Feature::EditParams, |rail, _| {
        with_owner_context(rail.test_support().network, |cx, root| {
            let changes = std::collections::BTreeMap::new();
            let edit = owner::EditParams {
                draft: b"{}",
                changes: &changes,
            };
            assert!(owner::edit_params(cx, edit).is_err());
            let revise = owner::Revise {
                draft: b"{}",
                instruction: "Lower the daily limit to 2",
            };
            assert!(owner::revise(cx, revise).is_err());
            assert!(!root.exists());
        });
    });
}

/// The generated policy of a rail's owner-generation fixture, as the draft document.
fn generated_draft(fx: &allowit_sdk::plugins::Fixtures) -> serde_json::Value {
    fx.owner_generation
        .get("policy")
        .unwrap_or(&fx.owner_generation)
        .clone()
}

#[test]
fn a_paired_export_separates_the_host_placement_from_the_native_placement() {
    use allowit_sdk::kinds::WalletKind as Kind;
    testkit::each(Feature::PairedExport, |rail, fx| {
        let network = rail.test_support().network;
        let draft = generated_draft(fx);
        let pair = owner::paired_export(network, &serde_json::to_vec(&draft).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", rail.id()));
        assert_eq!(pair.network, routing::normalize(network).unwrap());
        assert!(
            rail.wallet_kinds(network)
                .contains(&pair.native.wallet_kind)
        );
        assert_eq!(
            pair.host.source_hash,
            allowit_sdk::digest(pair.host.source.as_bytes())
        );
        // The server evaluates every rule and every param in each placement.
        assert!(!pair.host.rules.is_empty(), "{}", rail.id());
        assert!(!pair.host.params.is_empty(), "{}", rail.id());
        match pair.native.wallet_kind {
            Kind::Native | Kind::Simple => {
                assert!(pair.native.rules.is_empty() && pair.native.params.is_empty());
                assert_eq!(pair.native.source, None);
            }
            Kind::Standard => {
                // The wallet enforces an ordered subset of the rules the server evaluates.
                assert!(
                    pair.native
                        .rules
                        .iter()
                        .all(|r| pair.host.rules.contains(r))
                );
                assert!(
                    pair.native
                        .params
                        .iter()
                        .all(|(k, v)| pair.host.params.get(k) == Some(v))
                );
                assert_eq!(pair.native.source, None);
            }
            Kind::Custom => {
                assert_eq!(
                    pair.native.source.as_deref(),
                    Some(pair.host.source.as_str())
                );
                assert_eq!(pair.native.params, pair.host.params);
            }
        }
        // The pair has a stable wire form.
        let wire = serde_json::to_value(&pair).unwrap();
        assert_eq!(
            serde_json::from_value::<owner::PairedExport>(wire).unwrap(),
            pair
        );
        assert!(owner::paired_export(network, b"{}").is_err());
    });
}

#[test]
fn typed_changes_parse_owner_text_and_refuse_an_invalid_or_unknown_value() {
    testkit::each(Feature::EditParams, |rail, fx| {
        let network = rail.test_support().network;
        let draft = generated_draft(fx);
        let lines = kinds::param_lines(network, &draft).unwrap();
        let edit = |key: &str, text: &str| {
            owner::typed_changes(
                network,
                &draft,
                &[(key.to_owned(), text.to_owned())].into(),
                1_700_000_000,
            )
        };
        // A key that the policy does not read is refused on every rail.
        assert!(edit("not_a_param_of_this_policy", "1").is_err());
        let Some(line) = lines
            .iter()
            .find(|line| line.key == allowit_sdk::portable::PAYMENT_LIMIT)
        else {
            eprintln!("skip {}: the fixture has no per-payment limit", rail.id());
            return;
        };
        let (typed, changes) = edit(&line.key, "0.75").unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(typed.len(), 1);
        assert!(matches!(
            typed[&line.key].0,
            allowit_sdk::portable::AllowitValue::Int(units) if units > 0
        ));
        // Text that is no amount, and a negative amount, are refused with the owner's message.
        for refused in ["much", "-1"] {
            assert!(edit(&line.key, refused).is_err(), "{refused}");
        }
        // The outcome of an owner step holds its policy under `selection`. Its edits are the
        // same typed values.
        let outcome = serde_json::json!({"requestId": "r", "state": "installed",
            "selection": {"policy": draft}});
        let edits = [(line.key.clone(), "0.75".to_owned())].into();
        let (from_outcome, _) =
            owner::typed_changes(network, &outcome, &edits, 1_700_000_000).unwrap();
        assert_eq!(from_outcome, typed);
        // A param that is not canonical keeps the kind that its policy declares: an amount
        // parses in the unit of the asset, in a draft and in an outcome.
        let mut custom = draft.clone();
        custom["code"] = "use allowit::prelude::*;\npub fn execute(ctx: &AllowitContext) -> allowit::Result {\n    let reserve: i128 = allowit::param(ctx, \"reserve_amount\");\n    if ctx.request.amount > reserve {\n        return Err(AllowitError::PaymentLimitExceeded);\n    }\n    Ok(())\n}\n".into();
        let portable = &mut custom["runtime"]["instanceBinding"]["portable"];
        if portable.is_object() {
            portable["params"]["reserve_amount"] = serde_json::json!({"Int": "10"});
            portable["kinds"] = serde_json::json!({"reserve_amount": "amount"});
            let read = rail.draft_params(network, &custom).unwrap();
            let unit = 10i128.pow(u32::from(read.asset.decimals));
            let reserve = [("reserve_amount".to_owned(), "0.75".to_owned())].into();
            for document in [
                custom.clone(),
                serde_json::json!({"requestId": "r", "selection": {"policy": custom}}),
            ] {
                let (typed, _) =
                    owner::typed_changes(network, &document, &reserve, 1_700_000_000).unwrap();
                assert!(matches!(
                    typed["reserve_amount"].0,
                    allowit_sdk::portable::AllowitValue::Int(units) if units == unit * 3 / 4
                ));
            }
        }
        // A document without a policy source has nothing to edit.
        assert!(
            owner::typed_changes(network, &serde_json::json!({}), &edits, 1_700_000_000).is_err()
        );
    });
}

#[test]
fn a_rail_skips_the_custom_build_exactly_when_it_has_no_hook() {
    for rail in plugins::rails() {
        let support = rail.test_support();
        assert_eq!(
            support.skip(Feature::CustomBuild).is_some(),
            rail.custom_wallet_build().is_none(),
            "{}",
            rail.id()
        );
        assert_eq!(
            custom_build::supported(support.network),
            rail.custom_wallet_build().is_some(),
            "{}",
            rail.id()
        );
    }
    assert!(!custom_build::supported("unknown:network"));
}

/// A host checks a source against the Custom wallet of a network without building it: the SDK
/// gives exactly what the rail's own hook gives for the same source, a refusal included. Each rail
/// states its own limits; its tests hold them.
#[test]
fn a_host_checks_a_source_against_the_rails_custom_wallet_without_building() {
    let writes = |keys: usize| -> String {
        (1..=keys)
            .map(|i| format!("    allowit::set_state(ctx, \"k{i}\", ctx.request.amount)?;\n"))
            .collect()
    };
    let source = |writes: &str| {
        format!(
            "use allowit::prelude::*;\npub fn execute(ctx: &AllowitContext) -> allowit::Result {{\n{writes}    Ok(())\n}}\n"
        )
    };
    let outcome =
        |result: allowit_sdk::error::Result<()>| result.map_err(|error| error.to_string());
    testkit::each(Feature::CustomBuild, |rail, _| {
        let network = rail.test_support().network;
        let hook = rail
            .custom_wallet_build()
            .expect("the rail builds a Custom wallet");
        for keys in [1, 9] {
            let source = source(&writes(keys));
            assert_eq!(
                outcome(custom_build::check(network, &source)),
                outcome(hook.check(network, &source)),
                "{} with {keys} keys",
                rail.id()
            );
        }
    });
    assert!(custom_build::check("unknown:network", &source("")).is_err());
}

#[test]
fn a_host_builds_only_the_signed_source_and_compares_the_code() {
    use custom_build::code;
    testkit::each(Feature::CustomBuild, |rail, fx| {
        let network = rail.test_support().network;
        // The rail hashes the fixture wallet code. A host compares its own build with the code
        // that the server built and uploaded.
        assert!(!fx.wallet_code.is_empty(), "{}", rail.id());
        let hash = custom_build::code_hash(network, &fx.wallet_code).unwrap();
        custom_build::check_code_hash(&hash, &hash).unwrap();
        let mut other = hash;
        other[0] ^= 1;
        assert_eq!(
            custom_build::check_code_hash(&hash, &other)
                .unwrap_err()
                .api_code
                .as_deref(),
            Some(code::CODE_HASH_MISMATCH)
        );
        assert!(custom_build::code_hash(network, b"no wallet code").is_err());
        // A server source that is not the source that the owner signed is refused before
        // anything builds.
        let admission: custom_build::Admission = serde_json::from_value(serde_json::json!({
            "policyId": "policy-1",
            "network": network,
            "source": "pub fn execute() {}\n",
            "build": null,
        }))
        .unwrap();
        admission
            .check_source(&allowit_sdk::digest(b"pub fn execute() {}\n"))
            .unwrap();
        assert!(
            admission
                .check_source(&allowit_sdk::digest(
                    b"pub fn execute() { /* reviewed */ }\n"
                ))
                .unwrap_err()
                .message
                .contains("differs from the source that the owner signed")
        );
    });
}
