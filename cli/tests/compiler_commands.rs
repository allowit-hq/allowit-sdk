//! The policy compiler commands are local: wallet and network environment
//! must not change their output or exit code.
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
fn compiler_commands_ignore_wallet_environment() {
    let hostile = [
        ("ALLOWIT_NETWORK", "bogus"),
        ("ALLOWIT_RAIL", "bogus"),
        ("ALLOWIT_WALLET", "bogus"),
        ("ALLOWIT_CONFIG_DIR", "/nonexistent/allowit-config"),
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
        assert_eq!(clean.status.code(), with_env.status.code(), "{args:?}");
        assert_eq!(clean.stdout, with_env.stdout, "{args:?}");
        assert_eq!(clean.stderr, with_env.stderr, "{args:?}");
    }
    let compiled = allowit(&["compile", "../docs/examples/research.rs"], &hostile);
    assert!(compiled.status.success());
    assert!(String::from_utf8(compiled.stdout).unwrap().contains("USDC"));
}

#[test]
fn global_flags_in_any_position_leave_compiler_output_unchanged() {
    // The policy verbs and their hidden aliases of earlier releases print the
    // same bytes; rail and output flags do not change a local command.
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
        (&["check", research], &["policy", "check", research]),
        (
            &["check", research],
            &["policy", "check", research, "--rail", "nope"],
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
    let other = allowit(
        &["--rail", "solana:devnet", "policy", "build", approval],
        &[],
    );
    assert_eq!(other.status.code(), Some(3));
    // A missing file is a usage error with nothing on stdout.
    let output = allowit(&["policy", "check", "--json"], &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}
