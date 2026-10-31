//! `allowit curl ARGS...` (hidden alias `wget`): curl request syntax, sent and
//! paid by the engine. No local HTTP tool runs.
use crate::{
    ctx::Ctx,
    error::Result,
    gateway::{
        args::{self, parse},
        http::http_request,
        operation::purchase,
    },
};
use serde_json::json;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit curl https://api.example/v1/search -X POST -d '{\"q\":\"rust\"}' --max-payment 0.10
  allowit curl https://api.example/v1/data -o result.json";

/// Send a paid HTTP request with curl syntax (-X, -H, -d, --data-raw, -o)
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// URL and curl options
    #[arg(
        required = true,
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "ARGS"
    )]
    pub args: Vec<String>,
}

pub(crate) fn run(a: Args, cx: &mut Ctx) -> Result<i32> {
    cx.vendor()?.gateway("curl")?;
    let mut argv = vec!["curl".to_owned()];
    argv.extend(a.args);
    let mut inv = parse(&argv)?;
    inv.common.json |= cx.out.json;
    let request = http_request(&mut inv)?;
    let mut q = args::context(&inv.common);
    q["request"] = json!(request);
    purchase(q, &inv.common, &mut cx.out.stdout, &mut cx.out.stderr)
}
