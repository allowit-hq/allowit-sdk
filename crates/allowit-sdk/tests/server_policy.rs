//! A policy that a server evaluates is checked with Core's server policy profile, as a server
//! of this build checks it. The portable profile, which a contract compiles, refuses it.
use allowit_sdk::authoring::{SERVER_FUNCTIONS, check, check_server};
const PORTABLE: &str = include_str!("fixtures/portable-policy.rs");
const SERVER: &str = include_str!("fixtures/server-policy.rs");

#[test]
fn the_server_profile_admits_owner_text_and_names_the_server_functions() {
    // The paper's kind of policy: it reads its queries, its URL, its key and its question
    // from owner params, and passes each text to a server function.
    let error = check(SERVER).unwrap_err();
    assert!(
        error.message.contains("the method `as_str`"),
        "{}",
        error.message
    );
    let checked = check_server(SERVER).unwrap();
    assert_eq!(
        checked.functions,
        [
            "allowit::new_decision_context",
            "allowit::decision",
            "allowit::fetch",
            "allowit::internet_search",
            "allowit::marketplace_search",
        ]
    );
    assert!(!checked.shape.canonical_only);
    assert!(!checked.shape.writes_state);
    assert_eq!(
        checked.shape.prefix,
        ["operations", "tokens", "destinations", "payment_limit"]
    );
    // Source for a contract names no server function, and both profiles give one shape.
    let portable = check_server(PORTABLE).unwrap();
    assert!(portable.functions.is_empty());
    assert_eq!(portable.shape, check(PORTABLE).unwrap());
    // The server profile is no wider elsewhere: it refuses what the portable profile refuses.
    for refused in [
        SERVER.replace("question.as_str()", "question.to_string().as_str()"),
        SERVER.replace("allowit::decision(", "allowit::unknown("),
        SERVER.replace("use allowit::prelude::*;", "use std::fs;"),
    ] {
        assert!(check_server(&refused).is_err(), "{refused}");
    }
}

#[test]
fn every_listed_server_function_is_one_of_core() {
    // Core's gate treats its own server functions apart: a registry without one refuses the
    // path, although the portable profile knows it. A name that Core does not know as a
    // server function is refused by both.
    let registry = SERVER_FUNCTIONS;
    assert!(
        allowit_sdk::portable::policy_source::check_host_call_registry(&registry).is_ok(),
        "the registry of Core's server functions is usable"
    );
    for (namespace, name) in registry {
        assert_eq!(namespace, "allowit");
        let source = format!(
            "use allowit::prelude::*;\npub fn execute(ctx: &AllowitContext) -> allowit::Result {{\n    allowit::{name}(ctx)?;\n    Ok(())\n}}\n"
        );
        let without: Vec<_> = registry
            .into_iter()
            .filter(|(_, other)| *other != name)
            .collect();
        let error =
            allowit_sdk::portable::policy_source::source_shape_with_host_calls(&source, &without)
                .unwrap_err();
        assert!(
            error.contains(&format!("the path `allowit::{name}`")),
            "{name}: {error}"
        );
        // With the function registered, the gate reads past the path.
        let with =
            allowit_sdk::portable::policy_source::source_shape_with_host_calls(&source, &registry);
        assert!(
            with.as_ref()
                .err()
                .is_none_or(|error| !error.contains("the path `allowit::")),
            "{name}: {with:?}"
        );
    }
}

/// One source for a Custom wallet: the server evaluates all of it, and the contract compiles
/// its on-chain part. The part of the server policy fixture is the fixture without its server
/// calls, and it is portable source.
#[test]
fn the_on_chain_part_of_a_server_policy_is_portable_and_reports_what_left() {
    use allowit_sdk::authoring::{check, check_server, onchain_part};
    let source = include_str!("fixtures/server-policy.rs");
    assert!(!check_server(source).unwrap().functions.is_empty());
    let derived = onchain_part(source).unwrap();
    assert_ne!(derived.source, source);
    assert!(!derived.left.is_empty());
    check(&derived.source).unwrap();
    assert!(check_server(&derived.source).unwrap().functions.is_empty());
    // The part is its own on-chain part, and a portable source derives to itself.
    assert_eq!(
        onchain_part(&derived.source).unwrap().source,
        derived.source
    );
    let portable = "use allowit::prelude::*;\npub fn execute(ctx: &AllowitContext) -> allowit::Result {\n    allowit::payment_limit(ctx, &ctx.params[allowit::PAYMENT_LIMIT])?;\n    Ok(())\n}\n";
    let same = onchain_part(portable).unwrap();
    assert_eq!(same.source, portable);
    assert!(same.left.is_empty() && same.server_only.is_empty());
    assert_eq!(same.kept, ["payment_limit"]);
    assert!(same.figures.is_empty());
    // A contract wallet keeps only the figures of its kept rules, and no policy state of its
    // own: a source that writes state is refused.
    let daily = portable
        .replace("PAYMENT_LIMIT", "DAILY_SPENDING_LIMIT")
        .replace("payment_limit", "daily_spending_limit");
    let keys: Vec<&str> = onchain_part(&daily)
        .unwrap()
        .figures
        .iter()
        .map(|figure| figure.key)
        .collect();
    assert_eq!(keys, ["day_spent", "day_unpriced"]);
    let writes = portable.replace(
        "    Ok(())",
        "    allowit::set_state(ctx, \"last\", ctx.request.amount)?;\n    Ok(())",
    );
    assert!(
        onchain_part(&writes)
            .unwrap_err()
            .message
            .contains("cannot write policy state")
    );
}
