//! Hidden `allowit wallet approve`: the source approval link of earlier
//! releases for an installed classic Stellar policy, testnet only.
use crate::{ctx::Ctx, error::Result};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit --rail stellar:testnet wallet approve --policy-file policy.json --owner GOWNER… --request-id approve-0001";

/// Print the owner approval link for an installed classic Stellar policy
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// --policy-file FILE --owner G… --request-id ID
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "FLAGS"
    )]
    pub args: Vec<String>,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    cx.rail()?;
    crate::vendors::stellar::approval::run(&args.args, &mut cx.out.stdout)
}
