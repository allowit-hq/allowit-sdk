//! `allowit policy build FILE [-o OUT]`.
use super::super::compiler;
use crate::{ctx::Ctx, error::Result};
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
    let vendor = cx.vendor()?;
    if !vendor.caps().verbs.contains(&"policy build") {
        return Err(vendor.na("policy build"));
    }
    let source = compiler::read(&args.file)?;
    let built = vendor.policy_build(&source)?;
    write_source(&built, args.out.as_deref(), cx)
}

/// Prints or writes generated Rust source.
pub(crate) fn write_source(
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
