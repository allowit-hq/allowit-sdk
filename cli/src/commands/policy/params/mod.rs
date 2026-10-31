//! `allowit policy params`: the policy parameters, keyed by the canonical
//! keys of the contracts params map.
mod set;
mod show;

use crate::{ctx::Ctx, error::Result};
use clap::Subcommand;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit policy params
  allowit policy params set day_spent_cap=USDC:50 transaction_cap=USDC:10 --sign-with-key owner";

/// Show or change the policy parameters
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    #[command(before_help = show::EXAMPLES)]
    Show(show::Args),
    #[command(before_help = set::EXAMPLES)]
    Set(set::Args),
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    match args.command {
        None => show::run(show::Args {}, cx),
        Some(Command::Show(a)) => show::run(a, cx),
        Some(Command::Set(a)) => set::run(a, cx),
    }
}
