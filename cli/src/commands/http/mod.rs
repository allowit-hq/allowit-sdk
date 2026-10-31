//! `allowit http ARGS...` (hidden alias `fetch`): HTTPie request syntax, sent
//! and paid by the engine. No local HTTP tool runs.
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
  allowit http POST https://api.example/v1/translate text=hello lang=fr --max-amount 0.02
  allowit http https://api.example/v1/weather city=Paris";

/// Send a paid HTTP request with HTTPie syntax (METHOD URL NAME=VALUE...)
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// METHOD, URL and NAME=VALUE fields
    #[arg(
        required = true,
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "ARGS"
    )]
    pub args: Vec<String>,
}

pub(crate) fn run(a: Args, cx: &mut Ctx) -> Result<i32> {
    cx.vendor()?.gateway("http")?;
    let mut argv = vec!["http".to_owned()];
    argv.extend(a.args);
    let mut inv = parse(&argv)?;
    inv.common.json |= cx.out.json;
    let request = http_request(&mut inv)?;
    let mut q = args::context(&inv.common);
    q["request"] = json!(request);
    purchase(q, &inv.common, &mut cx.out.stdout, &mut cx.out.stderr)
}
