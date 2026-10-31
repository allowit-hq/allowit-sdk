//! `allowit wallet withdraw AMOUNT [--to ADDR]`.
use crate::{commands::SigningArgs, ctx::Ctx, error::Result, settings::keys, vendors::asset::Use};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit wallet withdraw XLM:max --to owner --sign-with-key owner
  allowit --rail solana:devnet wallet withdraw USDC:5 --sign-with-key owner";

/// Move funds from the wallet to the owner
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// The amount, or CODE:max for the whole balance
    pub amount: String,
    /// The receiving key or address [default: the owner]
    #[arg(long, value_name = "KEY|ADDR")]
    pub to: Option<String>,
    #[command(flatten)]
    pub signing: SigningArgs,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let rail = cx.rail()?;
    let vendor = cx.vendor()?;
    let amount = vendor.parse_amount(&args.amount, Use::Withdraw)?;
    let to = match &args.to {
        Some(v) => {
            let dir = cx.dir()?;
            Some(keys::resolve_address(&dir, v, rail.id)?)
        }
        None => None,
    };
    let signing = args.signing.resolve(cx, rail.id)?;
    vendor.wallet_withdraw(cx, &amount, to.as_deref(), &signing)
}
