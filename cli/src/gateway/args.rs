//! Paid-service command input. Vendor payment protocols remain on the backend.
//! Parses a service command and its service flags into an [`Invocation`].
pub(crate) use super::{Common, Money};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::io::Read;
use url::Url;

/// Top-level commands served through the Allowit backend.
pub(crate) const GROUPS: &[&str] = &[
    "call", "curl", "wget", "http", "fetch", "claude", "codex", "whoami", "mcp", "skills", "setup",
];

#[derive(Debug)]
pub(crate) struct Invocation {
    pub group: String,
    pub args: Vec<String>,
    pub common: Common,
}

/// Parses `GROUP ARGS...` for the paid-service commands. Their service flags
/// (`--policy`, `--sandbox`, `--account`, `--request-id`, `--rail` with
/// `--rpc`, `--max-amount`, `--currency`) may appear among the arguments.
/// `--max-payment` and `--budget` are earlier names of `--max-amount`.
pub(crate) fn parse(args: &[String]) -> Result<Invocation> {
    let mut common = Common::default();
    let mut rest = Vec::new();
    let mut group = None;
    let mut i = 0;
    let mut literal = false;
    while i < args.len() {
        let arg = &args[i];
        if literal {
            rest.push(arg.clone());
            i += 1;
            continue;
        }
        if arg == "--" {
            literal = true;
            rest.push(arg.clone());
            i += 1;
            continue;
        }
        let (name, inline) = arg
            .split_once('=')
            .map(|(a, b)| (a, Some(b.to_owned())))
            .unwrap_or((arg.as_str(), None));
        let is_value = matches!(
            name,
            "--policy"
                | "--account"
                | "--request-id"
                | "--operation-id"
                | "--rail"
                | "--rpc"
                | "--max-amount"
                | "--max-payment"
                | "--budget"
                | "--currency"
        );
        if is_value {
            let value = match inline {
                Some(s) => s,
                None => {
                    i += 1;
                    args.get(i)
                        .cloned()
                        .ok_or_else(|| Error::usage(format!("{name} requires a value")))?
                }
            };
            if value.is_empty() || value.len() > 2048 || value.chars().any(char::is_control) {
                return Err(Error::usage(format!("{name} has an invalid value")));
            }
            match name {
                "--policy" => {
                    validate_selector(&value)?;
                    set_once(&mut common.policy_selector, value, name)?;
                }
                "--account" => set_once(&mut common.account, value, name)?,
                "--request-id" | "--operation-id" => {
                    if !crate::request::request_id_valid(&value) {
                        return Err(Error::usage(
                            "--request-id must contain 8 to 100 letters, digits, periods, underscores, colons or hyphens",
                        ));
                    }
                    set_once(&mut common.operation_id, value, name)?;
                }
                "--rail" => set_once(&mut common.rail, value, name)?,
                "--currency" => {
                    if value.len() > 32
                        || !value
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                    {
                        return Err(Error::usage("--currency requires a stablecoin symbol"));
                    }
                    if let Some(money) = &mut common.max_debit {
                        if money.currency != "USDC" && money.currency != value {
                            return Err(Error::usage(
                                "--max-amount names another currency than --currency",
                            ));
                        }
                        money.currency = value.clone();
                    }
                    set_once(&mut common.currency, value, name)?;
                }
                "--rpc" => {
                    public_url(&value)?;
                    set_once(&mut common.rpc_url, value, name)?;
                }
                "--max-amount" | "--max-payment" | "--budget" => {
                    if common.max_debit.is_some() {
                        return Err(Error::usage("give one payment limit"));
                    }
                    // CODE:DECIMAL names the currency, as `call --max-amount` does.
                    let (code, amount) = match value.split_once(':') {
                        Some((code, amount)) if name == "--max-amount" => (Some(code), amount),
                        _ => (None, value.as_str()),
                    };
                    if let Some(code) = code {
                        if code.is_empty()
                            || code.len() > 32
                            || !code
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                        {
                            return Err(Error::usage(
                                "--max-amount requires CODE:DECIMAL or a decimal",
                            ));
                        }
                        if common.currency.as_deref().is_some_and(|c| c != code) {
                            return Err(Error::usage(
                                "--max-amount names another currency than --currency",
                            ));
                        }
                    }
                    let amount = amount.trim_start_matches('$');
                    if amount.is_empty()
                        || !amount.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                        || amount.matches('.').count() > 1
                        || !amount.bytes().any(|b| b.is_ascii_digit())
                    {
                        return Err(Error::usage(
                            "--max-amount requires CODE:DECIMAL or a decimal USD amount",
                        ));
                    }
                    common.max_debit = Some(Money {
                        currency: code
                            .map(str::to_owned)
                            .or_else(|| common.currency.clone())
                            .unwrap_or_else(|| "USDC".into()),
                        amount: amount.into(),
                    });
                }
                _ => unreachable!(),
            }
        } else if name == "--sandbox" {
            if inline.is_some() {
                return Err(Error::usage("--sandbox does not take a value"));
            }
            common.sandbox = true;
        } else if name == "--json" {
            common.json = true;
        } else if group.is_none() && GROUPS.contains(&arg.as_str()) {
            group = Some(arg.clone());
        } else if group.is_none() {
            return Err(Error::usage(
                "expected an Allowit command after global flags",
            ));
        } else {
            rest.push(arg.clone());
        }
        i += 1;
    }
    if common.rail.is_some() != common.rpc_url.is_some() {
        return Err(Error::usage("provide --rail and the exact --rpc together"));
    }
    Ok(Invocation {
        group: group.ok_or_else(|| Error::usage("expected an Allowit command"))?,
        args: rest,
        common,
    })
}

fn set_once(slot: &mut Option<String>, value: String, name: &str) -> Result<()> {
    if slot.is_some() {
        return Err(Error::usage(format!("{name} may be given only once")));
    }
    *slot = Some(value);
    Ok(())
}

pub(crate) fn validate_selector(value: &str) -> Result<()> {
    let Some((version, address)) = value.split_once('@') else {
        return Err(Error::usage("--policy requires version@addr"));
    };
    if version.is_empty()
        || version.len() > 128
        || address.is_empty()
        || address.len() > 256
        || !version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
        || !address
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_:-".contains(&b))
    {
        return Err(Error::usage(
            "--policy requires one bounded version@addr selector",
        ));
    }
    Ok(())
}

pub(crate) fn public_url(raw: &str) -> Result<Url> {
    let u =
        Url::parse(raw).map_err(|_| Error::usage("request URL must be an absolute HTTP(S) URL"))?;
    if !matches!(u.scheme(), "http" | "https")
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
    {
        return Err(Error::usage(
            "request URL must use HTTP(S), without credentials or a fragment",
        ));
    }
    Ok(u)
}

pub(crate) fn body_file(path: &str) -> Result<String> {
    let meta =
        std::fs::metadata(path).map_err(|_| Error::usage("cannot read the explicit body file"))?;
    if !meta.is_file() || meta.len() > 32 << 10 {
        return Err(Error::usage(
            "body file must be a regular file no larger than 32768 bytes",
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    let file = options
        .open(path)
        .map_err(|_| Error::usage("cannot open the explicit body file"))?;
    if !file
        .metadata()
        .map_err(|_| Error::usage("cannot inspect body file"))?
        .is_file()
    {
        return Err(Error::usage("body path must remain a regular file"));
    }
    let mut bytes = Vec::new();
    file.take((32 << 10) + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::usage("cannot read body file"))?;
    if bytes.len() > 32 << 10 {
        return Err(Error::usage("body file exceeds 32768 bytes"));
    }
    String::from_utf8(bytes).map_err(|_| Error::usage("this HTTP request body requires UTF-8 text"))
}

pub(crate) fn context(common: &Common) -> Value {
    let mut v = json!({"sandbox":common.sandbox});
    if let Some(n) = crate::routing::selected() {
        v["network"] = json!(n);
    }
    for (k, val) in [
        ("policySelector", &common.policy_selector),
        ("account", &common.account),
        ("rail", &common.rail),
        ("rpcUrl", &common.rpc_url),
    ] {
        if let Some(s) = val {
            v[k] = json!(s);
        }
    }
    if let Some(m) = &common.max_debit {
        v["maxDebitMoney"] = json!(m);
    }
    v
}

pub(crate) fn query_context(common: &Common) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in [
        ("account", &common.account),
        ("policySelector", &common.policy_selector),
        ("rail", &common.rail),
        ("rpcUrl", &common.rpc_url),
    ] {
        if let Some(s) = v {
            query.append_pair(k, s);
        }
    }
    query.append_pair("sandbox", if common.sandbox { "true" } else { "false" });
    query.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| (*s).into()).collect()
    }
    #[test]
    fn policy_is_explicit_version_selector() {
        let v = parse(&args(&[
            "call",
            "hash",
            "--policy",
            "v1@wallet",
            "--sandbox",
        ]))
        .unwrap();
        assert_eq!(v.common.policy_selector.as_deref(), Some("v1@wallet"));
        assert!(v.common.sandbox);
        assert!(parse(&args(&["call", "hash", "--policy", "policy-id"])).is_err());
        assert!(
            parse(&args(&["call", "hash"]))
                .unwrap()
                .common
                .policy_selector
                .is_none()
        );
    }
}
