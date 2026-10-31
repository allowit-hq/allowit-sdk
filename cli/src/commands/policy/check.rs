//! `allowit policy check FILE`.
use super::super::compiler;
use crate::{ctx::Ctx, error::Result};
use serde_json::json;
use std::path::PathBuf;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit policy check policy.rs
  cat policy.rs | allowit policy check -";

/// Compile a policy locally: validated IR, hashes, requirements, the owner's
/// workflow steps and the recommended Stellar engines
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// The policy source; - reads stdin
    pub file: PathBuf,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let json = cx.out.json;
    run_operation("check", &args.file, json, cx)
}

/// Runs one compile-family operation on a policy file.
pub(crate) fn run_operation(
    operation: &str,
    file: &std::path::Path,
    json: bool,
    cx: &mut Ctx,
) -> Result<i32> {
    let source = compiler::read(file)?;
    compiler::process(json!({"operation":operation,"source":source}), cx, json)
}
