//! `allowit tx send FILE|-`.
use crate::{
    ctx::{Ctx, read_input},
    error::Result,
};
use std::path::PathBuf;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit tx send signed.json
  cat signed.json | allowit tx send -";

/// Submit a signed transaction from --build-only output
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// The signed transaction; - reads stdin
    pub file: PathBuf,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let vendor = cx.vendor()?;
    let bytes = read_input(&args.file)?;
    vendor.tx_send(cx, &bytes)
}
