//! Terminal bindings for the selected SDK capability.
//!
#[cfg(feature = "stellar")]
#[path = "stellar/approval.rs"]
pub mod approval;
#[cfg(feature = "stellar")]
#[path = "stellar/owner.rs"]
pub mod owner;
#[cfg(all(test, feature = "stellar"))]
#[path = "stellar/request.rs"]
mod request;

use super::{
    Binding, Caps, ExecRequest, Rail,
    asset::{Amount, Token},
    require_op,
};
use crate::{config::env_value, ctx::Ctx, error::Result, routing::RailInfo, settings::keys::Key};
use std::time::Duration;

pub(crate) struct Adapter(pub &'static RailInfo);

pub(crate) fn tokens_for(rail: &str) -> &'static [Token] {
    allowit_sdk_engine::execution::tokens(rail).expect("admitted rail")
}

impl Adapter {
    fn profile(
        &self,
        cx: &mut Ctx,
        wallet: Option<&str>,
    ) -> Result<allowit_sdk_engine::profiles::Profile> {
        let root = cx.dir()?;
        let selected = match wallet {
            Some(w) => Some(w.into()),
            None => cx.wallet(self.0)?,
        };
        let agent = if cx.settings()?.rail(self.0.id).agent.is_some() {
            Some(cx.agent(self.0)?.address)
        } else {
            None
        };
        allowit_sdk_engine::profiles::Cache::open(&root)?.resolve(
            &allowit_sdk_engine::profiles::Selector {
                network: self.0.id.into(),
                wallet: selected,
                agent,
                ..Default::default()
            },
        )
    }
    fn signer(&self, cx: &mut Ctx) -> Result<allowit_sdk_engine::wallet::LocalSigner> {
        let agent = cx.agent(self.0)?;
        allowit_sdk_engine::wallet::local_signer(self.0.id, &cx.signer(&agent)?)
    }
}

impl Rail for Adapter {
    fn info(&self) -> &'static RailInfo {
        self.0
    }
    fn tokens(&self) -> &'static [Token] {
        tokens_for(self.0.id)
    }
    fn caps(&self) -> Caps {
        let c =
            allowit_sdk_engine::execution::capabilities(self.0.id).expect("registered capability");
        Caps {
            curve: c.curve,
            address_is_key: c.address_is_key,
            ops: c.operations,
            exec_build_only: c.build_only,
            verbs: c.verbs,
        }
    }
    fn keys_fund(&self, cx: &mut Ctx, key: &Key) -> Result<i32> {
        if !self.0.testnet {
            return Err(self.na("keys fund"));
        }
        key.check_rail(self.0.id)?;
        let endpoint = env_value("ALLOWIT_FRIENDBOT_URL");
        let hash =
            allowit_sdk_engine::execution::fund_key(self.0.id, &key.address, Some(&endpoint))?;
        cx.out.value(&serde_json::json!({
            "key": key.name,
            "address": key.address,
            "rail": self.0.id,
            "transactionHash": hash,
        }));
        Ok(0)
    }
    fn exec_binding(&self, cx: &mut Ctx, wallet: Option<&str>) -> Result<Binding> {
        let p = self.profile(cx, wallet)?;
        Ok(Binding {
            policy: Some(format!(
                "{}@{}",
                p.description().policy_id,
                p.description().module_digest
            )),
            wallet: Some(p.description().wallet.clone()),
        })
    }
    fn status_binding(&self, cx: &mut Ctx, wallet: Option<&str>, id: &str) -> Result<Binding> {
        let profile = self.profile(cx, wallet)?;
        let b = allowit_sdk_engine::execution::status_binding(&profile, &cx.dir()?, id)?;
        Ok(Binding {
            wallet: Some(b.wallet),
            policy: Some(b.policy),
        })
    }
    #[cfg(feature = "stellar")]
    fn legacy(&self, cx: &mut Ctx, verb: &str, args: &[String]) -> Result<i32> {
        match verb {
            "wallet configure" => owner::run(args, &mut cx.out.stdout),
            "wallet approve" => approval::run(args, &mut cx.out.stdout),
            _ => Err(self.na(verb)),
        }
    }
    fn wallet_show(&self, cx: &mut Ctx, wallet: Option<&str>) -> Result<i32> {
        let p = self.profile(cx, wallet)?;
        cx.out.value(&p.inspect()?);
        Ok(0)
    }
    fn policy_show(&self, cx: &mut Ctx, source: bool) -> Result<i32> {
        let p = self.profile(cx, None)?;
        if source {
            cx.out.source(&p.description().source);
        } else {
            let mut out = p.inspect()?;
            out["asset"] = out["assetSymbol"].clone();
            cx.out.value(&out);
        }
        Ok(0)
    }
    fn check_exec(
        &self,
        op: &str,
        to: &str,
        token_out: Option<&Amount>,
        args: Option<&serde_json::Value>,
    ) -> Result<()> {
        allowit_sdk_engine::execution::check(self.0.id, op, to, token_out, args.is_some())
    }
    fn exec(&self, cx: &mut Ctx, r: &ExecRequest) -> Result<i32> {
        require_op(self, &r.op)?;
        allowit_sdk_engine::execution::check(
            self.0.id,
            &r.op,
            &r.to,
            r.token_out.as_ref(),
            r.args.is_some(),
        )?;
        let p = self.profile(cx, r.wallet.as_deref())?;
        let root = cx.dir()?;
        let rpc = cx.rpc_url(self.0)?;
        if r.build_only {
            cx.out.json_value(&allowit_sdk_engine::execution::build(
                &p,
                &root,
                rpc.as_deref(),
                r,
            )?);
            return Ok(0);
        }
        let signer = self.signer(cx)?;
        crate::flush_stderr(&mut cx.out.stderr);
        let out = allowit_sdk_engine::execution::execute(p, signer, &root, rpc, r)?;
        cx.out.value(&out.document);
        Ok(out.exit_code)
    }
    fn status(&self, cx: &mut Ctx, id: &str, wallet: Option<&str>, wait: Duration) -> Result<i32> {
        let p = self.profile(cx, wallet)?;
        let signer = self.signer(cx)?;
        let root = cx.dir()?;
        let rpc = cx.rpc_url(self.0)?;
        let out = allowit_sdk_engine::execution::status(p, signer, &root, rpc, id, wait)?;
        cx.out.value(&out.document);
        Ok(out.exit_code)
    }
}

#[cfg(all(test, feature = "stellar"))]
mod tests {
    #[test]
    fn token_issuers_are_valid_strkeys() {
        for tokens in [
            super::tokens_for("stellar:testnet"),
            super::tokens_for("stellar:pubnet"),
        ] {
            for token in tokens {
                if let Some(issuer) = token.issuer {
                    allowit_stellar_wallet::account_key(issuer).unwrap();
                }
            }
        }
    }
}
