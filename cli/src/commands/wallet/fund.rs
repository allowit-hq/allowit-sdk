//! `allowit wallet fund AMOUNT [--from KEY]`.
use crate::{commands::SigningArgs, ctx::Ctx, error::Result, vendors::asset::Use};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit wallet fund XLM:200 --from owner
  allowit --rail solana:devnet wallet fund USDC:20 --sign-with-key owner";

/// Add funds to the wallet
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// The amount: CODE:DECIMAL, CODE:ISSUER:DECIMAL or INTEGER base units
    pub amount: String,
    /// The paying key
    #[arg(long, value_name = "KEY")]
    pub from: Option<String>,
    /// Operation ID; a retry reuses it
    #[arg(long, value_name = "ID")]
    pub request_id: Option<String>,
    /// Send an additional operation after an uncertain one; needs a new --request-id
    #[arg(long, requires = "request_id")]
    pub additional: bool,
    #[command(flatten)]
    pub signing: SigningArgs,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let rail = cx.rail()?;
    let vendor = cx.vendor()?;
    let amount = vendor.parse_amount(&args.amount, Use::Spend)?;
    let from = match &args.from {
        Some(name) => {
            let key = cx.key(name)?;
            key.check_rail(rail.id)?;
            Some(key)
        }
        None => None,
    };
    let mut signing = args.signing.resolve(cx, rail.id)?;
    signing.request_id = super::request_id(args.request_id)?;
    signing.additional = args.additional;
    vendor.wallet_fund(cx, &amount, from.as_ref(), &signing)
}
