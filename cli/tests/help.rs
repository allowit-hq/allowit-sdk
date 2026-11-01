//! Help checks: `--help` on every command node of the built binary, hidden
//! aliases included, and a snapshot of each command group. Set
//! `ALLOWIT_UPDATE_SNAPSHOTS=1` to rewrite the files in
//! `tests/snapshots/help/` after an intended help change.
mod common;

use std::{collections::BTreeSet, path::PathBuf};

fn snapshot_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/help")
}

fn file_name(path: &[String]) -> String {
    let mut parts = vec!["allowit".to_owned()];
    parts.extend(path.iter().cloned());
    format!("{}.txt", parts.join("_"))
}

#[test]
fn data_lines_refuse_prose() {
    for prose in [
        "state: settled\nThe transfer is confirmed on chain.",
        "next: request pay-0001 stays uncertain",
        "Imported Tempo policy 0f0f",
        "Tempo request pay-0001: status",
        "allowit: setup operation ID; do not repeat an uncertain mutation",
    ] {
        let text = prose.to_owned();
        let refused = std::panic::catch_unwind(|| common::assert_data(&text, "prose"));
        assert!(refused.is_err(), "{prose:?} passed as data");
    }
    common::assert_data(
        "name     agent\naddress  GABC\nsource\n  fn a() {}\n",
        "data",
    );
    common::assert_data("NAME   RAIL\nagent  stellar:testnet\n", "table");
    common::assert_data("* CWALLET\n  COTHER\n", "list");
}

/// Help examples that run offline, in order: each must exit 0 and print
/// only data (a JSON document, Rust source, or data lines).
#[test]
fn offline_help_examples_run_and_print_only_data() {
    let sandbox = common::Sandbox::new("examples");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::copy(
        root.join("../docs/examples/research.rs"),
        sandbox.work().join("policy.rs"),
    )
    .unwrap();
    std::fs::copy(
        root.join("../docs/examples/context.json"),
        sandbox.work().join("context.json"),
    )
    .unwrap();
    // Every example that appears in help.
    let mut examples = BTreeSet::new();
    for path in allowit_cli::command_paths() {
        let mut args: Vec<&str> = path.iter().map(String::as_str).collect();
        args.push("--help");
        let text = String::from_utf8(sandbox.run(&args).stdout).unwrap();
        examples.extend(
            text.lines()
                .map(str::trim)
                .filter(|l| l.starts_with("allowit "))
                .map(str::to_owned),
        );
    }
    let run_all = [
        "allowit rail ls",
        "allowit rail ls --json",
        "allowit rail use stellar:testnet",
        "allowit rail show",
        "allowit rail show solana:devnet --json",
        "allowit keys generate agent",
        "allowit keys ls",
        "allowit keys ls --json",
        "allowit keys show agent",
        "allowit keys show agent --json",
        "allowit setup --agent agent",
        // Preparation that no example covers.
        "prepare keys generate old-agent",
        "allowit whoami",
        "allowit whoami --json",
        "allowit config path",
        "allowit config show",
        "allowit config show --json",
        "allowit wallet ls",
        "allowit wallet ls --json",
        "allowit history",
        "allowit history --limit 5 --json",
        "allowit policy check policy.rs",
        "allowit policy test policy.rs --context context.json",
        "allowit policy functions",
        "allowit policy functions transaction_cap",
        "allowit keys rm old-agent --yes",
        "allowit --rail tempo:testnet keys generate executor",
        "allowit --rail solana:devnet policy new \"Spend up to 5 tokens per day\" -o policy.json",
        "allowit version",
        "allowit guide",
    ];
    for example in run_all {
        if let Some(args) = example.strip_prefix("prepare ") {
            sandbox.ok(&args.split(' ').collect::<Vec<_>>());
            continue;
        }
        assert!(
            examples.contains(example),
            "{example:?} is not a help example"
        );
        let mut words = vec![];
        let mut rest = example.strip_prefix("allowit ").unwrap();
        while !rest.is_empty() {
            rest = rest.trim_start();
            let (word, next) = match rest.strip_prefix('"') {
                Some(quoted) => quoted.split_once('"').unwrap(),
                None => rest.split_once(' ').unwrap_or((rest, "")),
            };
            words.push(word);
            rest = next;
        }
        let output = sandbox.run(&words);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{example}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        if words.contains(&"--json") {
            serde_json::from_str::<serde_json::Value>(&stdout)
                .unwrap_or_else(|_| panic!("{example}: stdout is not JSON"));
        } else if !matches!(words[..], ["version"] | ["guide"]) && !words.contains(&"new") {
            common::assert_data(&stdout, example);
        }
        common::assert_data(&stderr, example);
    }
}

#[test]
fn every_help_node_matches_its_snapshot() {
    let update = std::env::var_os("ALLOWIT_UPDATE_SNAPSHOTS").is_some();
    let dir = snapshot_dir();
    if update {
        std::fs::create_dir_all(&dir).unwrap();
    }
    let sandbox = common::Sandbox::new("help");
    let paths = allowit_cli::command_paths();
    assert!(paths.len() > 60, "{} nodes", paths.len());
    let mut expected_files = BTreeSet::new();
    let mut failures = vec![];
    for path in &paths {
        let mut args: Vec<&str> = path.iter().map(String::as_str).collect();
        args.push("--help");
        let output = sandbox.run(&args);
        let shown = format!("allowit {}", args.join(" "));
        assert_eq!(output.status.code(), Some(0), "{shown}");
        assert!(output.stderr.is_empty(), "{shown}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains('\x1b'), "{shown} printed color");
        // Examples come first: before the commands and options. A node that
        // no rail serves has none (the unit tests check which nodes).
        if let Some(examples) = text.find("Examples:\n  ") {
            for heading in ["\nCommands:\n", "\nArguments:\n", "\nOptions:\n"] {
                if let Some(at) = text.find(heading) {
                    assert!(examples < at, "{shown}: examples after {heading:?}");
                }
            }
        }
        // Only command groups keep a snapshot: one per group catches a
        // renamed, added or removed command. Every node gets the checks above.
        if !text.contains("\nCommands:\n") {
            continue;
        }
        let name = file_name(path);
        let file = dir.join(&name);
        expected_files.insert(name.clone());
        if update {
            std::fs::write(&file, &text).unwrap();
        } else {
            match std::fs::read_to_string(&file) {
                Ok(saved) if saved == text => {}
                Ok(_) => failures.push(format!("{shown}: differs from {name}")),
                Err(_) => failures.push(format!("{shown}: no snapshot {name}")),
            }
        }
    }
    let present: BTreeSet<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert!(
        !sandbox.config().exists(),
        "help created the config directory"
    );
    let stale: Vec<_> = present.difference(&expected_files).collect();
    assert!(stale.is_empty(), "stale snapshots: {stale:?}");
    assert!(
        failures.is_empty(),
        "{}\nRun ALLOWIT_UPDATE_SNAPSHOTS=1 cargo test -p allowit-cli --test help after an intended change.",
        failures.join("\n")
    );
}

#[test]
fn help_subcommand_matches_the_help_flag() {
    for path in [
        vec![],
        vec!["keys"],
        vec!["policy", "params", "set"],
        vec!["exec"],
    ] {
        let mut flag = path.clone();
        flag.push("--help");
        let mut sub = vec!["help"];
        sub.extend(path.iter().copied());
        let sandbox = common::Sandbox::new("help-sub");
        let a = sandbox.run(&flag);
        let b = sandbox.run(&sub);
        assert_eq!(a.status.code(), Some(0));
        assert_eq!(b.status.code(), Some(0), "{sub:?}");
        assert_eq!(a.stdout, b.stdout, "{sub:?}");
    }
}

#[test]
fn a_bare_group_prints_its_help_and_changes_nothing() {
    let sandbox = common::Sandbox::new("bare");
    for group in ["keys", "rail", "config", "wallet", "policy", "tx", "skills"] {
        let output = sandbox.run(&[group]);
        assert_eq!(output.status.code(), Some(0), "{group}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains(&format!("Usage: allowit {group}")), "{group}");
        assert!(text.contains("Commands:"), "{group}");
    }
    assert!(
        !sandbox.config().exists(),
        "a bare group created the config directory"
    );
}

#[test]
fn unknown_commands_exit_2_and_list_the_commands_at_that_level() {
    let sandbox = common::Sandbox::new("unknown");
    let output = sandbox.run(&["nope"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        error.contains("Commands: setup, whoami, keys, rail"),
        "{error}"
    );
    assert!(
        !error.contains("json"),
        "a hidden command was listed: {error}"
    );
    // A global flag's value is not read as a command name.
    let output = sandbox.run(&["--rail", "stellar:testnet", "policy", "mystery"]);
    assert_eq!(output.status.code(), Some(2));
    let error = String::from_utf8(output.stderr).unwrap();
    for verb in ["new", "check", "deploy", "params", "functions"] {
        assert!(error.contains(verb), "{verb}: {error}");
    }
    let output = sandbox.run(&["--json", "keys", "nope"]);
    assert_eq!(output.status.code(), Some(2));
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "usage");
    assert_eq!(error["error"]["exit"], 2);
    assert!(!sandbox.config().exists());
}

#[test]
fn version_flag_and_command_agree() {
    let sandbox = common::Sandbox::new("version");
    let flag = sandbox.run(&["--version"]);
    let command = sandbox.run(&["version"]);
    assert_eq!(flag.status.code(), Some(0));
    assert_eq!(command.status.code(), Some(0));
    let flag = String::from_utf8(flag.stdout).unwrap();
    let command = String::from_utf8(command.stdout).unwrap();
    assert!(flag.starts_with("allowit "), "{flag}");
    assert!(flag.contains(command.trim()), "{flag} / {command}");
}
