#![cfg(unix)]
//! The public authoring path must execute the real compiler directly and bound its subprocess tree.
use allowit_sdk::authoring::{HostTools, check, compile};
use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};
const VALID: &str = include_str!("fixtures/portable-policy.rs");

/// An executable shell script at `path`.
fn script(path: &Path, body: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn actual_embedded_core_compile_accepts_portable_rust_and_rejects_type_errors() {
    if let Some(rustc) = std::env::var_os("ALLOWIT_TEST_RUSTC") {
        let tools = HostTools {
            rustc: rustc.into(),
            ..Default::default()
        };
        compile(VALID, &tools).unwrap();
        return;
    }
    // The compiler inherits no provider secret and no tool override.
    let probe = tempfile::tempdir().unwrap();
    let names = [
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "ALLOWIT_PROVIDER_TOKEN",
        "ALLOWIT_OWNER_SECRET",
        "RUSTFLAGS",
        "RUSTC_WRAPPER",
    ];
    let (seen, leaked) = (probe.path().join("seen"), probe.path().join("leaked"));
    let checks: String = names
        .iter()
        .map(|name| format!("[ -z \"${{{name}+set}}\" ] || : > '{}'\n", leaked.display()))
        .collect();
    script(
        &probe.path().join("rustc"),
        &format!("{checks}: > '{}'\nexec rustc \"$@\"", seen.display()),
    );
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "--exact",
            "actual_embedded_core_compile_accepts_portable_rust_and_rejects_type_errors",
        ])
        .env("ALLOWIT_TEST_RUSTC", probe.path().join("rustc"));
    for name in names {
        child.env(name, "synthetic-compiler-canary");
    }
    assert!(
        child.output().unwrap().status.success(),
        "compiler probe child failed"
    );
    assert!(seen.exists(), "the probe compiler did not run");
    assert!(
        !leaked.exists(),
        "rustc inherited a synthetic secret or tool override"
    );
    // The source gate admits this source; only the compiler finds its type error.
    let invalid = "use allowit::prelude::*; pub fn execute(ctx: &AllowitContext) -> allowit::Result { let value: i128 = true; let _ = value; let _ = ctx; Ok(()) }";
    check(invalid).unwrap();
    let error = compile(invalid, &HostTools::default()).unwrap_err();
    assert_eq!(error.code, 2);
    assert!(
        error.message.contains("mismatched types"),
        "{}",
        error.message
    );
    let missing = HostTools {
        rustc: "allowit-rustc-tool-does-not-exist".into(),
        ..HostTools::default()
    };
    assert_eq!(compile(VALID, &missing).unwrap_err().code, 3);
}

#[test]
fn wall_limit_kills_compiler_descendants_and_a_noop_tool_cannot_confirm_compilation() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("escaped");
    let slow = directory.path().join("slow-rustc");
    script(
        &slow,
        &format!("(/bin/sleep 1; : > '{}') &\nwait", marker.display()),
    );
    let tools = HostTools {
        rustc: slow,
        timeout: Duration::from_millis(150),
    };
    let error = compile(VALID, &tools).unwrap_err();
    assert!(error.message.contains("timed out"), "{}", error.message);
    std::thread::sleep(Duration::from_millis(1100));
    assert!(
        !marker.exists(),
        "compiler descendant outlived the bounded operation"
    );
    let noop = HostTools {
        rustc: "/usr/bin/true".into(),
        ..HostTools::default()
    };
    let error = compile(VALID, &noop).unwrap_err();
    assert!(
        error.message.contains("did not confirm"),
        "{}",
        error.message
    );
    // A tool that exits while a descendant holds its output stays within the same limit.
    let exiting = directory.path().join("exiting-rustc");
    script(&exiting, "/bin/sleep 2 &\nexit 0");
    let tools = HostTools {
        rustc: exiting,
        timeout: Duration::from_millis(150),
    };
    let start = std::time::Instant::now();
    let error = compile(VALID, &tools).unwrap_err();
    assert!(error.message.contains("timed out"), "{}", error.message);
    assert!(start.elapsed() < Duration::from_secs(1));
}
