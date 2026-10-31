//! `allowit config show [--reveal]`.
use crate::{config::env_value, ctx::Ctx, error::Result, gateway::ACCOUNT_TOKEN_ENV};
use serde_json::json;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit config show
  allowit config show --json
  allowit config show --reveal";

/// Show the effective settings: the config file, the current rail and the
/// engine credentials in use (masked unless --reveal)
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Print credential values instead of masking them
    #[arg(long)]
    pub reveal: bool,
}

fn credential(name: &str, reveal: bool) -> serde_json::Value {
    let value = env_value(name);
    if value.is_empty() {
        serde_json::Value::Null
    } else if reveal {
        json!(value)
    } else {
        json!("set")
    }
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let rail = cx.rail_opt()?.map(|r| r.id);
    let rail_source = if cx.global.rail.is_some() {
        "flag or ALLOWIT_RAIL"
    } else if rail.is_some() {
        "config.toml"
    } else {
        "unset"
    };
    let settings = cx.settings()?;
    let path = crate::settings::Settings::path(&settings.dir);
    let config = serde_json::to_value(&settings.config).unwrap_or_default();
    let url = env_value("ALLOWIT_URL");
    let engine_url = if url.is_empty() {
        serde_json::Value::Null
    } else {
        json!(url)
    };
    let value = json!({
        "path": path.to_string_lossy(),
        "rail": rail,
        "railSource": rail_source,
        "engineUrl": engine_url,
        "credentials": {
            "ALLOWIT_TOKEN": credential("ALLOWIT_TOKEN", args.reveal),
            "ALLOWIT_ACCOUNT_TOKEN": credential(ACCOUNT_TOKEN_ENV, args.reveal),
        },
        "config": config,
    });
    if args.reveal {
        // Revealed values must reach the terminal unchanged.
        cx.out.raw = true;
    }
    cx.out.json_value(&value);
    Ok(0)
}
