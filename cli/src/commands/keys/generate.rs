//! `allowit keys generate NAME [--fund]`.
use crate::{ctx::Ctx, error::Result, settings::keys};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit keys generate agent
  allowit keys generate owner --fund
  allowit --rail tempo:testnet keys generate executor";

/// Generate a key with the rail's curve: ed25519 on Stellar and Solana,
/// secp256k1 on Tempo
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Key name
    pub name: String,
    /// Fund the new account from the rail's test faucet
    #[arg(long)]
    pub fund: bool,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let rail = cx.rail()?;
    let dir = cx.dir()?;
    let key = keys::generate(&dir, &args.name, rail.id)?;
    if args.fund {
        let vendor = cx.vendor()?;
        if cx.out.json {
            // One JSON document: the key, then the faucet result.
            let mut funded = crate::term::Out::new(true, true, false, true);
            std::mem::swap(&mut cx.out, &mut funded);
            let result = vendor.keys_fund(cx, &key);
            std::mem::swap(&mut cx.out, &mut funded);
            let code = result?;
            let mut value = super::view(&key);
            value["fund"] = serde_json::from_str(&funded.stdout).unwrap_or_default();
            cx.out.value(&value);
            return Ok(code);
        }
        cx.out.value(&super::view(&key));
        cx.out.line("");
        return vendor.keys_fund(cx, &key);
    }
    cx.out.value(&super::view(&key));
    Ok(0)
}
