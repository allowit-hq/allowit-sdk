//! Owner key custody and terminal bindings for shared server-backed policy generation.
use crate::{
    ctx::Ctx,
    error::{Error, Result},
};
use allowit_sdk_engine::stellar::owner_client::{
    GenerateRequest, OwnerClient, OwnerSigner, Target,
};
use allowit_stellar_wallet as native;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

pub(crate) const EXAMPLES: &str = "Examples:\n  allowit --rail stellar:testnet policy generate --owner-key owner --origin http://localhost:5173 --profile allowit-stellar-policy-wallet-v1 --new-wallet 'Pay only my approved recipient, up to 1 XLM per payment'";

/// Generate a portable draft through the owner-authenticated server; does not deploy or spend
#[derive(clap::Args, Debug)]
#[command(group(clap::ArgGroup::new("generation_target").args(["new_wallet","installation"]).required(true)))]
pub(crate) struct Args {
    /// Complete owner instructions, preserved exactly
    pub prompt: String,
    /// Named local owner key; independent of the selected agent key
    #[arg(long, value_name = "NAME")]
    pub owner_key: String,
    /// Configured sign-in Origin, which can differ from ALLOWIT_URL
    #[arg(long, env = "ALLOWIT_OWNER_ORIGIN", hide_env_values = true)]
    pub origin: String,
    /// Exact wallet profile advertised by the server
    #[arg(long)]
    pub profile: String,
    /// Explicitly create a draft from an advertised contract-wallet template
    #[arg(long)]
    pub new_wallet: bool,
    /// Existing owner installation ID (64 lowercase hexadecimal characters)
    #[arg(long, value_name = "ID")]
    pub installation: Option<String>,
}
struct LocalOwner {
    seed: Zeroizing<[u8; 32]>,
    account: String,
}
impl OwnerSigner for LocalOwner {
    fn account(&self) -> &str {
        &self.account
    }
    fn sign_hash(&self, digest: &[u8; 32]) -> native::Result<[u8; 64]> {
        use ed25519_dalek::{Signer, SigningKey};
        Ok(SigningKey::from_bytes(&self.seed).sign(digest).to_bytes())
    }
}
fn clock() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .map_err(|_| Error::config("System clock precedes the Unix epoch"))
}
pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    if cx.global.wallet_flag {
        return Err(Error::usage(
            "Generation selects --new-wallet or --installation; omit --wallet",
        ));
    }
    let rail = cx.rail()?;
    let request = GenerateRequest {
        prompt: args.prompt,
        network: rail.id.into(),
        profile: args.profile,
        target: if args.new_wallet {
            Target::NewWallet
        } else {
            Target::Installation(
                args.installation
                    .ok_or_else(|| Error::usage("Select --new-wallet or --installation"))?,
            )
        },
    };
    request.validate()?;
    let key = cx.key(&args.owner_key)?;
    key.check_rail(rail.id)?;
    let seed = crate::wallet::stellar_signing_seed(&cx.signer(&key)?)?;
    let account = native::account_for_seed(&seed);
    if account != key.address {
        return Err(Error::config(
            "The owner key differs from its public metadata",
        ));
    }
    let signer = LocalOwner { seed, account };
    let mut client = OwnerClient::new(
        &crate::config::env_value("ALLOWIT_URL"),
        &args.origin,
        &signer.account,
    )?;
    client.authenticate(&signer, clock()?)?;
    let policy = client.generate(&request, clock()?)?;
    let code = if policy
        .runtime
        .instance_binding
        .portable
        .unresolved
        .is_empty()
    {
        0
    } else {
        11
    };
    cx.out.value(
        &serde_json::to_value(policy)
            .map_err(|_| Error::config("Cannot encode generated policy"))?,
    );
    Ok(code)
}
