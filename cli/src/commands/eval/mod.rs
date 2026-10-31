//! `allowit eval`: the same request as `exec`, checked by the policy only.
use super::exec::{RequestArgs, submit};
use crate::{ctx::Ctx, error::Result, vendors::Mode};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit eval --op transfer --to GDEST… --amount XLM:5
  allowit eval --op transfer --to RECIPIENT… --amount USDC:2 --json";

/// Ask the policy whether a request would pass. Never signs or spends
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    #[command(flatten)]
    pub request: RequestArgs,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    submit(Mode::Eval, args.request, false, cx)
}
