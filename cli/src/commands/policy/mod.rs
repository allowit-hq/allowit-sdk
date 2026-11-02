//! `allowit policy`: author, check, build, test and deploy policies.
mod build;
mod check;
mod functions;
mod show;
mod test;

use crate::{ctx::Ctx, error::Result};
use clap::Subcommand;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit policy check policy.rs
  allowit policy test policy.rs --context context.json
  allowit policy show --source";

/// Check, build, test and inspect policies
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    #[command(before_help = show::EXAMPLES)]
    Show(show::Args),
    #[command(before_help = check::EXAMPLES)]
    Check(check::Args),
    #[command(before_help = build::EXAMPLES)]
    Build(build::Args),
    #[command(before_help = test::EXAMPLES)]
    Test(test::Args),
    #[command(before_help = functions::EXAMPLES)]
    Functions(functions::Args),
}

pub(crate) fn run(command: Command, cx: &mut Ctx) -> Result<i32> {
    match command {
        Command::Show(a) => show::run(a, cx),
        Command::Check(a) => check::run(a, cx),
        Command::Build(a) => build::run(a, cx),
        Command::Test(a) => test::run(a, cx),
        Command::Functions(a) => functions::run(a, cx),
    }
}

/// The verbs that read no rail, wallet or credential.
pub(crate) fn is_local(command: &Command) -> bool {
    matches!(
        command,
        Command::Check(_) | Command::Test(_) | Command::Functions(_)
    )
}

pub(crate) use build::write_source;
pub(crate) use check::run_operation;
pub(crate) use test::{Mode as TestMode, evaluate};
