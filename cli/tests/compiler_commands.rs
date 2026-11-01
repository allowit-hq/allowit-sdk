//! The policy compiler and other local commands are local: wallet and
//! network environment and a broken configuration must not change their
//! output or exit code.
use std::process::{Command, Output};

const WALLET_ENV: &[&str] = &[
    "ALLOWIT_RAIL",
    "ALLOWIT_WALLET",
    "ALLOWIT_CONFIG_DIR",
    "ALLOWIT_NETWORK",
    "ALLOWIT_TEMPO_BUNDLE",
    "ALLOWIT_STELLAR_BUNDLE",
    "ALLOWIT_TOKEN",
    "ALLOWIT_ACCOUNT_TOKEN",
    "ALLOWIT_PAYSH_TOKEN",
];

fn allowit(args: &[&str], wallet_env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_allowit"));
    command.args(args);
    for name in WALLET_ENV {
        command.env_remove(name);
    }
    command.envs(wallet_env.iter().copied());
    command.output().unwrap()
}

#[test]
fn local_commands_ignore_wallet_environment_and_broken_config() {
    let config = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("allowit-cli-broken-config-{}", std::process::id()));
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("config.toml"), "rail = [broken").unwrap();
    let config_dir = config.to_str().unwrap();
    let hostile = [
        ("ALLOWIT_NETWORK", "bogus"),
        ("ALLOWIT_RAIL", "bogus"),
        ("ALLOWIT_WALLET", "bogus"),
        ("ALLOWIT_CONFIG_DIR", config_dir),
        (
            "ALLOWIT_TEMPO_BUNDLE",
            "/nonexistent/allowit-tempo-bundle.json",
        ),
        (
            "ALLOWIT_STELLAR_BUNDLE",
            "/nonexistent/allowit-stellar-bundle.json",
        ),
        // A token value that occurs in the compiler output.
        ("ALLOWIT_TOKEN", "USDC"),
    ];
    let commands: &[&[&str]] = &[
        &["help"],
        &["--help"],
        &["version"],
        &["guide"],
        &["completion", "--shell", "bash"],
        &["compile", "../docs/examples/research.rs"],
        &["check", "../docs/examples/research.rs"],
        &["workflow", "../docs/examples/research.rs"],
        &[
            "evaluate",
            "../docs/examples/research.rs",
            "../docs/examples/context.json",
            "contract",
        ],
        &["stellar-native", "../docs/examples/approval.rs"],
        &["registry"],
        &["policy", "check", "../docs/examples/research.rs"],
        &["policy", "functions"],
        &[
            "--network",
            "eip155:1",
            "compile",
            "../docs/examples/research.rs",
        ],
    ];
    for args in commands {
        let clean = allowit(args, &[]);
        let with_env = allowit(args, &hostile);
        // The native Stellar profile rejects this example's expression.
        let expected = if args[0] == "stellar-native" { 2 } else { 0 };
        assert_eq!(clean.status.code(), Some(expected), "{args:?}");
        assert_eq!(clean.status.code(), with_env.status.code(), "{args:?}");
        assert_eq!(clean.stdout, with_env.stdout, "{args:?}");
        assert_eq!(clean.stderr, with_env.stderr, "{args:?}");
    }
    let compiled = allowit(&["compile", "../docs/examples/research.rs"], &hostile);
    assert!(String::from_utf8(compiled.stdout).unwrap().contains("USDC"));
    // A command that needs the configuration reports it.
    let output = allowit(&["keys", "ls"], &hostile);
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("config.toml is invalid"));
    let output = allowit(&["rail", "show"], &hostile);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown rail"));
    std::fs::remove_dir_all(config).unwrap();
}

#[test]
fn global_flags_in_any_position_leave_compiler_output_unchanged() {
    // With --json the policy verbs print the bytes of their hidden aliases of
    // earlier releases; rail and wallet flags do not change a local command.
    let research = "../docs/examples/research.rs";
    let context = "../docs/examples/context.json";
    let cases: &[(&[&str], &[&str])] = &[
        (
            &["compile", research],
            &["--network", "eip155:1", "compile", research],
        ),
        (
            &["check", research],
            &["--network=stellar:testnet", "check", research],
        ),
        (
            &["check", research],
            &["policy", "check", research, "--json"],
        ),
        (
            &["check", research],
            &["policy", "check", research, "--rail", "nope", "--json"],
        ),
        (&["registry"], &["--json", "registry"]),
        (&["registry"], &["policy", "functions", "--output", "json"]),
        (
            &["evaluate", research, context, "contract"],
            &[
                "policy",
                "test",
                research,
                "--context",
                context,
                "--mode",
                "contract",
                "--json",
            ],
        ),
        (
            &["evaluate", research, context],
            &[
                "--wallet",
                "nope",
                "policy",
                "test",
                research,
                "--context",
                context,
                "--json",
            ],
        ),
    ];
    for (old, new) in cases {
        let before = allowit(old, &[]);
        let after = allowit(new, &[]);
        assert_eq!(before.status.code(), Some(0), "{old:?}");
        assert_eq!(before.status.code(), after.status.code(), "{new:?}");
        assert_eq!(before.stdout, after.stdout, "{new:?}");
        assert_eq!(before.stderr, after.stderr, "{new:?}");
    }
    // stellar-native is policy build with a Stellar rail.
    let approval = "../docs/examples/approval.rs";
    let old = allowit(&["stellar-native", approval], &[]);
    let new = allowit(
        &["--rail", "stellar:testnet", "policy", "build", approval],
        &[],
    );
    assert_eq!(old.status.code(), new.status.code());
    assert_eq!(old.stdout, new.stdout);
    assert_eq!(old.stderr, new.stderr);
    // With --json the generated source is one JSON object.
    let policy = std::env::temp_dir().join(format!("allowit-native-{}.rs", std::process::id()));
    std::fs::write(
        &policy,
        r#"use allowit::v1::prelude::*; async fn _execute(ctx: &Context) -> PolicyResult { allowit::cap_per_transaction(ctx, "2", "USDC")?; Ok(()) }"#,
    )
    .unwrap();
    let policy_path = policy.to_str().unwrap();
    let text = allowit(
        &["--rail", "stellar:testnet", "policy", "build", policy_path],
        &[],
    );
    let json = allowit(
        &[
            "--rail",
            "stellar:testnet",
            "policy",
            "build",
            policy_path,
            "--json",
        ],
        &[],
    );
    std::fs::remove_file(&policy).unwrap();
    assert_eq!(text.status.code(), Some(0), "{:?}", text.stderr);
    assert_eq!(json.status.code(), Some(0), "{:?}", json.stderr);
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"source": String::from_utf8(text.stdout).unwrap()})
    );
    let other = allowit(
        &["--rail", "solana:devnet", "policy", "build", approval],
        &[],
    );
    assert_eq!(other.status.code(), Some(3));
    // Without --json the policy verbs print text lines, also when piped.
    for args in [
        &["policy", "check", research][..],
        &["policy", "functions"],
        &["policy", "test", research, "--context", context],
    ] {
        let text = allowit(args, &[]);
        assert_eq!(text.status.code(), Some(0), "{args:?}");
        let stdout = String::from_utf8(text.stdout).unwrap();
        assert!(!stdout.is_empty(), "{args:?}");
        assert!(
            serde_json::from_str::<serde_json::Value>(&stdout).is_err(),
            "{args:?}: {stdout}"
        );
        // A summary, not the flattened IR or source.
        assert!(!stdout.contains("policy.ir."), "{args:?}: {stdout}");
        assert!(stdout.lines().count() < 120, "{args:?}: {stdout}");
    }
    let functions = String::from_utf8(allowit(&["policy", "functions"], &[]).stdout).unwrap();
    assert!(functions.starts_with("NAME "), "{functions}");
    // A missing file is a usage error with nothing on stdout.
    let output = allowit(&["policy", "check", "--json"], &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}
