//! `allowit policy params set KEY=VALUE...`.
use crate::{
    commands::SigningArgs,
    ctx::Ctx,
    error::{Error, Result, quoted},
    exit::not_available,
    request::request_id_valid,
    vendors::{PARAM_KEYS, Param, Rail, address, asset::Use, param_request_id},
};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit --rail solana:devnet policy params set day_spent_cap=USDC:50 --sign-with-key owner
  allowit --rail solana:devnet policy params set transaction_cap=USDC:10 day_spent_cap=USDC:100 --sign-with-key owner
  allowit --rail solana:devnet policy params set day_spent_cap=USDC:50 --sign-with-key owner --request-id caps-0001";

/// Change parameters. Keys: transaction_cap, spent_cap, day_spent_cap,
/// day_total_cap (amounts), call_cap (a count), recipients (addresses),
/// tokens (assets), expiry (RFC 3339 time or duration), operations
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// KEY=VALUE pairs; a list value replaces the old list
    #[arg(required = true, value_name = "KEY=VALUE")]
    pub params: Vec<String>,
    /// Operation ID (ID.KEY per change); a retry reuses it [default: a new ID]
    #[arg(long, value_name = "ID")]
    pub request_id: Option<String>,
    #[command(flatten)]
    pub signing: SigningArgs,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let rail = cx.rail()?;
    let vendor = cx.vendor()?;
    let mut params = vec![];
    for pair in &args.params {
        let param = parse(vendor.as_ref(), pair)?;
        if params.iter().any(|p: &Param| p.key == param.key) {
            return Err(Error::usage(format!("{} is given twice", param.key)));
        }
        params.push(param);
    }
    for p in &params {
        if !vendor.caps().param_keys.contains(&p.key.as_str()) {
            return Err(not_available(
                &format!("policy params set {}", p.key),
                rail.id,
            ));
        }
    }
    if let Some(id) = &args.request_id
        && (!request_id_valid(id)
            || !params
                .iter()
                .all(|p| request_id_valid(&param_request_id(id, &p.key))))
    {
        return Err(Error::usage(
            "--request-id must be 8 characters or more from A-Z a-z 0-9 . _ : -, and ID.KEY at most 100",
        ));
    }
    let mut signing = args.signing.resolve(cx, rail.id)?;
    signing.request_id = args.request_id;
    vendor.policy_params_set(cx, &params, &signing)
}

/// Parses and checks one `KEY=VALUE` against its kind.
pub(crate) fn parse(rail: &dyn Rail, pair: &str) -> Result<Param> {
    let (key, value) = pair
        .split_once('=')
        .ok_or_else(|| Error::usage(format!("{} is not KEY=VALUE", quoted(pair))))?;
    if !PARAM_KEYS.contains(&key) {
        return Err(Error::usage(format!(
            "unknown parameter {}; keys are {}",
            quoted(key),
            PARAM_KEYS.join(", ")
        )));
    }
    let family = rail.info().family;
    let list = || {
        value
            .split(',')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
    };
    let mut amount = None;
    match key {
        "transaction_cap" | "spent_cap" | "day_spent_cap" | "day_total_cap" => {
            // The caps are nonnegative; only a transaction cap must be positive.
            let parsed = rail.parse_amount(value, Use::Limit)?;
            if key == "transaction_cap" && parsed.units() == Some(0) {
                return Err(Error::usage("transaction_cap must be positive"));
            }
            amount = Some(parsed);
        }
        "call_cap" => {
            value
                .parse::<u64>()
                .map_err(|_| Error::usage("call_cap is a whole number of calls"))?;
        }
        "recipients" => {
            if list().is_empty() {
                return Err(Error::usage("recipients needs at least one address"));
            }
            for a in list() {
                address::check_recipient(family, a)?;
            }
        }
        "tokens" => {
            if list().is_empty() {
                return Err(Error::usage("tokens needs at least one asset"));
            }
            for t in list() {
                rail.parse_amount(&format!("{t}:0"), Use::Minimum)?;
            }
        }
        "expiry" => expiry(value)?,
        _ => {
            for op in list() {
                crate::commands::exec::check_op(op)?;
            }
        }
    }
    Ok(Param {
        key: key.into(),
        value: value.into(),
        amount,
    })
}

/// An RFC 3339 time, or a duration such as 30d, 12h or 2w.
fn expiry(value: &str) -> Result<()> {
    use time::format_description::well_known::Rfc3339;
    if time::OffsetDateTime::parse(value, &Rfc3339).is_ok() {
        return Ok(());
    }
    let Some((number, unit)) = value
        .is_ascii()
        .then(|| value.split_at(value.len().saturating_sub(1)))
    else {
        return Err(Error::usage(
            "expiry is an RFC 3339 time or a duration such as 30d",
        ));
    };
    if !number.is_empty()
        && number.bytes().all(|b| b.is_ascii_digit())
        && matches!(unit, "s" | "m" | "h" | "d" | "w")
    {
        return Ok(());
    }
    Err(Error::usage(
        "expiry is an RFC 3339 time or a duration such as 30d",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn values_follow_their_kind() {
        let rail = crate::vendors::rail(crate::routing::info("stellar:testnet").unwrap());
        let r = rail.as_ref();
        assert_eq!(
            parse(r, "transaction_cap=XLM:10")
                .unwrap()
                .amount
                .unwrap()
                .units(),
            Some(100_000_000)
        );
        parse(r, "call_cap=20").unwrap();
        parse(r, "call_cap=5000000000").unwrap();
        parse(r, "day_total_cap=XLM:0").unwrap();
        parse(r, "spent_cap=0").unwrap();
        parse(r, "expiry=30d").unwrap();
        parse(r, "expiry=2026-12-31T00:00:00Z").unwrap();
        parse(r, "operations=transfer,swap").unwrap();
        parse(r, "tokens=XLM,USDC").unwrap();
        parse(
            r,
            "recipients=GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5",
        )
        .unwrap();
        for bad in [
            "daily_limit=XLM:5",
            "transaction_cap",
            "call_cap=-1",
            "call_cap=18446744073709551616",
            "transaction_cap=XLM:0",
            "transaction_cap=0",
            "expiry=soon",
            "expiry=é",
            "expiry=1é",
            "expiry=日d",
            "recipients=nope",
            "operations=steal",
            "tokens=",
        ] {
            assert_eq!(parse(r, bad).unwrap_err().code, 2, "{bad}");
        }
    }
}
