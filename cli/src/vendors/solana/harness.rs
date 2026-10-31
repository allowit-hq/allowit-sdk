//! Engine harness requests for a Solana policy. `ALLOWIT_TOKEN`
//! (`owner.policy.secret`) names the policy; `ALLOWIT_URL` names the engine.
//! `eval` posts to `judge`, `exec` posts once to `transactions`, `status`
//! posts to `status`; a pending result is polled within `--wait` and never
//! resubmitted.
use crate::{
    client::Client,
    config::{Config, env_value},
    error::{Error, Result},
    output::{self, check_result, classify},
    request::{Flags, encode, request_id_valid},
    routing,
    skill::{Network, Skill, text},
    term::Out,
};
use serde_json::{Value, json};
use std::time::Duration;

/// The policy ID inside `ALLOWIT_TOKEN`.
pub(crate) fn policy() -> Result<String> {
    let token = env_value("ALLOWIT_TOKEN");
    let mut parts = token.split('.');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(_), Some(policy), Some(_), None) if !policy.is_empty() => Ok(policy.into()),
        _ => Err(Error::config(
            "set ALLOWIT_TOKEN to the policy's harness token (owner.policy.secret)",
        )),
    }
}

fn client() -> Result<Client> {
    Client::new(Config::load(&policy()?)?)
}

/// Reads the policy description and checks it against the selected rail.
fn read_skill(c: &mut Client) -> Result<Skill> {
    let s = Skill::read(c)?;
    check_rail(&s)?;
    Ok(s)
}

/// Checks a policy description against the selected rail.
fn check_rail(s: &Skill) -> Result<()> {
    match routing::selected() {
        Some(network) if routing::normalize(s.get("network")).ok() != Some(network.as_str()) => {
            Err(Error::config(
                "the selected rail differs from the policy deployment",
            ))
        }
        _ => Ok(()),
    }
}

/// `policy show`: the description, or only the policy source.
pub(crate) fn show(source: bool, out: &mut Out) -> Result<i32> {
    let mut c = client()?;
    let policy = c.cfg.policy.clone();
    let s = read_skill(&mut c)?;
    if source {
        let code = text(&s.0["policy"]);
        if code.is_empty() {
            return Err(Error::unsupported(
                "the engine did not publish this policy's source",
            ));
        }
        out.source(code);
        return Ok(0);
    }
    if out.json {
        let mut raw = s.0;
        let map = raw
            .as_object_mut()
            .ok_or_else(|| Error::unsupported("the engine returned an empty policy description"))?;
        for private in ["authorization", "accessUrl", "policy"] {
            map.remove(private);
        }
        out.json_value(&raw);
    } else {
        out.value(&description(&s, &policy));
    }
    Ok(0)
}

/// The text view of a policy description: one data field per line.
fn description(s: &Skill, policy: &str) -> Value {
    let field = |value: &str| {
        if value.is_empty() {
            Value::Null
        } else {
            json!(value)
        }
    };
    let id = s.get("policyId");
    let mut view = json!({
        "policyId": if id.is_empty() { policy } else { id },
        "title": s.title(),
        "network": field(s.get("network")),
        "status": field(s.get("status")),
        "revision": s.0["revision"].as_i64(),
        "sourceHash": field(s.get("sourceHash")),
        "expiresAt": field(s.get("expiresAt")),
    });
    if let Some(binding) = s.contract().map(|c| &c["binding"])
        && !text(&binding["irHash"]).is_empty()
    {
        view["irHash"] = binding["irHash"].clone();
        view["registryVersion"] = binding["registryVersion"].clone();
    }
    if !s.0["budget"].is_null() {
        view["budgetAllocated"] = json!(text(&s.0["budget"]["allocation"]));
        view["budgetSpent"] = json!(text(&s.0["budget"]["spent"]));
    } else if !s.0["allocation"].is_null() {
        view["budgetAllocated"] = json!(text(&s.0["allocation"]["amount"]));
    }
    let rails: serde_json::Map<String, Value> = s
        .rails()
        .into_iter()
        .map(|(rail, assets)| (rail, json!(assets)))
        .collect();
    view["rails"] = Value::Object(rails);
    if let Some(keys) = s.0["contract"]["contextU64Keys"]
        .as_array()
        .filter(|a| !a.is_empty())
    {
        view["contextFields"] = json!(keys);
    }
    if let Some(steps) = s.0["workflow"].as_array().filter(|a| !a.is_empty()) {
        view["checks"] = json!(
            steps
                .iter()
                .map(|step| text(&step["label"]).to_owned())
                .collect::<Vec<_>>()
        );
    }
    view
}

fn check_status(r: &Value, id: &str, net: Network) -> Result<()> {
    check_result(r, "status", net)?;
    let got = text(&r["requestId"]);
    if got.is_empty() {
        return Err(Error::uncertain(
            "the engine returned a status without its requestId",
        ));
    }
    if got != id && text(&r["clientRequestId"]) != id {
        return Err(Error::uncertain(format!(
            "the engine returned the status of a different request ({got})"
        )));
    }
    Ok(())
}

fn poll(
    c: &mut Client,
    id: &str,
    wait: Duration,
    r: &Value,
    command: &str,
    net: Network,
) -> Result<Value> {
    let start = text(&r["status"]).to_owned();
    let mut r = r.clone();
    let mut elapsed = Duration::ZERO;
    while elapsed < wait {
        std::thread::sleep(Duration::from_secs(2));
        let next = c
            .call("POST", "status", Some(&encode(&json!({"requestId":id}))))
            .map_err(|e| Error::uncertain(format!("checking the request status failed: {e}")))?;
        check_status(&next, id, net)?;
        if command != "status" {
            check_result(&next, command, net)?;
        }
        r = next;
        if text(&r["status"]) != start {
            break;
        }
        elapsed += Duration::from_secs(2);
    }
    Ok(r)
}

fn unknown(e: Error, kind: &str, client_id: &str, r: &Value) -> Error {
    let server_id = text(&r["requestId"]);
    let label = if request_id_valid(server_id) {
        format!(" (server requestId {server_id})")
    } else {
        String::new()
    };
    Error::uncertain(format!(
        "{e}\nthe engine accepted {kind} request {client_id}{label}, but its result is unknown; check it with allowit status {client_id}, or rerun the same command with --request-id {client_id}"
    ))
}

/// `eval` (judge) or `exec` (transactions) for one request.
pub(crate) fn submit(
    command: &str,
    flags: &Flags,
    request_id: &str,
    wait: Duration,
    out: &mut Out,
) -> Result<i32> {
    let mut c = client()?;
    let s = read_skill(&mut c)?;
    let mut b = crate::request::build_body(command, flags, &s, &mut std::io::empty())?;
    b.request_id = request_id.into();
    // Pending progress leaves the process before the POST.
    crate::flush_stderr(&mut out.stderr);
    let mut r = match c.call(
        "POST",
        if command == "exec" {
            "transactions"
        } else {
            "judge"
        },
        Some(&encode(&b)),
    ) {
        Ok(r) => r,
        Err(e) if e.code == 5 => {
            return Err(Error::uncertain(format!(
                "{e}\nthe request may have reached the engine; retry only with --request-id {request_id}"
            )));
        }
        Err(e) if e.status == Some(409) => {
            return Err(Error::uncertain(format!(
                "{e}\nrequest ID {request_id} already names a request with different details; read it with allowit status {request_id}"
            )));
        }
        Err(e) => return Err(e),
    };
    let replayed = r["replayed"] == true;
    let created = r.get("createdAt").cloned();
    check_result(&r, command, s.net()).map_err(|e| unknown(e, command, request_id, &r))?;
    if text(&r["outcome"]) == "pending" {
        let id = text(&r["requestId"]).to_string();
        if !request_id_valid(&id) {
            return Err(unknown(
                Error::uncertain("the engine returned a pending result without a valid requestId"),
                command,
                request_id,
                &r,
            ));
        }
        r = poll(&mut c, &id, wait, &r, command, s.net())
            .map_err(|e| unknown(e, command, request_id, &r))?;
    }
    if replayed {
        r["replayed"] = Value::Bool(true);
        if let Some(v) = created {
            r["createdAt"] = v;
        }
    }
    let res = classify(&r, command);
    r["state"] = json!(res.state);
    r["clientRequestId"] = json!(request_id);
    if out.json {
        r["exitCode"] = json!(res.exit);
        out.json_value(&r);
    } else {
        out.text(&r);
    }
    Ok(res.exit)
}

/// `status ID`: reads one request without creating another.
pub(crate) fn status(id: &str, wait: Duration, out: &mut Out) -> Result<i32> {
    let mut c = client()?;
    // Status recovery tolerates an unavailable description; the rail check
    // runs only when a description is available.
    let net = match Skill::read(&mut c) {
        Ok(s) => {
            check_rail(&s)?;
            s.net()
        }
        Err(e) if e.config || matches!(e.status, Some(401 | 403)) => return Err(e),
        Err(_) => Network::Unknown,
    };
    let mut r = match c.call("POST", "status", Some(&encode(&json!({"requestId":id})))) {
        Ok(r) => r,
        Err(e) if e.status == Some(404) => match c.call(
            "POST",
            "status",
            Some(&encode(&json!({"clientRequestId":id}))),
        ) {
            Err(e) if e.status == Some(404) => {
                return Err(Error::uncertain(format!(
                    "the engine has no request {id} for this policy yet; if an exec with --request-id {id} may have been sent, rerun that identical command"
                )));
            }
            x => x?,
        },
        Err(e) => return Err(e),
    };
    let map_error = |e: Error| {
        Error::uncertain(format!(
            "{e}\nthe state of request {id} is unknown; check again with allowit status {id}"
        ))
    };
    check_status(&r, id, net).map_err(map_error)?;
    let outcome = text(&r["outcome"]);
    if outcome == "pending"
        || (outcome == "awaiting_input" || ["submitted", "ready"].contains(&text(&r["status"])))
            && !wait.is_zero()
    {
        let server = text(&r["requestId"]).to_owned();
        r = poll(&mut c, &server, wait, &r, "status", net).map_err(map_error)?;
    }
    if net == Network::Unknown && output::needs_network(&r) {
        return Err(Error::uncertain(format!(
            "the engine reported outcome {} with status {} for request {id}, but the policy's network is unknown, so nothing is confirmed as recorded, signed or paid; check again once allowit policy show works",
            crate::error::quoted(text(&r["outcome"])),
            crate::error::quoted(text(&r["status"]))
        )));
    }
    let res = classify(&r, "status");
    r["state"] = json!(res.state);
    // Without the policy description the result names no network.
    if net == Network::Unknown && r.get("network").is_none() {
        r["network"] = json!("unknown");
    }
    if out.json {
        r["exitCode"] = json!(res.exit);
        out.json_value(&r);
    } else {
        out.text(&r);
    }
    Ok(res.exit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_names_the_policy() {
        crate::config::with_settings(&[("ALLOWIT_TOKEN", "o.p.secret".into())], || {
            assert_eq!(policy().unwrap(), "p");
            Ok(())
        })
        .unwrap();
        crate::config::with_settings(&[("ALLOWIT_TOKEN", "bad".into())], || {
            assert_eq!(policy().unwrap_err().code, 3);
            Ok(())
        })
        .unwrap();
    }
}
