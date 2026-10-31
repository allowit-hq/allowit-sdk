//! `allowit policy test FILE --context CTX [--mode oracle|contract]`.
use super::super::compiler;
use crate::{ctx::Ctx, error::Result};
use serde_json::{Value, json};
use std::path::PathBuf;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit policy test policy.rs --context context.json
  allowit policy test policy.rs --context context.json --mode contract";

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum Mode {
    Oracle,
    Contract,
}

/// Evaluate a policy locally for one request context
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// The policy source; - reads stdin
    pub file: PathBuf,
    /// The request context JSON file; - reads stdin
    #[arg(long, value_name = "CTX.json")]
    pub context: PathBuf,
    /// The evaluation profile
    #[arg(long, value_enum, default_value = "oracle")]
    pub mode: Mode,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let json = cx.out.json;
    evaluate(&args.file, &args.context, args.mode, json, cx)
}

pub(crate) fn evaluate(
    file: &std::path::Path,
    context: &std::path::Path,
    mode: Mode,
    json: bool,
    cx: &mut Ctx,
) -> Result<i32> {
    let source = compiler::read(file)?;
    let context: Value = serde_json::from_str(&compiler::read(context)?)
        .map_err(|e| compiler::failure(format!("{}: {e}", context.display())))?;
    let profile = match mode {
        Mode::Oracle => "oracle",
        Mode::Contract => "contract",
    };
    compiler::process(
        json!({"operation":"evaluate","source":source,"context":context,"profile":profile}),
        cx,
        json,
    )
}
