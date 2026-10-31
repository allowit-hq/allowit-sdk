//! `allowit tx`: transactions signed outside the CLI.
mod send;

use crate::{ctx::Ctx, error::Result};
use clap::Subcommand;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit exec --op transfer --to GDEST… --amount XLM:5 --build-only > request.json
  allowit tx send signed.json";

/// Submit transactions that were signed outside the CLI
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    #[command(before_help = send::EXAMPLES)]
    Send(send::Args),
}

pub(crate) fn run(command: Command, cx: &mut Ctx) -> Result<i32> {
    match command {
        Command::Send(a) => send::run(a, cx),
    }
}
