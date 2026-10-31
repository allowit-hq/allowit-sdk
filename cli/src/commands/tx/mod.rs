//! `allowit tx`: transactions signed outside the CLI.
mod send;

use crate::{ctx::Ctx, error::Result};
use clap::Subcommand;

/// Submit transactions that were signed outside the CLI
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    Send(send::Args),
}

pub(crate) fn run(command: Command, cx: &mut Ctx) -> Result<i32> {
    match command {
        Command::Send(a) => send::run(a, cx),
    }
}
