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

/// Prints an SDK result: unchanged JSON, or `key value` text lines.
pub(super) fn print(result: &Value, cx: &mut Ctx, json: bool) {
    cx.out.raw = true;
    if json {
        cx.out.stdout += &serde_json::to_string_pretty(result).unwrap_or_default();
        cx.out.stdout.push('\n');
        cx.out.kind = Kind::Json;
    } else {
        cx.out.text(result);
    }
}
