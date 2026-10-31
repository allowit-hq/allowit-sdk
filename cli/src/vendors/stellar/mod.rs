//! Stellar adapter over `allowit-stellar-wallet`.
//!
//! - `keys fund`: Friendbot on testnet.
//! - `exec --op transfer`, `status`: the installed wallet profile's engine
//!   endpoints, signed by the rail's agent key (`setup --agent`). The public
//!   wallet profile comes from `ALLOWIT_STELLAR_BUNDLE` until the wallet is
//!   resolved from chain state.
//! - `wallet show`, `policy show`: the public wallet profile.
//! - Hidden `wallet configure` and `wallet approve`: the classic-account owner
//!   links of earlier releases.
pub mod approval;
pub mod owner;
pub(crate) mod request;
pub(crate) mod run;

use super::{Caps, ExecRequest, Rail, asset::Token, require_op};
use crate::{
    config::env_value,
    ctx::Ctx,
    error::{Error, Result},
    routing::RailInfo,
    settings::keys::Key,
};
use std::time::Duration;

pub(crate) struct Stellar(pub &'static RailInfo);

const TESTNET: &[Token] = &[
    Token {
        code: "XLM",
        decimals: 7,
        native: true,
        issuer: None,
    },
    Token {
        code: "USDC",
        decimals: 7,
        native: false,
        issuer: Some("GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5"),
    },
];
const PUBNET: &[Token] = &[
    Token {
        code: "XLM",
        decimals: 7,
        native: true,
        issuer: None,
    },
    Token {
        code: "USDC",
        decimals: 7,
        native: false,
        issuer: Some("GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN"),
    },
];

pub(crate) fn tokens_for(rail: &str) -> &'static [Token] {
    if rail == "stellar:testnet" {
        TESTNET
    } else {
        PUBNET
    }
}

const FRIENDBOT: &str = "https://friendbot.stellar.org";

impl Stellar {
    /// The public wallet profile, checked against the selected wallet.
    fn profile(&self, cx: &mut Ctx, wallet: Option<&str>) -> Result<run::Bundle> {
        let path = env_value("ALLOWIT_STELLAR_BUNDLE");
        if path.is_empty() {
            return Err(Error::config(format!(
                "no wallet profile for {}; set ALLOWIT_STELLAR_BUNDLE to the public wallet profile file",
                self.0.id
            )));
        }
        let bundle = run::read_public_bundle(std::path::Path::new(&path))?;
        if crate::routing::normalize(&bundle.network)? != self.0.id {
            return Err(Error::config(format!(
                "the wallet profile belongs to {}, not {}",
                bundle.network, self.0.id
            )));
        }
        let selected = match wallet {
            Some(w) => Some(w.to_owned()),
            None => cx.wallet(self.0)?,
        };
        if let Some(w) = selected
            && w != bundle.wallet
        {
            return Err(Error::config(format!(
                "the wallet profile is for {}, not {w}",
                bundle.wallet
            )));
        }
        Ok(bundle)
    }
    fn client(&self, cx: &mut Ctx, bundle: run::Bundle) -> Result<run::Client> {
        let agent = cx.agent(self.0)?;
        let signer = cx.signer(&agent)?;
        let rpc = cx.rpc_url(self.0)?;
        run::Client::load(bundle, &signer, rpc)
    }
    fn journal(&self, cx: &mut Ctx) -> Result<std::path::PathBuf> {
        Ok(cx.dir()?.join("stellar-journal"))
    }
}

impl Rail for Stellar {
    fn info(&self) -> &'static RailInfo {
        self.0
    }
    fn tokens(&self) -> &'static [Token] {
        tokens_for(self.0.id)
    }
    fn caps(&self) -> Caps {
        Caps {
            curve: "ed25519",
            ops: &["transfer"],
            param_keys: super::PARAM_KEYS,
            signing: &[],
            verbs: if self.0.testnet {
                &["keys fund", "wallet show", "policy show", "exec", "status"]
            } else {
                &["wallet show", "policy show", "exec", "status"]
            },
        }
    }
    fn keys_fund(&self, cx: &mut Ctx, key: &Key) -> Result<i32> {
        if !self.0.testnet {
            return Err(self.na("keys fund"));
        }
        key.check_rail(self.0.id)?;
        let base = match env_value("ALLOWIT_FRIENDBOT_URL") {
            v if v.is_empty() => FRIENDBOT.to_owned(),
            v => v,
        };
        let mut url =
            url::Url::parse(&base).map_err(|_| Error::config("the Friendbot URL is invalid"))?;
        url.query_pairs_mut().append_pair("addr", &key.address);
        let response = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .and_then(|c| c.get(url).send())
            .map_err(|_| {
                Error::uncertain("Friendbot did not answer; check the balance before retrying")
            })?;
        let status = response.status().as_u16();
        let body: serde_json::Value = response.json().unwrap_or_default();
        if !(200..300).contains(&status) {
            let detail = body["detail"].as_str().unwrap_or("request refused");
            return Err(Error::config(format!(
                "Friendbot refused to fund {} (HTTP {status}): {detail}",
                key.address
            )));
        }
        cx.out.value(&serde_json::json!({
            "key": key.name,
            "address": key.address,
            "rail": self.0.id,
            "transactionHash": body["hash"],
        }));
        Ok(0)
    }
    fn wallet_show(&self, cx: &mut Ctx, wallet: Option<&str>) -> Result<i32> {
        let bundle = self.profile(cx, wallet)?;
        let mut value = run::public_description(&bundle, false);
        value["rail"] = serde_json::json!(self.0.id);
        cx.out.value(&value);
        Ok(0)
    }
    fn policy_show(&self, cx: &mut Ctx, source: bool) -> Result<i32> {
        let bundle = self.profile(cx, None)?;
        if source {
            cx.out.source(&bundle.config.policy_source);
        } else {
            let d = run::public_description(&bundle, false);
            cx.out.value(&serde_json::json!({
                "policyId": d["policyId"],
                "wallet": d["wallet"],
                "sourceHash": d["sourceHash"],
                "irHash": d["irHash"],
                "moduleDigest": d["moduleDigest"],
                "asset": d["assetSymbol"],
            }));
        }
        Ok(0)
    }
    fn exec(&self, cx: &mut Ctx, r: &ExecRequest) -> Result<i32> {
        require_op(self, &r.op)?;
        if r.token_out.is_some() || r.args.is_some() {
            return Err(Error::usage(
                "--token-out and --args apply to swap and custom operations, not transfer",
            ));
        }
        super::address::check_recipient(self.0.family, &r.to)?;
        let bundle = self.profile(cx, r.wallet.as_deref())?;
        let units = request::installed_units(&bundle.config, &r.amount)?;
        let typed = request::transfer(
            &bundle.policy_id,
            &r.request_id,
            &bundle.config,
            &r.to,
            units,
        )?;
        if r.build_only {
            let wire = typed.wire()?;
            cx.out.value(&wire);
            return Ok(0);
        }
        let journal = self.journal(cx)?;
        let client = self.client(cx, bundle)?;
        let (value, code) = run::exec(&client, &journal, typed, r.wait, &mut cx.out.stderr)?;
        cx.out.value(&value);
        Ok(code)
    }
    fn status(&self, cx: &mut Ctx, id: &str, wallet: Option<&str>, wait: Duration) -> Result<i32> {
        let bundle = self.profile(cx, wallet)?;
        let journal = self.journal(cx)?;
        let client = self.client(cx, bundle)?;
        let (value, code) = run::status(&client, &journal, id, wait)?;
        cx.out.value(&value);
        Ok(code)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn token_issuers_are_valid_strkeys() {
        for tokens in [super::TESTNET, super::PUBNET] {
            for token in tokens {
                if let Some(issuer) = token.issuer {
                    allowit_stellar_wallet::account_key(issuer).unwrap();
                }
            }
        }
    }
}
