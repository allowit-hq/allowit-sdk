//! `allowit mcp`: the local MCP server for paid services. Reads
//! newline-delimited JSON-RPC on stdin and writes replies on stdout. Tools:
//! `allowit_call`, `allowit_services` and `allowit_status`. `allowit claude`
//! and `allowit codex` start it for the agent.
mod tools;

use crate::{
    ctx::Ctx,
    error::{Error, Result},
    gateway::args::parse,
};
use serde_json::{Value, json};
use std::io::{BufRead, Read, Write};

pub(crate) const EXAMPLES: &str = "\
Examples:
  printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}' | allowit mcp
  allowit mcp -- --sandbox";

/// Run the MCP server for paid services on stdin and stdout
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Service flags: --sandbox, --policy VERSION@ADDR, --account NAME
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "FLAGS"
    )]
    pub args: Vec<String>,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    cx.vendor()?.gateway("mcp")?;
    let mut argv = vec!["mcp".to_owned()];
    argv.extend(args.args);
    let inv = parse(&argv)?;
    if !inv.args.iter().all(|a| a == "--") {
        return Err(Error::usage(
            "allowit mcp takes only service flags: --sandbox, --policy, --account",
        ));
    }
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    let mut line = Vec::new();
    loop {
        line.clear();
        let n = std::io::Read::by_ref(&mut reader)
            .take(65_537)
            .read_until(b'\n', &mut line)
            .map_err(|_| Error::config("cannot read MCP input"))?;
        if n == 0 {
            break;
        }
        if line.len() > 65_536 {
            return Err(Error::usage("MCP message exceeds 65536 bytes"));
        }
        let message: Value = serde_json::from_slice(&line)
            .map_err(|_| Error::usage("MCP input must be a JSON object"))?;
        if message.get("id").is_none() {
            continue;
        }
        let id = message["id"].clone();
        let result = tools::dispatch(&message, &inv, &mut cx.out.stderr);
        let response = match result {
            Ok(v) => json!({"jsonrpc":"2.0","id":id,"result":v}),
            Err(e) => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":crate::output::one_line(&e.message)}})
            }
        };
        writeln!(writer, "{}", crate::redact(response.to_string()))
            .map_err(|_| Error::config("cannot write MCP response"))?;
        writer
            .flush()
            .map_err(|_| Error::config("cannot flush MCP response"))?;
    }
    Ok(0)
}
