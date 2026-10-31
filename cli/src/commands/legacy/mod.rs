//! Hidden commands of earlier releases, kept as aliases: `compile`, `check`
//! and `workflow` (now `policy check`), `evaluate` (`policy test`),
//! `stellar-native` (`policy build` on Stellar), `registry`
//! (`policy functions`) and the `tempo` group.
use super::{compiler, policy};
use crate::{ctx::Ctx, error::Result};
use clap::Subcommand;
use serde_json::json;
use std::path::PathBuf;

const CHECK: &str = "\
Examples:
  allowit policy check policy.rs";
const TEST: &str = "\
Examples:
  allowit policy test policy.rs --context context.json";
const BUILD: &str = "\
Examples:
  allowit --rail stellar:testnet policy build policy.rs";
const FUNCTIONS: &str = "\
Examples:
  allowit policy functions";
const TEMPO: &str = "\
Examples:
  allowit --rail tempo:testnet keys import executor executor.json
  allowit --rail tempo:testnet exec --op transfer --to 0xRECIPIENT… --amount AlphaUSD:1
  allowit --rail tempo:testnet call https://service.example/resource --max-amount AlphaUSD:0.1";

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    #[command(hide = true, before_help = CHECK)]
    Compile { file: PathBuf },
    #[command(hide = true, before_help = CHECK)]
    Check { file: PathBuf },
    #[command(hide = true, before_help = CHECK)]
    Workflow { file: PathBuf },
    #[command(hide = true, before_help = TEST)]
    Evaluate {
        file: PathBuf,
        context: PathBuf,
        #[arg(value_enum, default_value = "oracle")]
        profile: policy::TestMode,
    },
    #[command(hide = true, name = "stellar-native", before_help = BUILD)]
    StellarNative { file: PathBuf },
    #[command(hide = true, before_help = FUNCTIONS)]
    Registry,
    #[command(hide = true, before_help = TEMPO)]
    Tempo {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

pub(crate) fn run(command: Command, cx: &mut Ctx) -> Result<i32> {
    match command {
        Command::Compile { file } => policy::run_operation("compile", &file, cx),
        Command::Check { file } => policy::run_operation("check", &file, cx),
        Command::Workflow { file } => policy::run_operation("workflow", &file, cx),
        Command::Evaluate {
            file,
            context,
            profile,
        } => policy::evaluate(&file, &context, profile, cx),
        Command::StellarNative { file } => {
            let source = compiler::read(&file)?;
            let built = allowit_sdk::compile_stellar_policy(&source)
                .map_err(|e| compiler::failure(e.to_string()))?;
            policy::stellar_source(&built.rust_source, None, cx)
        }
        Command::Registry => compiler::process(json!({"operation":"registry"}), cx),
        Command::Tempo { args } => {
            let out = &mut cx.out;
            crate::vendors::tempo::legacy(&args, &mut out.stdout, &mut out.stderr)
        }
    }
}

/// The hidden commands that read no rail, wallet or credential.
pub(crate) fn is_local(command: &Command) -> bool {
    !matches!(command, Command::Tempo { .. })
}
