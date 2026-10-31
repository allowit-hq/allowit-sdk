//! `allowit codex [-- ARGS]`: Codex with the paid-service MCP server.
use crate::{ctx::Ctx, error::Result, gateway::args::parse};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit codex -- \"Buy one search result under 0.05 USDC\"
  allowit codex -- --policy 3@POLICY_ADDRESS \"Find a weather API\"";

/// Run Codex with the AllowIt MCP server. Needs a ChatGPT sign-in; API-key
/// settings are removed
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Service flags, then Codex arguments
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "ARGS"
    )]
    pub args: Vec<String>,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    // Codex reaches paid services through the MCP tools.
    cx.vendor()?.gateway("codex")?;
    let mut argv = vec!["codex".to_owned()];
    argv.extend(args.args);
    let inv = parse(&argv)?;
    crate::agents::run("codex", &inv.args, &inv.common)
}
