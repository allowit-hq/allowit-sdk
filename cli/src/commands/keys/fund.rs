//! `allowit keys fund NAME`.
use crate::{ctx::Ctx, error::Result};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit --rail stellar:testnet keys fund agent
  allowit --rail solana:devnet keys fund agent";

/// Fund a key's account from the rail's test faucet (Friendbot on Stellar
/// testnet, an RPC airdrop on Solana devnet and testnet)
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Key name
    pub name: String,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let key = cx.key(&args.name)?;
    let vendor = cx.vendor()?;
    vendor.keys_fund(cx, &key)
}
