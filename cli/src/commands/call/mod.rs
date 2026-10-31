//! `allowit call SERVICE|URL`: one paid service call. On Tempo testnet a URL
//! is paid through MPP from the vault; otherwise the engine quotes and calls.
use crate::{
    ctx::Ctx,
    error::{Error, Result},
    gateway::{
        Common, Money,
        args::{self, Invocation},
        operation::purchase,
        target::call_target,
    },
    request::request_id_valid,
    vendors::CallRequest,
};
use serde_json::json;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit --rail tempo:testnet call https://api.example/v1/data --max-amount 0.05";

/// Call a paid service by catalog ID or URL
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// A catalog service ID or an HTTP(S) URL
    pub target: String,
    /// JSON input for the service
    #[arg(long, value_name = "JSON")]
    pub input: Option<String>,
    /// The most this call may cost: CODE:DECIMAL, or a decimal of the paying asset
    #[arg(long, value_name = "AMOUNT")]
    pub max_amount: Option<String>,
    /// Operation ID; a retry reuses it
    #[arg(long, value_name = "ID")]
    pub request_id: Option<String>,
    #[arg(long, hide = true)]
    pub sandbox: bool,
    #[arg(long, hide = true, value_name = "VERSION@ADDR")]
    pub policy: Option<String>,
    #[arg(long, hide = true, value_name = "NAME")]
    pub account: Option<String>,
}

/// `CODE:DECIMAL` or a plain decimal (USDC) as engine money.
fn money(text: &str) -> Result<Money> {
    let (currency, amount) = text.split_once(':').unwrap_or(("USDC", text));
    crate::vendors::asset::decimal_units(amount, 18)?;
    Ok(Money {
        currency: currency.into(),
        amount: amount.into(),
    })
}

pub(crate) fn run(a: Args, cx: &mut Ctx) -> Result<i32> {
    if let Some(id) = &a.request_id
        && !request_id_valid(id)
    {
        return Err(Error::usage(
            "--request-id must be 8 to 100 characters from A-Z a-z 0-9 . _ : -",
        ));
    }
    let input = a
        .input
        .as_deref()
        .map(|raw| {
            serde_json::from_str::<serde_json::Value>(raw)
                .map_err(|_| Error::usage("--input must be valid JSON"))
        })
        .transpose()?;
    let vendor = cx.vendor()?;
    let request = CallRequest {
        target: a.target.clone(),
        input: input.clone(),
        max_amount: a.max_amount.clone(),
        request_id: a.request_id.clone(),
    };
    if let Some(code) = vendor.call(cx, &request)? {
        return Ok(code);
    }
    vendor.gateway(if a.target.contains("://") {
        "call URL"
    } else {
        "call SERVICE"
    })?;
    if let Some(policy) = &a.policy {
        args::validate_selector(policy)?;
    }
    let common = Common {
        policy_selector: a.policy,
        sandbox: a.sandbox,
        account: a.account,
        operation_id: a.request_id,
        max_debit: a.max_amount.as_deref().map(money).transpose()?,
        json: cx.out.json,
        ..Common::default()
    };
    let mut argv = vec![a.target];
    if let Some(raw) = a.input {
        argv.extend(["--input".into(), raw]);
    }
    let inv = Invocation {
        group: "call".into(),
        args: argv,
        common,
    };
    let (service, request, input) = call_target(&inv)?;
    let mut q = args::context(&inv.common);
    if let Some(s) = service {
        q["serviceId"] = json!(s);
    }
    if let Some(r) = request {
        q["request"] = json!(r);
    }
    if let Some(i) = input {
        q["input"] = i;
    }
    purchase(q, &inv.common, &mut cx.out.stdout, &mut cx.out.stderr)
}
