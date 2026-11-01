//! Hidden `allowit json [FILE|-]`: the editor wire protocol. Runs one SDK
//! request, for example `edit_preference` or `compile_stellar_wallet`.
use super::compiler;
use crate::{ctx::Ctx, error::Result};
use std::path::PathBuf;

pub(crate) const EXAMPLES: &str = "\
Examples:
  echo '{\"operation\":\"registry\"}' | allowit json";

/// Run one SDK wire-protocol request
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// The request JSON; - or nothing reads stdin
    pub file: Option<PathBuf>,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let path = args.file.unwrap_or_else(|| PathBuf::from("-"));
    let input = compiler::read(&path)?;
    let request = serde_json::from_str(&input).map_err(|e| compiler::failure(e.to_string()))?;
    compiler::process(request, cx, true)
}
