//! `allowit wallet create`.
use crate::{
    commands::SigningArgs,
    ctx::Ctx,
    error::{Error, Result},
    settings::keys,
    vendors::{WalletCreate, WalletKind},
};
use std::path::PathBuf;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit wallet create --kind common --asset XLM --policy policy.rs --owner owner --agent agent
  allowit wallet create --kind simple --asset USDC --policy policy.rs --owner GOWNER… --agent agent --x402
  allowit wallet create --kind common --asset XLM --policy policy.rs --owner owner --agent agent --sign-with-key owner
  allowit --rail solana:devnet wallet create --asset USDC --policy policy.json --owner owner --agent agent --fund USDC:10";

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum Kind {
    Simple,
    Common,
    Custom,
}

/// Create a policy wallet with a policy, an owner and an agent
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Wallet kind (Stellar): simple, common or custom
    #[arg(long, value_enum)]
    pub kind: Option<Kind>,
    /// The wallet asset [default: the asset that setup chose, USDC]
    #[arg(long, value_name = "ASSET")]
    pub asset: Option<String>,
    /// The policy source file
    #[arg(long, value_name = "FILE")]
    pub policy: PathBuf,
    /// The owner: a key name or an address
    #[arg(long, value_name = "KEY|ADDR")]
    pub owner: String,
    /// The agent key
    #[arg(long, value_name = "KEY")]
    pub agent: String,
    /// Let the agent pay x402 resources from the wallet
    #[arg(long)]
    pub x402: bool,
    /// The initial deposit, for a vault that takes it at creation (Solana)
    #[arg(long, value_name = "AMOUNT")]
    pub fund: Option<String>,
    /// Creation request ID [default: derived from the inputs, so a retry
    /// reuses it]. A new ID replaces an expired unsigned preparation
    #[arg(long, value_name = "ID")]
    pub request_id: Option<String>,
    #[command(flatten)]
    pub signing: SigningArgs,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let rail = cx.rail()?;
    let dir = cx.dir()?;
    let owner = keys::resolve_address(&dir, &args.owner, rail.id)?;
    let agent = cx.key(&args.agent)?;
    agent.check_rail(rail.id)?;
    let asset = match args.asset {
        Some(a) => a,
        None => cx.settings()?.rail(rail.id).service_asset.ok_or_else(|| {
            Error::usage("give --asset, or run allowit setup to choose the service asset")
        })?,
    };
    let mut signing = args.signing.resolve(cx, rail.id)?;
    signing.request_id = super::request_id(args.request_id)?;
    let vendor = cx.vendor()?;
    let fund = args
        .fund
        .as_deref()
        .map(|a| vendor.parse_amount(a, crate::vendors::asset::Use::Spend))
        .transpose()?;
    let request = WalletCreate {
        kind: args.kind.map(|k| match k {
            Kind::Simple => WalletKind::Simple,
            Kind::Common => WalletKind::Common,
            Kind::Custom => WalletKind::Custom,
        }),
        asset,
        fund,
        policy: args.policy,
        owner,
        agent,
        x402: args.x402,
        signing,
    };
    vendor.wallet_create(cx, &request)
}
