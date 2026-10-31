//! Read and mutation helpers for account and service controls. Agent keys
//! stay local. Used by `whoami`, `setup` and `skills`: reads send `GET`,
//! mutations send one `POST` under an operation ID.
use super::{
    Gateway,
    args::{self, Invocation},
    operation::{operation_id, render_operation},
};
use crate::{
    error::{Error, Result},
    output,
};
use serde_json::{Value, json};

/// Refuses account input outside an allowlist, so future key fields cannot
/// silently become transport data.
pub(crate) fn public_fields(input: &Value, allowed: &[&str]) -> Result<()> {
    let fields = input
        .as_object()
        .ok_or_else(|| Error::usage("Account controls require a public JSON object"))?;
    if fields.keys().any(|name| !allowed.contains(&name.as_str())) {
        return Err(Error::usage(
            "Account input contains a field outside its public schema. Private keys, seeds, shares and wallet files must remain local",
        ));
    }
    if fields.values().any(|value| {
        value.as_str().is_none_or(|text| {
            text.is_empty() || text.len() > 2048 || text.chars().any(char::is_control)
        })
    }) {
        return Err(Error::usage(
            "Account public fields must contain bounded text",
        ));
    }
    Ok(())
}

/// Reads one backend route and prints its JSON. `query` is the optional search text.
pub(crate) fn read(
    inv: &Invocation,
    route: &str,
    query: Option<&String>,
    stdout: &mut String,
) -> Result<i32> {
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in [
        ("account", &inv.common.account),
        ("policySelector", &inv.common.policy_selector),
        ("rail", &inv.common.rail),
        ("rpcUrl", &inv.common.rpc_url),
    ] {
        if let Some(s) = v {
            q.append_pair(k, s);
        }
    }
    q.append_pair("sandbox", if inv.common.sandbox { "true" } else { "false" });
    if let Some(s) = query {
        q.append_pair("query", s);
    }
    let value = Gateway::new(&inv.common)?.send("GET", &format!("{route}?{}", q.finish()), None)?;
    *stdout += &output::json(&value);
    Ok(0)
}

/// Sends one control mutation under a fresh or explicit operation ID.
pub(crate) fn mutate(
    inv: &Invocation,
    route: &str,
    action: &str,
    input: Value,
    stdout: &mut String,
    out: &mut crate::term::Out,
) -> Result<i32> {
    let mut request = args::context(&inv.common);
    request["action"] = json!(action);
    request["input"] = input;
    let id = operation_id(&inv.common, &request);
    request["operationId"] = json!(id);
    // The operation ID reaches the terminal before the request leaves.
    out.generated_id(&id);
    crate::flush_stderr(&mut out.stderr);
    let stderr = &mut out.stderr;
    let value = Gateway::new(&inv.common)?.send("POST", route, Some(&request))?;
    if value.get("operationId").and_then(Value::as_str) != Some(id.as_str()) {
        return Err(Error::uncertain(
            "Allowit control response has a different operationId",
        ));
    }
    if value.get("state").is_some() {
        return render_operation(&value, &inv.common, stdout, stderr);
    }
    *stdout += &output::json(&value);
    Ok(0)
}
