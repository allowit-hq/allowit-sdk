//! Portable source validation, source output and installed-policy inspection.
mod build;
mod check;
mod show;
use crate::{ctx::Ctx, error::Result};
use clap::Subcommand;
pub(crate) const EXAMPLES: &str = "Examples:\n  allowit policy check policy.rs\n  allowit policy build policy.rs -o wallet.rs\n  allowit policy show --source";
/// Validate portable Rust source, emit its source or inspect an installed policy
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    #[command(before_help=show::EXAMPLES)]
    Show(show::Args),
    #[command(before_help=check::EXAMPLES)]
    Check(check::Args),
    #[command(before_help=build::EXAMPLES)]
    Build(build::Args),
}
pub(crate) fn run(command: Command, cx: &mut Ctx) -> Result<i32> {
    match command {
        Command::Show(a) => show::run(a, cx),
        Command::Check(a) => check::run(a, cx),
        Command::Build(a) => build::run(a, cx),
    }
}
pub(crate) fn is_local(command: &Command) -> bool {
    matches!(command, Command::Check(_) | Command::Build(_))
}
fn read(path: &std::path::Path) -> Result<String> {
    String::from_utf8(crate::ctx::read_input(path)?)
        .map_err(|_| crate::error::Error::usage("The policy source must be UTF-8"))
}
