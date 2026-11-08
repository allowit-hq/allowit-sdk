//! Shared parameter display and editing. Callers retain policy admission and source binding checks.
use crate::parameter_reads::{ParamRead, param_reads};
use chrono::{DateTime, NaiveDate, NaiveDateTime, SecondsFormat};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Shared display asset metadata, resolved by the selected vendor adapter.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Asset {
    pub symbol: String,
    pub address: String,
    pub decimals: u32,
}
/// Parameter values and display metadata. This is not a policy admission or execution capability.
#[derive(Clone, Debug)]
pub struct Parameters {
    pub params: serde_json::Map<String, Value>,
    pub kinds: BTreeMap<String, String>,
    pub asset: Asset,
    pub unresolved: Vec<String>,
}

/// Longest value that one edit may carry, in bytes.
pub const MAX_VALUE: usize = 4096;
fn canonical(key: &str) -> Option<&'static allowit_core::policy::CanonicalFunctionDescriptor> {
    static FUNCTIONS: std::sync::LazyLock<allowit_core::policy::CanonicalFunctionRegistry> =
        std::sync::LazyLock::new(|| {
            allowit_core::policy::canonical_function_registry()
                .expect("compiled Core function metadata")
        });
    FUNCTIONS
        .functions
        .iter()
        .find(|function| function.name == key)
}
fn blocks_all_at_zero(key: &str) -> bool {
    canonical(key).is_some_and(|function| {
        function.parameter.requires_amount_context || function.parameter.semantic_type == "count"
    })
}

/// Display value of a recipients list that the owner has not supplied yet.
const NOT_SET: &str = "Not set yet";

/// One param as the owner reviews it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Field {
    pub key: String,
    /// Plain label, such as "Daily volume".
    pub label: String,
    /// Asset symbol of an amount.
    pub unit: Option<String>,
    /// The text that an edit field starts with, or `None` when the owner cannot edit the param
    /// here, such as the assets.
    pub input: Option<String>,
    /// The value in human units, such as "5 USDC".
    pub shown: String,
}
/// One changed param.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub key: String,
    /// The label in a sentence, such as "daily volume".
    pub noun: String,
    pub before: String,
    pub after: String,
}

fn kind(draft: &Parameters, key: &str) -> String {
    if let Some(kind) = draft.kinds.get(key) {
        return kind.clone();
    }
    if let Some(function) = canonical(key) {
        return match function.parameter.semantic_type.as_str() {
            "timestamp" => "time".into(),
            other => other.into(),
        };
    }
    match draft.params.get(key).and_then(Value::as_object) {
        Some(o) if o.contains_key("Int") => "int",
        Some(o) if o.contains_key("Bool") => "bool",
        Some(o) if o.contains_key("Str") => "text",
        Some(o) if o.contains_key("List") => "addresses",
        _ => "",
    }
    .into()
}

fn humanize(key: &str) -> String {
    let words = key.replace('_', " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The Core registry label of a canonical key, such as "Daily volume", or a custom key in words.
fn label(key: &str) -> String {
    canonical(key).map_or_else(|| humanize(key), |function| function.label.clone())
}

fn list(value: &Value) -> Option<&Vec<Value>> {
    value.get("List").and_then(Value::as_array)
}

fn int(value: &Value) -> Option<i128> {
    value.get("Int")?.as_str()?.parse().ok()
}

fn addresses(value: &Value) -> Option<Vec<String>> {
    list(value)?
        .iter()
        .map(|item| item.get("Addr").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

/// The names of an assets list: the installed asset as its symbol, any other asset as its address.
fn asset_names<'a>(items: &'a [String], asset: &'a Asset) -> Vec<&'a str> {
    items
        .iter()
        .map(|item| {
            if *item == asset.address {
                asset.symbol.as_str()
            } else {
                item.as_str()
            }
        })
        .collect()
}

/// Decimal text of `units` with `decimals` places, without trailing zeros.
fn decimal(units: i128, decimals: u32) -> String {
    let scale = 10i128.pow(decimals);
    let (whole, fraction) = (units / scale, (units % scale).abs());
    let fraction = format!("{fraction:0width$}", width = decimals as usize);
    let fraction = fraction.trim_end_matches('0');
    let sign = if units < 0 && whole == 0 { "-" } else { "" };
    if fraction.is_empty() {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{fraction}")
    }
}

/// A Unix time as UTC text to the minute, which [`parse_time`] reads back: "2026-11-07 14:03 UTC".
pub fn time_text(at: i64) -> Option<String> {
    let time = DateTime::from_timestamp(at, 0)?;
    Some(time.format("%Y-%m-%d %H:%M UTC").to_string())
}

/// The edit text of a Unix time: [`time_text`], with the seconds when they are not 0, so saving
/// the text unchanged keeps the time.
pub fn time_input(at: i64) -> Option<String> {
    let time = DateTime::from_timestamp(at, 0)?;
    let format = if at % 60 == 0 {
        "%Y-%m-%d %H:%M UTC"
    } else {
        "%Y-%m-%d %H:%M:%S UTC"
    };
    Some(time.format(format).to_string())
}

/// A UTC time from owner text: RFC 3339, "2026-11-07 14:03 UTC", "2026-11-07T14:03" or a date
/// alone, which means 00:00 UTC on that date.
fn parse_time(text: &str) -> Option<i64> {
    let text = text.trim();
    if let Ok(time) = DateTime::parse_from_rfc3339(text) {
        return Some(time.timestamp());
    }
    let text = match text.len().checked_sub(3) {
        Some(end) if text.is_char_boundary(end) && text[end..].eq_ignore_ascii_case("utc") => {
            text[..end].trim_end()
        }
        _ => text,
    };
    for format in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(time) = NaiveDateTime::parse_from_str(text, format) {
            return Some(time.and_utc().timestamp());
        }
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|time| time.and_utc().timestamp())
}

/// The edit text and the display text of one stored value. The edit text is `None` when the
/// owner cannot edit the value here.
fn render(kind: &str, value: &Value, asset: &Asset) -> (Option<String>, String) {
    let rendered = match kind {
        "amount" => int(value).map(|n| {
            let amount = decimal(n, asset.decimals);
            (Some(amount.clone()), format!("{amount} {}", asset.symbol))
        }),
        "count" | "int" => int(value).map(|n| (Some(n.to_string()), n.to_string())),
        "addresses" => addresses(value).map(|items| {
            let text = items.join(", ");
            let shown = if items.is_empty() {
                NOT_SET.to_string()
            } else {
                text.clone()
            };
            (Some(text), shown)
        }),
        "assets" => addresses(value).map(|items| (None, asset_names(&items, asset).join(", "))),
        "time" | "duration" => int(value)
            .and_then(|at| i64::try_from(at).ok())
            .and_then(|at| Some((time_input(at), time_text(at)?))),
        "operations" => match (value.get("Str").and_then(Value::as_str), list(value)) {
            (Some("*"), _) => Some((None, "All operations".into())),
            (_, Some(items)) => Some((
                None,
                items
                    .iter()
                    .filter_map(|item| item.get("Str").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(", "),
            )),
            _ => None,
        },
        "bool" => value.get("Bool").and_then(Value::as_bool).map(|b| {
            (
                Some(if b { "yes" } else { "no" }.into()),
                if b { "Yes" } else { "No" }.into(),
            )
        }),
        "text" => value
            .get("Str")
            .and_then(Value::as_str)
            .map(|text| (Some(text.to_string()), text.to_string())),
        _ => None,
    };
    rendered.unwrap_or_else(|| (None, value.to_string()))
}

/// Where the source first reads `key`: through its canonical constant, such as
/// `allowit::DAILY_VOLUME_LIMIT`, or its string literal, such as `"large_purchase"`.
fn position(code: &str, key: &str) -> usize {
    let constant = format!("allowit::{}", key.to_uppercase());
    let literal = format!("\"{key}\"");
    [code.find(&constant), code.find(&literal)]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(usize::MAX)
}

/// The integer range that every `allowit::param` read of a param admits, or `None` when a read
/// has a type that the source analysis cannot bound, such as an inferred one. A param without such reads
/// has the full range: the library bounds of its rule apply.
fn read_range(reads: Option<&Vec<ParamRead>>) -> Option<(i128, i128)> {
    reads
        .into_iter()
        .flatten()
        .try_fold((i128::MIN, i128::MAX), |(low, high), read| {
            let (min, max) = match read {
                ParamRead::Typed(name) => match name.as_str() {
                    "u32" => (0, u32::MAX.into()),
                    "u64" => (0, u64::MAX.into()),
                    "i32" => (i32::MIN.into(), i32::MAX.into()),
                    "i64" => (i64::MIN.into(), i64::MAX.into()),
                    "i128" => (i128::MIN, i128::MAX),
                    _ => return None,
                },
                ParamRead::Inferred => return None,
            };
            Some((low.max(min), high.min(max)))
        })
}

fn bounded_range(key: &str, reads: Option<&Vec<ParamRead>>) -> Option<(i128, i128)> {
    let (low, high) = read_range(reads)?;
    match canonical(key).and_then(|function| function.parameter.integer_range.as_ref()) {
        Some(range) => Some((
            low.max(range.min.parse().ok()?),
            high.min(range.max.parse().ok()?),
        )),
        None => Some((low, high)),
    }
}

/// Whether `kind` stores an integer, which a typed read bounds. A time stores Unix seconds.
fn integer(kind: &str) -> bool {
    matches!(kind, "amount" | "count" | "int" | "time" | "duration")
}

/// The params of a portable draft in the order in which its source `code` reads them. An
/// integer param is shown but not editable here when the source analysis fails or cannot bound
/// its reads.
pub fn fields(draft: &Parameters, code: &str) -> Vec<Field> {
    fields_with(draft, code, param_reads(code).ok().as_ref())
}

/// [`fields`] with the source's param reads, or `None` when the source analysis fails.
fn fields_with(
    draft: &Parameters,
    code: &str,
    reads: Option<&BTreeMap<String, Vec<ParamRead>>>,
) -> Vec<Field> {
    let mut keys: Vec<&String> = draft.params.keys().collect();
    keys.sort_by_key(|key| (position(code, key), key.as_str()));
    keys.into_iter()
        .map(|key| {
            let value = &draft.params[key];
            let kind = kind(draft, key);
            let (input, shown) = render(&kind, value, &draft.asset);
            let input = input.filter(|_| {
                !integer(&kind)
                    || reads.is_some_and(|reads| bounded_range(key, reads.get(key)).is_some())
            });
            Field {
                key: key.clone(),
                label: label(key),
                unit: (kind == "amount").then(|| draft.asset.symbol.clone()),
                input,
                shown,
            }
        })
        .collect()
}

/// One line per param, such as "Daily volume: 5 USDC".
pub fn lines(draft: &Parameters, code: &str) -> String {
    fields(draft, code)
        .iter()
        .map(|f| format!("{}: {}", f.label, f.shown))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Owner text in an error message, shortened.
fn clip(text: &str) -> String {
    let mut out: String = text.chars().take(64).collect();
    if out.len() < text.len() {
        out.push('\u{2026}');
    }
    out
}

/// The number of an amount, with an optional asset symbol after it: "3", "3 USDC" or "3usdc".
/// An asset symbol can start with a digit; then a space must separate the
/// number from the symbol, because "32XLM" could also mean 32 of another asset.
pub fn amount_number<'a>(text: &'a str, symbol: &str) -> Option<&'a str> {
    let split = text.len().checked_sub(symbol.len()).filter(|&at| {
        !symbol.is_empty() && text.is_char_boundary(at) && text[at..].eq_ignore_ascii_case(symbol)
    });
    match split {
        Some(at)
            if !symbol.starts_with(|c: char| c.is_ascii_digit())
                || text[..at].ends_with(char::is_whitespace) =>
        {
            Some(text[..at].trim())
        }
        _ if !text.contains(|c: char| c.is_ascii_alphabetic()) => Some(text),
        _ => None,
    }
}

/// The stored encoding of one edited value.
pub fn parse(
    kind: &str,
    label: &str,
    input: &str,
    asset: &Asset,
    now: i64,
) -> Result<Value, String> {
    let text = input.trim();
    if text.is_empty() {
        return Err(format!("{label}: enter a value."));
    }
    match kind {
        "amount" => {
            let number = amount_number(text, &asset.symbol).ok_or_else(|| {
                format!(
                    "{label}: \"{}\" is not a {} amount. Enter a number such as 3 or 2.5.",
                    clip(text),
                    asset.symbol
                )
            })?;
            let units = base_units(number, asset.decimals).map_err(|_| {
                format!(
                    "{label}: \"{}\" is not a {} amount. Enter a number with at most {} decimal places, such as 3 or 2.5.",
                    clip(text),
                    asset.symbol,
                    asset.decimals
                )
            })?;
            Ok(json!({"Int": units.to_string()}))
        }
        "count" => match text.parse::<u64>() {
            Ok(n) => Ok(json!({"Int": n.to_string()})),
            Err(_) => Err(format!(
                "{label}: \"{}\" is not a whole number. Enter a number such as 10.",
                clip(text)
            )),
        },
        "int" => text
            .parse::<i128>()
            .map(|n| json!({"Int": n.to_string()}))
            .map_err(|_| format!("{label}: \"{}\" is not a whole number.", clip(text))),
        "addresses" => {
            let mut items: Vec<&str> = Vec::new();
            for address in text
                .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
                .filter(|a| !a.is_empty())
            {
                if let Some(error) = crate::input::address::wallet_error(address) {
                    return Err(format!("{label}: {error}"));
                }
                if !items.contains(&address) {
                    items.push(address);
                }
            }
            if items.is_empty() {
                return Err(format!("{label}: enter at least one address."));
            }
            Ok(json!({"List": items.iter().map(|a| json!({"Addr": a})).collect::<Vec<_>>()}))
        }
        "time" | "duration" => {
            let at = parse_time(text).ok_or_else(|| {
                format!(
                    "{label}: \"{}\" is not a date. Enter a UTC date and time such as 2026-11-30 or 2026-11-30 18:00 UTC.",
                    clip(text)
                )
            })?;
            if at <= now {
                return Err(format!(
                    "{label}: {} has already passed. Choose a time in the future.",
                    clip(text)
                ));
            }
            Ok(json!({"Int": at.to_string()}))
        }
        "bool" => match text.to_ascii_lowercase().as_str() {
            "yes" | "true" => Ok(json!({"Bool": true})),
            "no" | "false" => Ok(json!({"Bool": false})),
            _ => Err(format!("{label}: enter yes or no.")),
        },
        "text" => Ok(json!({"Str": text})),
        _ => Err(format!("{label} cannot be changed here.")),
    }
}

/// Set the param `key` of a portable draft whose stored source is `code` to the owner's `input`
/// in human units. Returns the new portable draft and the change, or a message for the owner. See
/// [`set_all`].
pub fn set(
    draft: &Parameters,
    code: &str,
    key: &str,
    input: &str,
    now: i64,
) -> Result<(Parameters, Change), String> {
    let edits = BTreeMap::from([(key.to_string(), input.to_string())]);
    let (next, mut changes) = set_all(draft, code, &edits, now)?;
    Ok((next, changes.remove(0)))
}

/// Set the params of a portable draft whose stored source is `code` to the owner's `edits`: the
/// text of each edited param in human units, by param key. One Save sends every field of the
/// card, so a value that equals the current one stays as it is, and the field's own start text
/// is never checked again. Returns the new portable draft and the changes in source order, or a
/// message for the owner when any value is refused or no value changes. The new draft has the
/// same parameter schema. The caller must verify its source binding and private admission rules.
pub fn set_all(
    draft: &Parameters,
    code: &str,
    edits: &BTreeMap<String, String>,
    now: i64,
) -> Result<(Parameters, Vec<Change>), String> {
    let reads = param_reads(code).ok();
    let fields = fields_with(draft, code, reads.as_ref());
    if let Some(key) = edits.keys().find(|k| !fields.iter().any(|f| &f.key == *k)) {
        return Err(format!("\"{}\" is not a value of this policy.", clip(key)));
    }
    let mut params = draft.params.clone();
    let mut changed = Vec::new();
    for field in &fields {
        let Some(input) = edits.get(&field.key) else {
            continue;
        };
        // The start text is the current value, even an expiry that has passed since.
        if field.input.as_deref() == Some(input.as_str()) {
            continue;
        }
        let kind = kind(draft, &field.key);
        let field_reads = reads.as_ref().and_then(|reads| reads.get(&field.key));
        if let Some(value) = edited(draft, field, &kind, input, field_reads, now)? {
            params.insert(field.key.clone(), value);
            changed.push((field, kind));
        }
    }
    if changed.is_empty() {
        return Err(match fields.iter().find(|f| edits.contains_key(&f.key)) {
            Some(field) if edits.len() == 1 => {
                format!("{} is already {}.", field.label, field.shown)
            }
            _ => "No value changed. Change a value, then save.".into(),
        });
    }
    // A gross limit per payment above the gross daily limit can never be reached. The total
    // limit counts only irreversible expense, so a deposit may be larger than it.
    if let (Some(per_payment), Some(per_day)) = (
        params.get("payment_limit").and_then(int),
        params.get("daily_volume_limit").and_then(int),
    ) && per_payment > per_day
        && changed
            .iter()
            .any(|(f, _)| f.key == "payment_limit" || f.key == "daily_volume_limit")
    {
        return Err(format!(
            "{} ({} {symbol}) is more than {} ({} {symbol}). Lower {} or raise {}.",
            label("payment_limit"),
            decimal(per_payment, draft.asset.decimals),
            label("daily_volume_limit"),
            decimal(per_day, draft.asset.decimals),
            label("payment_limit"),
            label("daily_volume_limit"),
            symbol = draft.asset.symbol,
        ));
    }
    let mut next = draft.clone();
    for (field, kind) in &changed {
        if kind == "duration" {
            // The edit is an absolute time, as a revision replays an expiry.
            next.kinds.insert(field.key.clone(), "time".into());
        }
    }
    next.unresolved
        .retain(|k| params.get(k).and_then(list).is_some_and(Vec::is_empty));
    next.params = params;
    let changes = changed
        .into_iter()
        .map(|(field, kind)| change(draft, field, &kind, &next.params[&field.key]))
        .collect();
    Ok((next, changes))
}

/// The stored value of the owner's `input` for `field`, or `None` when it equals the current
/// value. `reads` are the source's typed reads of the param.
fn edited(
    draft: &Parameters,
    field: &Field,
    kind: &str,
    input: &str,
    reads: Option<&Vec<ParamRead>>,
    now: i64,
) -> Result<Option<Value>, String> {
    if input.len() > MAX_VALUE {
        return Err(format!("{}: the value is too long.", field.label));
    }
    if field.input.is_none() {
        return Err(format!(
            "{} cannot be changed here. Describe the change in the chat.",
            field.label
        ));
    }
    let value = parse(kind, &field.label, input, &draft.asset, now)?;
    // A canonical limit of 0 blocks every payment. A custom amount or count, such as a minimum,
    // may be 0.
    if blocks_all_at_zero(&field.key) && int(&value) == Some(0) {
        return Err(if kind == "count" {
            format!(
                "{}: enter 1 or more. A count of 0 would block every payment.",
                field.label
            )
        } else {
            format!(
                "{}: enter an amount above 0 {}. A limit of 0 would block every payment.",
                field.label, draft.asset.symbol
            )
        });
    }
    if value == draft.params[&field.key] {
        return Ok(None);
    }
    // The policy reads the value with the types that its source names, so it must fit them.
    if let (Some(n), Some((low, high))) = (int(&value), bounded_range(&field.key, reads))
        && (n < low || n > high)
    {
        return Err(if kind == "amount" {
            format!(
                "{}: enter at most {} {}.",
                field.label,
                decimal(high, draft.asset.decimals),
                draft.asset.symbol
            )
        } else if matches!(kind, "time" | "duration") {
            let at = |seconds: i128| {
                i64::try_from(seconds)
                    .ok()
                    .and_then(|s| DateTime::from_timestamp(s, 0))
                    .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true))
            };
            match (at(low), at(high)) {
                (Some(low), Some(high)) => {
                    format!("{}: enter a time from {low} to {high}.", field.label)
                }
                _ => format!("{}: this time does not fit the policy.", field.label),
            }
        } else {
            format!("{}: enter a number from {low} to {high}.", field.label)
        });
    }
    Ok(Some(value))
}

/// The change of `field` to the stored `value`, as a revision request names it.
fn change(draft: &Parameters, field: &Field, kind: &str, value: &Value) -> Change {
    // A revision replays the request, so a time keeps its seconds there.
    let after = match render(kind, value, &draft.asset) {
        (Some(input), _) if matches!(kind, "time" | "duration") => input,
        (_, shown) => shown,
    };
    Change {
        key: field.key.clone(),
        noun: field.label.to_lowercase(),
        before: field.shown.clone(),
        after,
    }
}

/// Typed parameter entries with human units. Every expiry is an absolute time.
pub fn entries(draft: &Parameters) -> Vec<Value> {
    let asset = &draft.asset;
    draft
        .params
        .iter()
        .map(|(key, value)| {
            let kind = kind(draft, key);
            match kind.as_str() {
                "amount" => int(value).map(|n| {
                    json!({"key":key,"kind":"amount","amount":decimal(n, asset.decimals),"asset":asset.symbol})
                }),
                "count" => int(value).map(|n| {
                    let count = u64::try_from(n).map_or_else(|_| json!(n.to_string()), |count| json!(count));
                    json!({"key":key,"kind":"count","count":count})
                }),
                "int" => int(value).map(|n| json!({"key":key,"kind":"int","int":n.to_string()})),
                "addresses" => addresses(value).map(|a| json!({"key":key,"kind":"addresses","addresses":a})),
                "assets" => addresses(value).map(|items| json!({"key":key,"kind":"assets","assets":asset_names(&items, asset)})),
                "time" | "duration" => int(value)
                    .and_then(|at| i64::try_from(at).ok())
                    .and_then(|at| DateTime::from_timestamp(at, 0))
                    .map(|at| json!({"key":key,"kind":"time","time":at.to_rfc3339_opts(SecondsFormat::Secs, true)})),
                "operations" => match (value.get("Str").and_then(Value::as_str), list(value)) {
                    (Some("*"), _) => Some(json!({"key":key,"kind":"operations","operations":["*"]})),
                    (_, Some(items)) => Some(json!({"key":key,"kind":"operations","operations":items.iter().filter_map(|i| i.get("Str")).collect::<Vec<_>>()})),
                    _ => None,
                },
                "bool" => value.get("Bool").map(|b| json!({"key":key,"kind":"bool","bool":b})),
                "text" => value.get("Str").map(|t| json!({"key":key,"kind":"text","text":t})),
                _ => None,
            }
            .unwrap_or_else(|| json!({"key":key,"kind":kind}))
        })
        .collect()
}

pub fn base_units(amount: &str, decimals: u32) -> std::result::Result<i128, String> {
    // A comma is a decimal separator in many locales, so "1,5" is ambiguous; never guess.
    if amount.contains(',') {
        return Err(format!(
            "\"{amount}\" contains a comma; write a plain decimal with a point and no separators, such as 1500.5"
        ));
    }
    let amount = amount.trim().replace('_', "");
    let (whole, fraction) = amount.split_once('.').unwrap_or((&amount, ""));
    if whole.is_empty() && fraction.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || amount.ends_with('.')
    {
        return Err(format!("\"{amount}\" is not a nonnegative decimal amount"));
    }
    let fraction = fraction.trim_end_matches('0');
    if fraction.len() > decimals as usize {
        return Err(format!(
            "\"{amount}\" has more than {decimals} decimal places"
        ));
    }
    let scale = 10i128
        .checked_pow(decimals)
        .ok_or("the asset has too many decimals")?;
    let whole: i128 = if whole.is_empty() {
        0
    } else {
        whole
            .parse()
            .map_err(|_| format!("\"{amount}\" is too large"))?
    };
    let fraction: i128 = if fraction.is_empty() {
        0
    } else {
        format!("{fraction:0<width$}", width = decimals as usize)
            .parse()
            .map_err(|_| format!("\"{amount}\" is invalid"))?
    };
    whole
        .checked_mul(scale)
        .and_then(|units| units.checked_add(fraction))
        .ok_or_else(|| format!("\"{amount}\" is too large"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_types_and_ranges_are_the_core_registry_contract() {
        let mut p = Parameters {
            params: serde_json::Map::new(),
            kinds: BTreeMap::new(),
            asset: Asset {
                symbol: "TEST".into(),
                address: "opaque".into(),
                decimals: 3,
            },
            unresolved: vec![],
        };
        for f in allowit_core::policy::canonical_function_registry()
            .unwrap()
            .functions
        {
            let expected = if f.parameter.semantic_type == "timestamp" {
                "time"
            } else {
                &f.parameter.semantic_type
            };
            assert_eq!(kind(&p, &f.name), expected);
            if let Some(range) = f.parameter.integer_range {
                assert_eq!(
                    bounded_range(&f.name, None),
                    Some((range.min.parse().unwrap(), range.max.parse().unwrap()))
                );
            }
        }
        p.params.insert("custom_minimum".into(), json!({"Int":"1"}));
        assert_eq!(kind(&p, "custom_minimum"), "int");
        assert!(!blocks_all_at_zero("custom_minimum"));
        let reads = vec![ParamRead::Typed("u32".into())];
        assert_eq!(
            bounded_range("custom_minimum", Some(&reads)),
            Some((0, u32::MAX.into()))
        );
        assert!(blocks_all_at_zero(allowit::CALL_LIMIT));
        assert!(!blocks_all_at_zero(allowit::EXPIRES_AT));
    }

    fn draft(params: Value) -> Parameters {
        Parameters {
            params: params.as_object().unwrap().clone(),
            kinds: BTreeMap::new(),
            asset: Asset {
                symbol: "USDC".into(),
                address: "CUSDC".into(),
                decimals: 7,
            },
            unresolved: vec![],
        }
    }

    #[test]
    fn owner_views_show_core_labels_and_every_token() {
        let draft = draft(json!({
            "call_limit": {"Int": "3"},
            "daily_spending_limit": {"Int": "50000000"},
            "tokens": {"List": [{"Addr": "CUSDC"}, {"Addr": "CTOKENB"}]},
        }));
        let code = "pub fn execute(ctx: &AllowitContext) -> allowit::Result {\n    allowit::daily_spending_limit(ctx, &ctx.params[allowit::DAILY_SPENDING_LIMIT])?;\n    allowit::call_limit(ctx, &ctx.params[allowit::CALL_LIMIT])?;\n    allowit::tokens(ctx, &ctx.params[allowit::TOKENS])?;\n    Ok(())\n}\n";
        assert_eq!(
            lines(&draft, code),
            "Daily spending: 5 USDC\nOperation count: 3\nAssets: USDC, CTOKENB"
        );
        assert!(
            entries(&draft)
                .contains(&json!({"key":"tokens","kind":"assets","assets":["USDC","CTOKENB"]}))
        );
    }

    #[test]
    fn integer_params_are_not_editable_when_the_source_analysis_fails() {
        let draft = draft(json!({"max_count": {"Int": "5"}}));
        let code = format!(
            "pub fn execute(ctx: &AllowitContext) -> allowit::Result {{\n    let max: u32 = allowit::param(ctx, \"max_count\");\n    let deep = {}0{};\n    Ok(())\n}}\n",
            "(".repeat(33),
            ")".repeat(33)
        );
        assert!(param_reads(&code).is_err());
        assert_eq!(fields(&draft, &code)[0].input, None);
        assert!(set(&draft, &code, "max_count", "5000000000", 0).is_err());
    }
}
