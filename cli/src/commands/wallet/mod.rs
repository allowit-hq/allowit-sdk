//! `allowit wallet`: policy wallets. A wallet is identified only by its
//! address; `wallet use` caches it as the rail's default.
mod approve;
mod close;
mod configure;
mod create;
mod fund;
mod ls;
mod pause;
mod resume;
mod show;
mod update;
mod r#use;
mod withdraw;

use crate::{ctx::Ctx, error::Result};
use clap::Subcommand;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit --rail solana:devnet wallet create --asset USDC --policy policy.json --owner owner --agent agent --fund USDC:10
  allowit wallet use CWALLET…
  allowit wallet show
  allowit --rail solana:devnet wallet fund USDC:20 --sign-with-key owner";

/// Create, select, fund and manage policy wallets
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    #[command(before_help = create::EXAMPLES)]
    Create(create::Args),
    #[command(visible_alias = "list", before_help = ls::EXAMPLES)]
    Ls(ls::Args),
    #[command(before_help = show::EXAMPLES)]
    Show(show::Args),
    #[command(before_help = r#use::EXAMPLES)]
    Use(r#use::Args),
    #[command(before_help = fund::EXAMPLES)]
    Fund(fund::Args),
    #[command(before_help = withdraw::EXAMPLES)]
    Withdraw(withdraw::Args),
    Update(update::Args),
    #[command(before_help = pause::EXAMPLES)]
    Pause(pause::Args),
    Resume(resume::Args),
    #[command(before_help = close::EXAMPLES)]
    Close(close::Args),
    #[command(hide = true, before_help = configure::EXAMPLES)]
    Configure(configure::Args),
    #[command(hide = true, before_help = approve::EXAMPLES)]
    Approve(approve::Args),
}

pub(crate) fn run(command: Command, cx: &mut Ctx) -> Result<i32> {
    match command {
        Command::Create(a) => create::run(a, cx),
        Command::Ls(a) => ls::run(a, cx),
        Command::Show(a) => show::run(a, cx),
        Command::Use(a) => r#use::run(a, cx),
        Command::Fund(a) => fund::run(a, cx),
        Command::Withdraw(a) => withdraw::run(a, cx),
        Command::Update(a) => update::run(a, cx),
        Command::Pause(a) => pause::run(a, cx),
        Command::Resume(a) => resume::run(a, cx),
        Command::Close(a) => close::run(a, cx),
        Command::Configure(a) => configure::run(a, cx),
        Command::Approve(a) => approve::run(a, cx),
    }
}

/// Checks an owner operation's `--request-id`.
fn request_id(id: Option<String>) -> Result<Option<String>> {
    match id {
        Some(id) if !crate::request::request_id_valid(&id) => Err(crate::error::Error::usage(
            "--request-id must be 8 to 100 characters from A-Z a-z 0-9 . _ : -",
        )),
        id => Ok(id),
    }
}
