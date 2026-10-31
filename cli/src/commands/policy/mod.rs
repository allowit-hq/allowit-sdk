//! `allowit policy`: author, check, build, test and deploy policies.
mod build;
mod check;
mod deploy;
mod functions;
mod legacy;
mod new;
mod params;
mod show;
mod test;

use crate::{ctx::Ctx, error::Result};
use clap::Subcommand;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit policy check policy.rs
  allowit policy test policy.rs --context context.json
  allowit policy show --source
  allowit --rail solana:devnet policy params set transaction_cap=USDC:10 --sign-with-key owner";

/// Write, check, build, test, show and deploy policies
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    #[command(before_help = new::EXAMPLES)]
    New(new::Args),
    #[command(before_help = show::EXAMPLES)]
    Show(show::Args),
    #[command(before_help = check::EXAMPLES)]
    Check(check::Args),
    #[command(before_help = build::EXAMPLES)]
    Build(build::Args),
    #[command(before_help = test::EXAMPLES)]
    Test(test::Args),
    Deploy(deploy::Args),
    #[command(before_help = params::EXAMPLES)]
    Params(params::Args),
    #[command(before_help = functions::EXAMPLES)]
    Functions(functions::Args),
    #[command(flatten)]
    Legacy(legacy::Command),
}

pub(crate) fn run(command: Command, cx: &mut Ctx) -> Result<i32> {
    match command {
        Command::New(a) => new::run(a, cx),
        Command::Show(a) => show::run(a, cx),
        Command::Check(a) => check::run(a, cx),
        Command::Build(a) => build::run(a, cx),
        Command::Test(a) => test::run(a, cx),
        Command::Deploy(a) => deploy::run(a, cx),
        Command::Params(a) => params::run(a, cx),
        Command::Functions(a) => functions::run(a, cx),
        Command::Legacy(c) => legacy::run(c, cx),
    }
}

/// The verbs that read no rail, wallet or credential.
/// True for the hidden native Solana verbs of earlier releases.
pub(crate) fn is_legacy(command: &Command) -> bool {
    matches!(command, Command::Legacy(_))
}

pub(crate) fn is_local(command: &Command) -> bool {
    matches!(
        command,
        Command::Check(_) | Command::Test(_) | Command::Functions(_)
    )
}

pub(crate) use build::write_source;
pub(crate) use check::run_operation;
#[cfg(test)]
pub(crate) use params::parse_set as params_set_parse;
pub(crate) use test::{Mode as TestMode, evaluate};
