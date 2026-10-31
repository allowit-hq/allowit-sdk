//! `allowit policy new "PROMPT" [-o FILE]`.
use crate::{
    ctx::Ctx,
    error::{Error, Result},
};
use std::path::PathBuf;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit --rail solana:devnet policy new \"Spend up to 5 USDC per day\" -o policy.json
  allowit policy new \"Spend up to 5 test tokens per day\"";

/// Generate a policy from a prompt
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// What the policy allows, in one sentence
    pub prompt: String,
    /// Save the policy to FILE
    #[arg(short = 'o', long = "out", value_name = "FILE")]
    pub out: Option<PathBuf>,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    if args.prompt.trim().is_empty() || args.prompt.contains('\0') {
        return Err(Error::usage("PROMPT must be non-empty text"));
    }
    let vendor = cx.vendor()?;
    vendor.policy_new(cx, &args.prompt, args.out.as_deref())
}
