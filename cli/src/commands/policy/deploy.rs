//! `allowit policy deploy FILE [--x402 on|off]`.
use crate::{commands::SigningArgs, ctx::Ctx, error::Result};
use std::path::PathBuf;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit policy deploy policy-v2.rs
  allowit policy deploy policy-v2.rs --sign-with-key owner";

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum Switch {
    On,
    Off,
}

/// Replace the wallet's policy. The owner signs
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// The policy source
    pub file: PathBuf,
    /// Let the agent pay x402 resources from the wallet
    #[arg(long, value_enum)]
    pub x402: Option<Switch>,
    #[command(flatten)]
    pub signing: SigningArgs,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let rail = cx.rail()?;
    let signing = args.signing.resolve(cx, rail.id)?;
    let vendor = cx.vendor()?;
    vendor.policy_deploy(
        cx,
        &args.file,
        args.x402.map(|s| matches!(s, Switch::On)),
        &signing,
    )
}
