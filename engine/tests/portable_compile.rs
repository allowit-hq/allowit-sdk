#![cfg(unix)]
//! The public authoring path must execute the pinned real compiler and bound its subprocess tree.
use allowit_sdk_engine::authoring::{HostTools, check, compile};
use std::{os::unix::fs::PermissionsExt, time::Duration};
const VALID: &str = include_str!("../../cli/templates/policy/src/policy.rs");

#[test]
fn actual_embedded_core_compile_accepts_portable_rust_and_rejects_type_errors() {
    if std::env::var_os("ALLOWIT_TEST_COMPILER_CHILD").is_some() {
        compile(
            VALID,
            &HostTools {
                python: std::env::var_os("ALLOWIT_TEST_PYTHON").unwrap().into(),
                rustc: std::env::var_os("ALLOWIT_TEST_RUSTC").unwrap().into(),
                ..Default::default()
            },
        )
        .unwrap();
        return;
    }
    let probe = tempfile::tempdir().unwrap();
    let names = [
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "ALLOWIT_PROVIDER_TOKEN",
        "ALLOWIT_OWNER_SECRET",
        "PYTHONPATH",
        "RUSTFLAGS",
    ];
    for (kind, tool) in [("python", "python3"), ("rustc", "rustc")] {
        let marker =
            serde_json::to_string(probe.path().join(format!("{kind}.json")).to_str().unwrap())
                .unwrap();
        let keys = serde_json::to_string(&names).unwrap();
        let script = format!(
            "#!/usr/bin/env python3\nimport os,sys,json\nfrom pathlib import Path\nPath({marker}).write_text(json.dumps({{k:k in os.environ for k in {keys}}}))\nos.execvp('{tool}',['{tool}']+sys.argv[1:])\n"
        );
        let path = probe.path().join(kind);
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "--exact",
            "actual_embedded_core_compile_accepts_portable_rust_and_rejects_type_errors",
        ])
        .env("ALLOWIT_TEST_COMPILER_CHILD", "1")
        .env("ALLOWIT_TEST_PYTHON", probe.path().join("python"))
        .env("ALLOWIT_TEST_RUSTC", probe.path().join("rustc"));
    for name in names {
        child.env(name, "synthetic-compiler-canary");
    }
    let output = child.output().unwrap();
    assert!(output.status.success(), "compiler probe child failed");
    for kind in ["python", "rustc"] {
        let values: std::collections::BTreeMap<String, bool> = serde_json::from_slice(
            &std::fs::read(probe.path().join(format!("{kind}.json"))).unwrap(),
        )
        .unwrap();
        assert!(
            values.values().all(|v| !*v),
            "{kind} inherited a synthetic secret/tool override"
        );
    }
    compile(VALID, &HostTools::default()).unwrap();
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
        python: "allowit-python-tool-does-not-exist".into(),
        ..HostTools::default()
    };
    assert_eq!(compile(VALID, &missing).unwrap_err().code, 3);
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
    let script = directory.path().join("slow-rustc");
    let marker_literal = serde_json::to_string(marker.to_str().unwrap()).unwrap();
    std::fs::write(&script,format!("#!/usr/bin/env python3\nimport time\nfrom pathlib import Path\ntime.sleep(1)\nPath({marker_literal}).write_text('escaped')\n")).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let tools = HostTools {
        rustc: script,
        timeout: Duration::from_millis(150),
        ..HostTools::default()
    };
    let error = compile(VALID, &tools).unwrap_err();
    assert!(error.message.contains("timed out"), "{}", error.message);
    std::thread::sleep(Duration::from_millis(1100));
    assert!(
        !marker.exists(),
        "compiler descendant outlived the bounded operation"
    );
    let noop = HostTools {
        python: "/usr/bin/true".into(),
        ..HostTools::default()
    };
    let error = compile(VALID, &noop).unwrap_err();
    assert!(
        error.message.contains("did not confirm"),
        "{}",
        error.message
    );
}

#[test]
fn exited_tool_cannot_leave_unbounded_descendant_output_readers() {
    let directory = tempfile::tempdir().unwrap();
    let script = directory.path().join("exiting-python");
    std::fs::write(&script, "#!/bin/sh\n/bin/sleep 2 &\nexit 0\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let tools = HostTools {
        python: script,
        timeout: Duration::from_millis(150),
        ..HostTools::default()
    };
    let start = std::time::Instant::now();
    let error = compile(VALID, &tools).unwrap_err();
    assert!(error.message.contains("timed out"), "{}", error.message);
    assert!(start.elapsed() < Duration::from_secs(1));
}
