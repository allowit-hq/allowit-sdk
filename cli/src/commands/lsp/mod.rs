//! `allowit lsp`: the policy language server on stdin and stdout.
use crate::{ctx::Ctx, error::Result};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit lsp";

/// Run the policy language server: diagnostics, hover, completion and
/// allowit/workflow
#[derive(clap::Args, Debug)]
pub(crate) struct Args {}

pub(crate) fn run(_: Args, _: &mut Ctx) -> Result<i32> {
    allowit_sdk::lsp::run_stdio().map_err(|e| crate::error::Error {
        code: 1,
        message: e.to_string(),
        status: None,
        config: false,
    })?;
    Ok(0)
}
