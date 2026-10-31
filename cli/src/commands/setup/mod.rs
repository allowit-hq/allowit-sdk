//! `allowit setup --agent NAME [--force]`: onboarding for the current rail.
//! Chooses the agent key and the rail's paid-service asset (USDC; AlphaUSD on
//! Tempo testnet, which has no USDC) and, with `ALLOWIT_ACCOUNT_TOKEN`, registers the agent's public key with
//! the engine. Running it again changes nothing.
use crate::{
    config::env_value,
    ctx::Ctx,
    error::{Error, Result},
    gateway::{ACCOUNT_TOKEN_ENV, Common, args::Invocation, control},
};
use serde_json::{Value, json};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit keys generate agent --fund
  allowit setup --agent agent
  allowit setup --agent new-agent --force";

/// Set up the current rail: the agent key and the paid-service asset
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// The agent key, which signs agent requests
    #[arg(long, value_name = "NAME")]
    pub agent: String,
    /// Replace a different agent key
    #[arg(long)]
    pub force: bool,
}

const FIELDS: &[&str] = &["network", "address", "publicKey", "keyType", "name"];

/// Accepts only public wallet metadata that matches its own public key.
fn registration(input: &Value) -> Result<()> {
    control::public_fields(input, FIELDS)?;
    let mut registration = input.clone();
    if let Some(map) = registration.as_object_mut() {
        map.remove("name");
    }
    let wallet: crate::wallet::PublicWallet = serde_json::from_value(registration)
        .map_err(|_| Error::usage("registration needs network, address, publicKey and keyType"))?;
    wallet.validate()?;
    if crate::routing::selected().is_some_and(|network| network != wallet.network) {
        return Err(Error::usage(
            "the registration conflicts with the selected rail",
        ));
    }
    Ok(())
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let rail = cx.rail()?;
    let default_asset = cx.vendor()?.caps().service_asset;
    let key = cx.key(&args.agent)?;
    key.check_rail(rail.id)?;
    if !key.secret {
        return Err(Error::config(format!(
            "key {} has no private key on this device; the agent must sign",
            key.name
        )));
    }
    let settings = cx.settings()?;
    let current = settings.rail(rail.id);
    if let Some(agent) = &current.agent
        && agent != &key.name
        && !args.force
    {
        return Err(Error::config(format!(
            "{} already uses agent key {agent}; pass --force to replace it",
            rail.id
        )));
    }
    let service_asset = current
        .service_asset
        .clone()
        .unwrap_or_else(|| default_asset.into());
    let changed = current.agent.as_deref() != Some(key.name.as_str())
        || current.service_asset.as_deref() != Some(service_asset.as_str());
    if changed {
        let r = settings.rail_mut(rail.id);
        r.agent = Some(key.name.clone());
        r.service_asset = Some(service_asset.clone());
        settings.save()?;
    }
    let mut result = json!({
        "rail": rail.id,
        "agent": key.name,
        "address": key.address,
        "serviceAsset": service_asset,
        "registered": false,
    });
    if !env_value(ACCOUNT_TOKEN_ENV).is_empty() {
        let public_key = key
            .public_key
            .clone()
            .ok_or_else(|| Error::config("the agent key has no public key to register"))?;
        let input = json!({
            "network": rail.id,
            "address": key.address,
            "publicKey": public_key,
            "keyType": key.curve,
            "name": key.name,
        });
        registration(&input)?;
        let inv = Invocation {
            group: "setup".into(),
            args: vec![],
            common: Common {
                json: true,
                ..Common::default()
            },
        };
        let mut reply = String::new();
        control::mutate(
            &inv,
            "/api/gateway/setup",
            "setup",
            input,
            &mut reply,
            &mut cx.out.stderr,
        )?;
        result["registered"] = json!(true);
        result["registration"] = serde_json::from_str(&reply).unwrap_or(Value::Null);
    }
    cx.out.value(&result);
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn public_registration() -> Value {
        let key = ed25519_dalek::SigningKey::from_bytes(&[7; 32])
            .verifying_key()
            .to_bytes();
        let public = bs58::encode(key).into_string();
        json!({
            "network": "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
            "address": public,
            "publicKey": public,
            "keyType": "ed25519"
        })
    }
    #[test]
    fn registration_accepts_only_matching_public_metadata() {
        let public = public_registration();
        assert!(registration(&public).is_ok());
        let mut wrong = public.clone();
        wrong["address"] = json!("11111111111111111111111111111111");
        assert!(registration(&wrong).is_err());
        for field in ["seed", "privateKey", "keypair", "walletFile", "config"] {
            let mut secret = public.clone();
            secret[field] = json!({"value": "private material"});
            assert!(registration(&secret).is_err());
        }
        assert!(registration(&json!({})).is_err());
        crate::routing::with_network(Some("eip155:42431".into()), || {
            assert!(registration(&public_registration()).is_err());
            Ok(())
        })
        .unwrap();
    }
}
