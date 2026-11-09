//! Runs the repository vendor-boundary eval, so `cargo test` fails when a vendor name leaves the
//! registration files. The scanner needs Node 22 or later on `PATH`.
use std::{path::Path, process::Command};

fn scan(args: &[&str]) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("node")
        .arg("scripts/vendor-boundary.mjs")
        .args(args)
        .current_dir(&root)
        .output()
        .expect("run node scripts/vendor-boundary.mjs (Node 22 or later)");
    assert!(
        output.status.success(),
        "vendor-boundary {args:?} failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn scanner_self_test_passes() {
    scan(&["--self-test"]);
}

#[test]
fn only_registration_files_name_a_vendor() {
    scan(&[]);
}
