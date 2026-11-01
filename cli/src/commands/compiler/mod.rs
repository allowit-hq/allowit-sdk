//! Shared helpers for the local policy compiler verbs (`policy check`,
//! `policy build`, `policy test`, `policy functions`, hidden `json`). They run
//! the SDK locally and read no rail, wallet or credential. They print the SDK
//! result as text, or unchanged as JSON with `--json`; the hidden commands of
//! earlier releases always print JSON.
use crate::{
    ctx::Ctx,
    error::{Error, Result},
    term::Kind,
};
use serde_json::Value;
use std::{io::Read, path::Path};

/// A compiler failure: an invalid policy or input file exits 2.
pub(super) fn failure(message: impl Into<String>) -> Error {
    Error::usage(message)
}

/// Reads a policy or JSON file. `-` reads stdin.
pub(super) fn read(path: &Path) -> Result<String> {
    if path == Path::new("-") {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| failure(e.to_string()))?;
        Ok(s)
    } else {
        std::fs::read_to_string(path).map_err(|e| failure(format!("{}: {e}", path.display())))
    }
}

/// Runs one wire-protocol request and prints the result, as sorted JSON when
/// `json` is set. A result with `"ok": false` exits 2 after printing.
pub(super) fn process(request: Value, cx: &mut Ctx, json: bool) -> Result<i32> {
    let mut result = allowit_sdk::process_value(request);
    // Keys print in sorted order, as the standalone compiler printed them.
    result.sort_all_objects();
    print(&result, cx, json);
    if result["ok"] == false {
        return Err(failure(
            "the policy operation failed; see the result on stdout",
        ));
    }
    Ok(0)
}

/// Prints an SDK result: unchanged JSON, or a text summary.
pub(super) fn print(result: &Value, cx: &mut Ctx, json: bool) {
    cx.out.raw = true;
    if json {
        cx.out.stdout += &serde_json::to_string_pretty(result).unwrap_or_default();
        cx.out.stdout.push('\n');
        cx.out.kind = Kind::Json;
    } else if let Some(functions) = result["functions"].as_array() {
        // The function registry: one row per function.
        let mut rows = vec![vec!["NAME".to_owned(), "EFFECT".into(), "TITLE".into()]];
        for f in functions {
            let text = |key: &str| f[key].as_str().unwrap_or("-").to_owned();
            rows.push(vec![text("name"), text("effect"), text("title")]);
        }
        cx.out.table(&rows);
    } else {
        cx.out.text(&summary(result));
    }
}

/// A compiler result for text output: the policy IR and source give way to
/// the names of its calls, workflow steps and recommended engines. `--json`
/// prints them all.
fn summary(result: &Value) -> Value {
    let mut view = result.clone();
    let names = |items: &Value, key: &str| -> Value {
        match items.as_array() {
            Some(items) => items
                .iter()
                .filter_map(|i| i[key].as_str())
                .map(|s| Value::String(s.to_owned()))
                .collect(),
            None => items.clone(),
        }
    };
    if let Some(policy) = view.get_mut("policy").and_then(Value::as_object_mut) {
        policy.remove("ir");
        policy.remove("source");
        if let Some(calls) = policy.get("calls") {
            let calls = names(calls, "name");
            policy.insert("calls".into(), calls);
        }
        if let Some(workflow) = policy.get("workflow") {
            let steps = names(workflow, "label");
            policy.insert("workflow".into(), steps);
        }
    }
    // Any list of engine recommendations prints one "ENGINE (RAIL)" per entry.
    if let Some(fields) = view.as_object_mut() {
        for value in fields.values_mut() {
            if let Some(engines) = engine_names(value) {
                *value = engines;
            }
        }
    }
    view.sort_all_objects();
    view
}

/// A non-empty array whose every entry has string `engine` and `rail` fields,
/// as "ENGINE (RAIL)" strings. Any other value gives `None`.
fn engine_names(value: &Value) -> Option<Value> {
    let items = value.as_array().filter(|items| !items.is_empty())?;
    items
        .iter()
        .map(|e| {
            let engine = e.get("engine")?.as_str()?;
            let rail = e.get("rail")?.as_str()?;
            Some(Value::String(format!("{engine} ({rail})")))
        })
        .collect::<Option<Vec<_>>>()
        .map(Value::Array)
}

#[cfg(test)]
mod tests {
    use super::summary;
    use serde_json::json;

    #[test]
    fn engine_lists_print_as_engine_and_rail_under_any_key() {
        let result = json!({
            "ok": true,
            "engines": [{"engine": "a", "rail": "r1", "score": 1}],
            "other": [{"engine": "a"}],
            "empty": [],
        });
        assert_eq!(
            summary(&result),
            json!({
                "ok": true,
                "engines": ["a (r1)"],
                "other": [{"engine": "a"}],
                "empty": [],
            })
        );
    }
}
