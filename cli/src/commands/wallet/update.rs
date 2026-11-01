//! `allowit wallet update`.
use crate::{
    commands::SigningArgs,
    ctx::Ctx,
    error::{Error, Result},
    settings::keys,
    vendors::WalletUpdate,
};

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum Switch {
    On,
    Off,
}

/// Replace the owner or the agent, or switch x402 payments
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// The new owner: a key name or an address
    #[arg(long, value_name = "KEY|ADDR")]
    pub owner: Option<String>,
    /// The new agent key
    #[arg(long, value_name = "KEY")]
    pub agent: Option<String>,
    /// Let the agent pay x402 resources from the wallet
    #[arg(long, value_enum)]
    pub x402: Option<Switch>,
    #[command(flatten)]
    pub signing: SigningArgs,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    if args.owner.is_none() && args.agent.is_none() && args.x402.is_none() {
        return Err(Error::usage("give --owner, --agent or --x402"));
    }
    let rail = cx.rail()?;
    let owner = match &args.owner {
        Some(v) => {
            let dir = cx.dir()?;
            Some(keys::resolve_address(&dir, v, rail.id)?)
        }
        None => None,
    };
    let agent = match &args.agent {
        Some(name) => {
            let key = cx.key(name)?;
            key.check_rail(rail.id)?;
            Some(key)
        }
        None => None,
    };
    let signing = args.signing.resolve(cx, rail.id)?;
    let update = WalletUpdate {
        owner,
        agent,
        x402: args.x402.map(|s| matches!(s, Switch::On)),
        signing,
    };
    let vendor = cx.vendor()?;
    vendor.wallet_update(cx, &update)
}
