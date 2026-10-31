//! Hidden `allowit wallet configure`: the classic-account owner setup link of
//! earlier releases, Stellar testnet only.
use crate::{ctx::Ctx, error::Result};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit --rail stellar:testnet wallet configure --owner GOWNER… --agent GAGENT… --engine GENGINE… --request-id setup-0001";

/// Print the owner signing link for a classic Stellar account setup
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// --owner G… --agent G… --engine G… --request-id ID [--fee-per-operation N]
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "FLAGS"
    )]
    pub args: Vec<String>,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    cx.rail()?;
    crate::vendors::stellar::owner::run(&args.args, &mut cx.out.stdout)
}
