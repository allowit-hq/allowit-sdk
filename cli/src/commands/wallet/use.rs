//! `allowit wallet use ADDR`.
use crate::{
    ctx::Ctx,
    error::{Error, Result},
    vendors::address,
};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit wallet use CWALLET…
  allowit --rail solana:devnet wallet use VAULT…";

/// Make an address the current rail's default wallet
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// The wallet address
    pub address: String,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let rail = cx.rail()?;
    if let Some(family) = address::infer_family(&args.address)
        && family != rail.family
    {
        return Err(Error::usage(format!(
            "{} is a {} address, but the rail is {}",
            args.address,
            family.name(),
            rail.id
        )));
    }
    address::check_wallet(rail.family, &args.address)?;
    let settings = cx.settings()?;
    let r = settings.rail_mut(rail.id);
    r.wallet = Some(args.address.clone());
    if !r.wallets.contains(&args.address) {
        r.wallets.push(args.address.clone());
    }
    settings.save()?;
    cx.out
        .value(&serde_json::json!({"rail": rail.id, "wallet": args.address}));
    Ok(0)
}
