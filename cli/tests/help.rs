//! Help snapshots: `--help` on every command node of the built binary,
//! hidden aliases included. Set `ALLOWIT_UPDATE_SNAPSHOTS=1` to rewrite the
//! files in `tests/snapshots/help/` after an intended help change.
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
        // Examples come first: before the commands and options.
        let examples = text
            .find("Examples:\n  ")
            .unwrap_or_else(|| panic!("{shown} has no examples"));
        for heading in ["\nCommands:\n", "\nArguments:\n", "\nOptions:\n"] {
            if let Some(at) = text.find(heading) {
                assert!(examples < at, "{shown}: examples after {heading:?}");
            }
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
    let output = sandbox.run(&["policy", "mystery"]);
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
