//! `allowit wallet use ADDR`.
use crate::{ctx::Ctx, error::Result, vendors::address};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit wallet use WALLET";

/// Make an address the current rail's default wallet
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// The wallet address
    pub address: String,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let rail = cx.rail()?;
    address::check_wallet(rail.family, &args.address)?;
    cx.settings()?.update(|config| {
        let r = config.rail_mut(rail.id);
        r.wallet = Some(args.address.clone());
        if !r.wallets.contains(&args.address) {
            r.wallets.push(args.address.clone());
        }
        Ok(())
    })?;
    cx.out
        .value(&serde_json::json!({"rail": rail.id, "wallet": args.address}));
    Ok(0)
}
