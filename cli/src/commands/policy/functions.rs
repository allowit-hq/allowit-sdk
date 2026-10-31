//! `allowit policy functions [NAME]`.
use super::super::compiler;
use crate::{
    ctx::Ctx,
    error::{Error, Result},
};
use serde_json::{Value, json};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit policy functions
  allowit policy functions transaction_cap";

/// List the policy functions with their signatures and help, or show one
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// One function name
    pub name: Option<String>,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let Some(name) = args.name else {
        return compiler::process(json!({"operation":"registry"}), cx);
    };
    let mut result = allowit_sdk::process_value(json!({"operation":"registry"}));
    result.sort_all_objects();
    let found = find(&result, &name).ok_or_else(|| {
        Error::usage(format!(
            "no policy function {name}; run allowit policy functions"
        ))
    })?;
    compiler::print(&found, cx);
    Ok(0)
}

/// Finds the registry entry whose name matches.
fn find(value: &Value, name: &str) -> Option<Value> {
    match value {
        Value::Object(map) => {
            let names = ["name", "function", "id"];
            if names
                .iter()
                .any(|k| map.get(*k).and_then(Value::as_str) == Some(name))
            {
                return Some(value.clone());
            }
            map.values().find_map(|v| find(v, name))
        }
        Value::Array(items) => items.iter().find_map(|v| find(v, name)),
        _ => None,
    }
}
