//! `allowit policy build FILE [-o OUT]`.
use super::super::compiler;
use crate::{ctx::Ctx, error::Result, exit::not_available, routing::Family};
use std::path::PathBuf;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit --rail stellar:testnet policy build policy.rs
  allowit --rail stellar:testnet policy build policy.rs -o wallet.rs";

/// Build a policy for the selected rail. On Stellar this is the Rust source of
/// a custom policy wallet
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// The policy source; - reads stdin
    pub file: PathBuf,
    /// Write the build to OUT instead of stdout
    #[arg(short = 'o', long = "out", value_name = "OUT")]
    pub out: Option<PathBuf>,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let rail = cx.rail()?;
    if rail.family != Family::Stellar {
        return Err(not_available("policy build", rail.id));
    }
    let source = compiler::read(&args.file)?;
    let policy = allowit_sdk::compile_stellar_policy(&source)
        .map_err(|e| compiler::failure(e.to_string()))?;
    stellar_source(&policy.rust_source, args.out.as_deref(), cx)
}

/// Prints or writes generated Stellar Rust.
pub(crate) fn stellar_source(
    rust: &str,
    output: Option<&std::path::Path>,
    cx: &mut Ctx,
) -> Result<i32> {
    match output {
        Some(path) => std::fs::write(path, rust)
            .map_err(|e| compiler::failure(format!("{}: {e}", path.display())))?,
        None => {
            cx.out.raw = true;
            cx.out.source(rust);
        }
    }
    Ok(0)
}
