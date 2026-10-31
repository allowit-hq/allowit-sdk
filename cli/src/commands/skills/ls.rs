//! `allowit skills ls`.
use crate::{ctx::Ctx, error::Result};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit skills ls
  allowit skills ls --json";

/// List the catalog services
#[derive(clap::Args, Debug)]
pub(crate) struct Args {}

pub(crate) fn run(_: Args, cx: &mut Ctx) -> Result<i32> {
    let value = super::read(None)?;
    cx.out.json_value(&value);
    Ok(0)
}
