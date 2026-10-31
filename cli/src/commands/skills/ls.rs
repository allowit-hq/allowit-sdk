//! `allowit skills ls`.
use crate::{ctx::Ctx, error::Result};

/// List the catalog services
#[derive(clap::Args, Debug)]
pub(crate) struct Args {}

pub(crate) fn run(_: Args, cx: &mut Ctx) -> Result<i32> {
    let value = super::read(None)?;
    cx.out.json_value(&value);
    Ok(0)
}
