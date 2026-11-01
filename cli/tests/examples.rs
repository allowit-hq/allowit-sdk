//! docs/examples/ is a copy of the test inputs in allowit-core. Keep the two
//! byte-identical at the pinned core revision.

use serde_json::Value;
use std::{fs, path::Path, process::Command};

const CORE_SOURCE: &str = "git+https://github.com/allowit-hq/allowit-core.git";

#[test]
fn examples_match_the_pinned_allowit_core() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    // Resolve for the host only: offline, a clean runner has no crates for
    // other platforms.
    let version = Command::new(env!("CARGO")).arg("-vV").output().unwrap();
    let version = String::from_utf8(version.stdout).unwrap();
    let host = version
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .expect("cargo -vV prints the host triple");
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .args(["--filter-platform", host])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let cores: Vec<&str> = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| {
            p["source"]
                .as_str()
                .is_some_and(|s| s.starts_with(CORE_SOURCE))
        })
        .map(|p| p["manifest_path"].as_str().unwrap())
        .collect();
    assert_eq!(
        cores.len(),
        1,
        "expected one allowit-core package: {cores:?}"
    );
    let core = Path::new(cores[0]).parent().unwrap().join("docs/examples");
    let ours = root.join("docs/examples");
    let names = |dir: &Path| {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .map(|e| e.unwrap().file_name())
            .collect();
        names.sort();
        names
    };
    assert_eq!(names(&ours), names(&core), "docs/examples file list");
    for name in names(&ours) {
        assert!(
            fs::read(ours.join(&name)).unwrap() == fs::read(core.join(&name)).unwrap(),
            "docs/examples/{} differs from allowit-core",
            name.to_string_lossy()
        );
    }
}
