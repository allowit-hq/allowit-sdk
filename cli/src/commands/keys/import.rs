//! `allowit keys import NAME FILE|-`.
use crate::{
    ctx::{Ctx, read_input},
    error::{Error, Result},
    settings::keys,
};
use std::path::PathBuf;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit keys import agent agent.key
  allowit --rail solana:devnet keys import executor executor.json
  allowit --rail tempo:testnet keys import executor executor.json
  pass show allowit/agent | allowit keys import agent -";

/// Import a private key in the rail's signer format, or an owner-issued
/// executor bundle (Solana executor JSON, Tempo bundle)
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Key name
    pub name: String,
    /// The key or bundle file; - reads stdin
    pub file: PathBuf,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let rail = cx.rail()?;
    let dir = cx.dir()?;
    keys::valid_name(&args.name)?;
    let bytes = zeroize::Zeroizing::new(read_input(&args.file)?);
    let bundle = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .filter(serde_json::Value::is_object);
    if let Some(bundle) = bundle {
        let vendor = cx.vendor()?;
        if args.file.as_os_str() == "-" {
            // The vendor importers read a file; keep a private copy briefly.
            crate::settings::private_dir(&dir)?;
            let copy = dir.join(format!(".import-{}.json", std::process::id()));
            crate::settings::write_atomic(&copy, &bytes, 0o600)?;
            let result = vendor.keys_import(cx, &args.name, &copy, &bundle);
            let _ = std::fs::remove_file(&copy);
            return result;
        }
        return vendor.keys_import(cx, &args.name, &args.file, &bundle);
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| Error::usage("the key file is not text"))?
        .trim();
    let key = keys::import_secret(&dir, &args.name, rail.id, text.as_bytes())?;
    cx.out.value(&super::view(&key));
    Ok(0)
}
