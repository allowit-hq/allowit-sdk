//! Tempo adapter over `allowit-tempo`, testnet only.
//!
//! - `keys import`: the owner-exported executor bundle.
//! - `exec --op transfer`: one vault payment signed by the executor key;
//!   `--build-only` prints the authority-signed wallet plan and sends nothing.
//! - `status`, `wallet show`, `policy show`: the bundle and its engine.
//! - `call URL`: one MPP-paid HTTPS GET.
//!
//! The bundle is `ALLOWIT_TEMPO_BUNDLE` (default `.allowit-tempo/executor.json`)
//! and must belong to the selected rail. The hidden `tempo` group keeps the
//! commands of earlier releases.
use super::{
    CallRequest, Caps, ExecRequest, Mode, Rail,
    asset::{Amount, Asset, Token},
    require_op,
};
use crate::{
    ctx::Ctx,
    error::{Error, Result},
    routing::{RailInfo, normalize, selected},
    settings::keys,
};
use std::{path::Path, time::Duration};

pub(crate) struct Tempo(pub &'static RailInfo);

const TOKENS: &[Token] = &[
    Token {
        code: "AlphaUSD",
        decimals: 6,
        native: false,
        issuer: Some("0x20c0000000000000000000000000000000000001"),
    },
    Token {
        code: "pathUSD",
        decimals: 6,
        native: false,
        issuer: Some("0x20c0000000000000000000000000000000000000"),
    },
];
const DECIMALS: u8 = 6;

fn bundle_path() -> String {
    match crate::config::env_value("ALLOWIT_TEMPO_BUNDLE") {
        v if v.is_empty() => ".allowit-tempo/executor.json".into(),
        v => v,
    }
}

/// Runs a Tempo crate command and maps its errors.
fn run(
    args: Vec<String>,
    id: Option<&str>,
    stdout: &mut String,
    stderr: &mut String,
) -> Result<i32> {
    allowit_tempo::run_with_request_id(&args, id, stdout, stderr)
}

impl Tempo {
    fn testnet_only(&self, verb: &str) -> Result<()> {
        if self.0.testnet {
            Ok(())
        } else {
            Err(self.na(verb))
        }
    }
    /// The public policy of the bundle, checked against the rail and wallet.
    fn describe(&self, wallet: Option<&str>) -> Result<serde_json::Value> {
        let policy = allowit_tempo::describe(Path::new(&bundle_path()))?;
        let network = policy["network"]
            .as_str()
            .ok_or_else(|| Error::config("the Tempo bundle names no network"))?;
        if normalize(network).ok() != Some(self.0.id) {
            return Err(Error::config(format!(
                "the Tempo bundle belongs to {network}, not {}",
                self.0.id
            )));
        }
        if let Some(w) = wallet
            && !policy["vault"]
                .as_str()
                .is_some_and(|v| v.eq_ignore_ascii_case(w))
        {
            return Err(Error::config(format!(
                "the Tempo bundle is for vault {}, not {w}",
                policy["vault"].as_str().unwrap_or("?")
            )));
        }
        Ok(policy)
    }
    fn decimal(&self, amount: &Amount) -> Result<String> {
        match &amount.asset {
            Asset::Installed | Asset::Token { .. } => {}
            _ => return Err(Error::usage("Tempo vaults hold one TIP-20 USD token")),
        }
        let units = amount
            .units()
            .ok_or_else(|| Error::usage("an exact amount is required"))?;
        Ok(super::asset::format_units(units, DECIMALS))
    }
    fn json_flag(cx: &Ctx, mut args: Vec<String>) -> Vec<String> {
        if cx.out.json {
            args.push("--json".into());
        }
        args
    }
}

impl Rail for Tempo {
    fn info(&self) -> &'static RailInfo {
        self.0
    }
    fn tokens(&self) -> &'static [Token] {
        TOKENS
    }
    fn caps(&self) -> Caps {
        Caps {
            curve: "secp256k1",
            ops: &["transfer"],
            param_keys: &[],
            signing: &[],
            verbs: if self.0.testnet {
                &[
                    "keys import",
                    "wallet show",
                    "policy show",
                    "exec",
                    "status",
                    "call",
                ]
            } else {
                &[]
            },
        }
    }
    fn keys_import(
        &self,
        cx: &mut Ctx,
        name: &str,
        file: &Path,
        bundle: &serde_json::Value,
    ) -> Result<i32> {
        self.testnet_only("keys import")?;
        let network = bundle["policy"]["network"].as_str().unwrap_or_default();
        if normalize(network).ok() != Some(self.0.id) {
            return Err(Error::config(format!(
                "the Tempo bundle belongs to {network}, not {}",
                self.0.id
            )));
        }
        let executor = bundle["policy"]["executor"]
            .as_str()
            .ok_or_else(|| Error::usage("the Tempo bundle names no executor"))?
            .to_owned();
        let dir = cx.dir()?;
        keys::valid_name(name)?;
        if keys::exists(&dir, name) {
            return Err(Error::config(format!("key {name} already exists")));
        }
        let args = vec!["import".into(), file.to_string_lossy().into_owned()];
        let out = &mut cx.out;
        let code = run(args, None, &mut out.stdout, &mut out.stderr)?;
        keys::add(&dir, name, self.0.id, &executor)?;
        Ok(code)
    }
    fn wallet_show(&self, cx: &mut Ctx, wallet: Option<&str>) -> Result<i32> {
        self.testnet_only("wallet show")?;
        let selected = match wallet {
            Some(w) => Some(w.to_owned()),
            None => cx.wallet(self.0)?,
        };
        let policy = self.describe(selected.as_deref())?;
        cx.out.value(&policy);
        Ok(0)
    }
    fn policy_show(&self, cx: &mut Ctx, source: bool) -> Result<i32> {
        self.testnet_only("policy show")?;
        if source {
            return Err(self.na("policy show --source"));
        }
        let policy = self.describe(None)?;
        cx.out.value(&policy);
        Ok(0)
    }
    fn exec(&self, cx: &mut Ctx, r: &ExecRequest) -> Result<i32> {
        self.testnet_only("exec")?;
        require_op(self, &r.op)?;
        if r.mode == Mode::Eval {
            return Err(self.na("eval"));
        }
        if r.token_out.is_some() || r.args.is_some() {
            return Err(Error::usage(
                "--token-out and --args apply to swap and custom operations, not transfer",
            ));
        }
        super::address::check_recipient(self.0.family, &r.to)?;
        self.describe(r.wallet.as_deref())?;
        let amount = self.decimal(&r.amount)?;
        let verb = if r.build_only { "execute" } else { "pay" };
        let args = Self::json_flag(cx, vec![verb.into(), r.to.clone(), amount]);
        let out = &mut cx.out;
        run(args, Some(&r.request_id), &mut out.stdout, &mut out.stderr)
    }
    fn status(&self, cx: &mut Ctx, id: &str, wallet: Option<&str>, _: Duration) -> Result<i32> {
        self.testnet_only("status")?;
        self.describe(wallet)?;
        let args = Self::json_flag(cx, vec!["status".into(), id.into()]);
        let out = &mut cx.out;
        run(args, None, &mut out.stdout, &mut out.stderr)
    }
    fn call(&self, cx: &mut Ctx, c: &CallRequest) -> Result<Option<i32>> {
        if !c.target.starts_with("https://") || !self.0.testnet {
            return Ok(None);
        }
        if c.input.is_some() {
            return Err(Error::usage("an MPP call is one GET without --input"));
        }
        self.describe(None)?;
        let max = c
            .max_amount
            .as_deref()
            .ok_or_else(|| Error::usage("an MPP call needs --max-amount"))?;
        let max = self.parse_amount(max, super::asset::Use::Spend)?;
        let units = self.decimal(&max)?;
        let units = super::asset::decimal_units(&units, DECIMALS)?;
        let exact = format!("{}.{:06}", units / 1_000_000, units % 1_000_000);
        let id = match &c.request_id {
            Some(id) => id.clone(),
            None => {
                let id = crate::settings::journal::new_id()?;
                cx.out.progress(&id);
                id
            }
        };
        let args = Self::json_flag(
            cx,
            vec![
                "call".into(),
                c.target.clone(),
                "--id".into(),
                id,
                "--max-amount".into(),
                exact,
            ],
        );
        let out = &mut cx.out;
        run(args, None, &mut out.stdout, &mut out.stderr).map(Some)
    }
}

/// The hidden `tempo` group of earlier releases. With a selected rail, the
/// bundle must belong to it.
pub(crate) fn legacy(args: &[String], stdout: &mut String, stderr: &mut String) -> Result<i32> {
    if let Some(selected) = selected()
        && !args
            .first()
            .is_some_and(|s| matches!(s.as_str(), "help" | "-h" | "--help"))
    {
        let path = if args.first().is_some_and(|s| s == "import") {
            args.iter()
                .skip(1)
                .find(|a| *a != "--json")
                .cloned()
                .ok_or_else(|| Error::usage("tempo import needs a bundle path"))?
        } else {
            bundle_path()
        };
        let policy = allowit_tempo::describe(Path::new(&path))?;
        let network = policy["network"]
            .as_str()
            .ok_or_else(|| Error::config("the Tempo bundle names no network"))?;
        if normalize(network).ok() != Some(selected.as_str()) {
            return Err(Error::config(
                "the selected rail differs from the Tempo wallet deployment",
            ));
        }
    }
    allowit_tempo::run(args, stdout, stderr)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tokens_are_tip20_addresses_with_six_decimals() {
        for t in TOKENS {
            super::super::address::check_token(crate::routing::Family::Tempo, t.issuer.unwrap())
                .unwrap();
            assert_eq!(t.decimals, 6);
        }
        let rail = Tempo(crate::routing::info("tempo:testnet").unwrap());
        let a = rail
            .parse_amount("AlphaUSD:1.5", super::super::asset::Use::Spend)
            .unwrap();
        assert_eq!(rail.decimal(&a).unwrap(), "1.5");
        let base = rail
            .parse_amount("1500000", super::super::asset::Use::Spend)
            .unwrap();
        assert_eq!(rail.decimal(&base).unwrap(), "1.5");
    }
}
