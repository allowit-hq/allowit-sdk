//! `allowit status ID`: reads one request without creating another.
use crate::{
    ctx::Ctx,
    error::{Error, Result},
    request::request_id_valid,
    routing,
    settings::journal,
    vendors,
};
use std::time::Duration;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit status pay-001
  allowit status pay-001 --wait 60s
  allowit status pay-001 --wallet CWALLET… --json";

/// Read a request's state. The local journal supplies its rail and wallet;
/// --wallet is the fallback
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// The request ID that exec printed
    pub id: String,
    /// Wait up to DUR for a final state
    #[arg(long, value_name = "DUR", default_value = "0", value_parser = super::parse_wait)]
    pub wait: Duration,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    if !request_id_valid(&args.id) {
        return Err(Error::usage(
            "ID is the request ID that exec or eval printed",
        ));
    }
    cx.out.request_id = Some(args.id.clone());
    let dir = cx.dir()?;
    let entry = journal::load(&dir, &args.id)?;
    let rail = match &entry {
        Some(e) => {
            let recorded = routing::info(&e.rail)?;
            if let Some(flag) = &cx.global.rail
                && routing::normalize(flag)? != recorded.id
            {
                return Err(Error::usage(format!(
                    "request {} belongs to {}",
                    args.id, recorded.id
                )));
            }
            recorded
        }
        None => cx.rail()?,
    };
    let wallet = match cx.global.wallet.clone() {
        Some(w) => Some(w),
        None => match entry.as_ref().and_then(|e| e.wallet.clone()) {
            Some(w) => Some(w),
            None => cx.wallet(rail)?,
        },
    };
    if let Some(w) = &wallet {
        vendors::address::check_wallet(rail.family, w)?;
    }
    let vendor = vendors::rail(rail);
    let code = routing::with_network(Some(rail.id.into()), || {
        vendor.status(cx, &args.id, wallet.as_deref(), args.wait)
    })?;
    if entry.is_some() {
        journal::finish(&dir, &args.id, code)?;
    }
    Ok(code)
}
