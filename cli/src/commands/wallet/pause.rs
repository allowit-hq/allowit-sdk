//! `allowit wallet pause`.
use crate::{commands::SigningArgs, ctx::Ctx, error::Result};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit wallet pause --sign-with-key owner
  allowit wallet pause";

/// Stop agent spending until the owner resumes it
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    #[command(flatten)]
    pub signing: SigningArgs,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let rail = cx.rail()?;
    let signing = args.signing.resolve(cx, rail.id)?;
    let vendor = cx.vendor()?;
    vendor.wallet_pause(cx, &signing)
}
