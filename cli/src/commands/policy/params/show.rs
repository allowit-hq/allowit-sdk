//! `allowit policy params show`.
use crate::{ctx::Ctx, error::Result};

/// Show the current parameter values
#[derive(clap::Args, Debug)]
pub(crate) struct Args {}

pub(crate) fn run(_: Args, cx: &mut Ctx) -> Result<i32> {
    let vendor = cx.vendor()?;
    vendor.policy_params(cx)
}
