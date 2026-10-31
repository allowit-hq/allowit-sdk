//! `allowit whoami`: the current rail, agent key and wallet, and the engine
//! account when `ALLOWIT_ACCOUNT_TOKEN` is set.
use crate::{
    config::env_value,
    ctx::Ctx,
    error::Result,
    gateway::{ACCOUNT_TOKEN_ENV, Common, args::Invocation, control},
};
use serde_json::{Value, json};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit whoami
  allowit whoami --json";

/// Show the current rail, agent key, default wallet and engine account
#[derive(clap::Args, Debug)]
pub(crate) struct Args {}

pub(crate) fn run(_: Args, cx: &mut Ctx) -> Result<i32> {
    let rail = cx.rail_opt()?;
    let mut value = json!({
        "rail": rail.map(|r| r.id),
        "agent": Value::Null,
        "agentAddress": Value::Null,
        "wallet": Value::Null,
    });
    if let Some(rail) = rail {
        let settings = cx.settings()?.rail(rail.id);
        if let Some(name) = &settings.agent {
            value["agent"] = json!(name);
            if let Ok(key) = cx.key(name) {
                value["agentAddress"] = json!(key.address);
            }
        }
        value["wallet"] = json!(cx.wallet(rail)?);
    }
    if !env_value(ACCOUNT_TOKEN_ENV).is_empty() {
        let inv = Invocation {
            group: "whoami".into(),
            args: vec![],
            common: Common::default(),
        };
        let mut reply = String::new();
        control::read(&inv, "/api/gateway/whoami", None, &mut reply)?;
        value["account"] = serde_json::from_str(&reply).unwrap_or(Value::Null);
    }
    cx.out.value(&value);
    Ok(0)
}
