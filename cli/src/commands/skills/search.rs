//! `allowit skills search QUERY`.
use crate::{ctx::Ctx, error::Result};

/// Search the catalog
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Search text
    pub query: String,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let value = super::read(Some(&args.query))?;
    cx.out.json_value(&value);
    Ok(0)
}
