//! `allowit exec`: one agent request, signed by the agent key and checked by
//! the wallet's policy on chain.
use crate::{
    ctx::Ctx,
    error::{Error, Result, quoted},
    request::request_id_valid,
    settings::journal,
    vendors::{ExecRequest, Mode, asset::Use},
};
use std::time::Duration;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit exec --op transfer --to GDEST… --amount XLM:5
  allowit exec --op transfer --to GDEST… --amount XLM:5 --request-id pay-0001 --wait 2m
  allowit exec --op transfer --to GDEST… --amount 50000000 --build-only";

/// The request flags shared by `exec` and `eval`.
#[derive(clap::Args, Debug)]
pub(crate) struct RequestArgs {
    /// transfer, swap, deposit, redeem, custom, custom.FN or custom:C….FN
    #[arg(long, value_name = "OP")]
    pub op: String,
    /// The recipient address
    #[arg(long, value_name = "ADDR")]
    pub to: String,
    /// CODE:DECIMAL, CODE:ISSUER:DECIMAL, C…:DECIMAL or INTEGER base units
    #[arg(long, value_name = "AMOUNT")]
    pub amount: String,
    /// The asset and minimum to receive, for swap: ASSET:MIN
    #[arg(long, value_name = "ASSET:MIN")]
    pub token_out: Option<String>,
    /// Call arguments as a JSON object, for custom operations
    #[arg(long, value_name = "JSON")]
    pub args: Option<String>,
    /// Request ID; a retry reuses it [default: a new ID, printed on stderr]
    #[arg(long, value_name = "ID")]
    pub request_id: Option<String>,
    /// Wait up to DUR for a final result
    #[arg(long, value_name = "DUR", default_value = "60s", value_parser = super::parse_wait)]
    pub wait: Duration,
}

/// Submit an agent request: the policy checks it and the rail settles it
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    #[command(flatten)]
    pub request: RequestArgs,
    /// Print the unsigned request as JSON and never sign or submit it; a signer outside the CLI submits it
    #[arg(long)]
    pub build_only: bool,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    submit(Mode::Exec, args.request, args.build_only, cx)
}

/// Checks the `--op` syntax. Which operations work depends on the rail.
pub(crate) fn check_op(op: &str) -> Result<()> {
    let ident = |s: &str| {
        !s.is_empty()
            && s.len() <= 64
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && !s.starts_with(|c: char| c.is_ascii_digit())
    };
    let ok = matches!(op, "transfer" | "swap" | "deposit" | "redeem" | "custom")
        || op.strip_prefix("custom.").is_some_and(ident)
        || op.strip_prefix("custom:").is_some_and(|rest| {
            rest.rsplit_once('.')
                .is_some_and(|(contract, function)| !contract.is_empty() && ident(function))
        });
    if ok {
        Ok(())
    } else {
        Err(Error::usage(format!(
            "unknown operation {}; use transfer, swap, deposit, redeem, custom, custom.FN or custom:C.FN",
            quoted(op)
        )))
    }
}

/// Builds, journals and sends one `exec` or `eval` request.
pub(crate) fn submit(mode: Mode, a: RequestArgs, build_only: bool, cx: &mut Ctx) -> Result<i32> {
    check_op(&a.op)?;
    let rail = cx.rail()?;
    let vendor = cx.vendor()?;
    // Refuse what the rail cannot serve before a request ID is spent.
    if !vendor.caps().verbs.contains(&mode.name()) {
        return Err(vendor.na(mode.name()));
    }
    crate::vendors::require_op(vendor.as_ref(), &a.op)?;
    // Input checks come before any settings, journal or network access.
    crate::vendors::address::check_recipient(rail.family, &a.to)?;
    let amount = vendor.parse_amount(&a.amount, Use::Spend)?;
    let token_out = a
        .token_out
        .as_deref()
        .map(|t| vendor.parse_amount(t, Use::Minimum))
        .transpose()?;
    let call_args = match &a.args {
        Some(raw) => {
            let value: serde_json::Value = serde_json::from_str(raw)
                .map_err(|_| Error::usage("--args must be a JSON object"))?;
            if !value.is_object() {
                return Err(Error::usage("--args must be a JSON object"));
            }
            Some(value)
        }
        None => None,
    };
    if a.request_id
        .as_deref()
        .is_some_and(|id| !request_id_valid(id))
    {
        return Err(Error::usage(
            "--request-id must be 8 to 100 characters from A-Z a-z 0-9 . _ : -",
        ));
    }
    let selected = cx.wallet(rail)?;
    let binding = vendor.exec_binding(cx, selected.as_deref())?;
    let request_id = match a.request_id {
        Some(id) => id,
        None => {
            let id = journal::new_id()?;
            cx.out.generated_id(&id);
            id
        }
    };
    cx.out.request_id = Some(request_id.clone());
    let request = ExecRequest {
        mode,
        op: a.op,
        to: a.to,
        amount,
        token_out,
        args: call_args,
        request_id: request_id.clone(),
        wait: a.wait,
        build_only,
        wallet: binding.wallet.clone(),
    };
    cx.out.detail(format!(
        "wallet  {}",
        request.wallet.as_deref().unwrap_or("-")
    ));
    let dir = cx.dir()?;
    if !build_only {
        journal::record(
            &dir,
            &journal::Entry {
                request_id: request_id.clone(),
                command: mode.name().into(),
                rail: rail.id.into(),
                wallet: binding.wallet.clone(),
                policy: binding.policy.clone(),
                op: request.op.clone(),
                to: request.to.clone(),
                amount: request.amount.text.clone(),
                created_at: journal::now(),
                exit: None,
            },
        )?;
    }
    let result = match mode {
        Mode::Exec => vendor.exec(cx, &request),
        Mode::Eval => vendor.eval(cx, &request),
    };
    if !build_only {
        let code = match &result {
            Ok(code) => *code,
            Err(e) => e.code,
        };
        journal::finish(&dir, &request_id, code)?;
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn op_syntax() {
        for ok in [
            "transfer",
            "swap",
            "custom",
            "custom.deposit_all",
            "custom:CABC.run",
        ] {
            super::check_op(ok).unwrap();
        }
        for bad in [
            "",
            "Transfer",
            "custom.",
            "custom:.f",
            "custom:C",
            "custom.1x",
            "pay",
        ] {
            assert_eq!(super::check_op(bad).unwrap_err().code, 2, "{bad}");
        }
    }
}
