//! `allowit claude [-- ARGS]`: Claude Code with the paid-service MCP server.
use crate::{ctx::Ctx, error::Result, gateway::args::parse};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit claude -- -p \"Find a weather API and call it\"
  allowit claude -- --sandbox --max-amount 1 -p \"List services\"";

/// Run Claude Code with the AllowIt MCP server. Needs a Claude subscription
/// sign-in; API-key settings are removed
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Service flags, then Claude Code arguments
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "ARGS"
    )]
    pub args: Vec<String>,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    // Claude Code reaches paid services through the MCP tools.
    cx.vendor()?.gateway("claude")?;
    let mut argv = vec!["claude".to_owned()];
    argv.extend(args.args);
    let inv = parse(&argv)?;
    crate::agents::run("claude", &inv.args, &inv.common)
}
