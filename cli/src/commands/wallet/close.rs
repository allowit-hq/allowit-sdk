//! `allowit wallet close`.
use crate::{commands::SigningArgs, ctx::Ctx, error::Result};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit --rail solana:devnet wallet close --sign-with-key owner --yes
  allowit wallet close";

/// Close the wallet and return its funds to the owner
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    #[command(flatten)]
    pub signing: SigningArgs,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let rail = cx.rail()?;
    let signing = args.signing.resolve(cx, rail.id)?;
    let vendor = cx.vendor()?;
    if vendor.caps().verbs.contains(&"wallet close") {
        cx.confirm("Close the wallet")?;
    }
    vendor.wallet_close(cx, &signing)
}
