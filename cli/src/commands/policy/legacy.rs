//! Hidden `allowit policy` verbs of earlier releases for the native Solana
//! vault: generate, import, fund, execute, status, revoke, withdraw, close,
//! tune and tune-action. They keep their environment settings
//! (`ALLOWIT_POLICY_DIR`, `ALLOWIT_OWNER_KEYPAIR`, `ALLOWIT_EXECUTOR_KEYPAIR`,
//! `ALLOWIT_REQUEST_ID`) and default to solana:testnet.
use crate::{
    ctx::Ctx,
    error::{Error, Result},
    routing::Family,
};
use clap::Subcommand;
use std::path::PathBuf;

const NEW: &str = "\
Examples:
  allowit --rail solana:testnet policy new \"Spend up to 5 test tokens per day\"";
const IMPORT: &str = "\
Examples:
  allowit --rail solana:testnet keys import executor executor.json";
const FUND: &str = "\
Examples:
  allowit --rail solana:testnet wallet fund USDC:10 --sign-with-key owner";
const EXECUTE: &str = "\
Examples:
  allowit --rail solana:testnet exec --op transfer --to RECIPIENT… --amount USDC:0.25";
const STATUS: &str = "\
Examples:
  allowit --rail solana:testnet wallet show";
const REVOKE: &str = "\
Examples:
  allowit --rail solana:testnet wallet pause --sign-with-key owner";
const WITHDRAW: &str = "\
Examples:
  allowit --rail solana:testnet wallet withdraw USDC:5 --sign-with-key owner";
const CLOSE: &str = "\
Examples:
  allowit --rail solana:testnet wallet close --sign-with-key owner --yes";
const TUNE: &str = "\
Examples:
  allowit --rail solana:testnet policy params set day_total_cap=USDC:20 --sign-with-key owner";
const TUNE_ACTION: &str = "\
Examples:
  allowit --rail solana:testnet policy params set transaction_cap=USDC:1 --sign-with-key owner";

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    #[command(hide = true, before_help = NEW)]
    Generate { prompt: String },
    #[command(hide = true, before_help = IMPORT)]
    Import { file: PathBuf },
    #[command(hide = true, before_help = FUND)]
    Fund { amount: String },
    #[command(hide = true, before_help = EXECUTE)]
    Execute { recipient: String, amount: String },
    #[command(hide = true, before_help = STATUS)]
    Status,
    #[command(hide = true, before_help = REVOKE)]
    Revoke,
    #[command(hide = true, before_help = WITHDRAW)]
    Withdraw { amount: String },
    #[command(hide = true, before_help = CLOSE)]
    Close,
    #[command(hide = true, before_help = TUNE)]
    Tune { value: String },
    #[command(hide = true, name = "tune-action", before_help = TUNE_ACTION)]
    TuneAction { value: String },
}

pub(crate) fn run(command: Command, cx: &mut Ctx) -> Result<i32> {
    if let Some(rail) = cx.rail_opt()?
        && rail.family != Family::Solana
    {
        return Err(Error::unsupported(format!(
            "this policy verb serves Solana vaults, not {}",
            rail.id
        )));
    }
    let (name, pos): (&str, Vec<String>) = match command {
        Command::Generate { prompt } => ("generate", vec![prompt]),
        Command::Import { file } => ("import", vec![file.to_string_lossy().into_owned()]),
        Command::Fund { amount } => ("fund", vec![amount]),
        Command::Execute { recipient, amount } => {
            if !crate::request::solana_address(&recipient) {
                return Err(Error::usage(format!(
                    "RECIPIENT {recipient} is not a Solana token account address"
                )));
            }
            ("execute", vec![recipient, amount])
        }
        Command::Status => ("status", vec![]),
        Command::Revoke => ("revoke", vec![]),
        Command::Withdraw { amount } => ("withdraw", vec![amount]),
        Command::Close => ("close", vec![]),
        Command::Tune { value } => ("tune", vec![value]),
        Command::TuneAction { value } => ("tune-action", vec![value]),
    };
    if pos.iter().any(|p| p.contains('\0') || p.trim().is_empty()) {
        return Err(Error::usage("arguments must be non-empty text"));
    }
    let json = cx.out.json;
    let out = &mut cx.out;
    crate::vendors::solana::policy::run(name, &pos, json, &mut out.stdout, &mut out.stderr)
}
