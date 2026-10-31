//! Tempo adapter over `allowit-tempo`, testnet only.
//!
//! - `keys import`: the owner-exported executor bundle.
//! - `exec --op transfer`: one vault payment signed by the executor key;
//!   `--build-only` prints the authority-signed wallet plan and sends nothing.
//! - `status`, `wallet show`, `policy show`: the bundle and its engine.
//! - `call URL`: one MPP-paid HTTPS GET. `--max-amount` takes a decimal of
//!   the vault token or the vault token's `CODE:DECIMAL`.
//!
//! Every amount must name the bundle's installed token; another token exits 4.
//! The bundle is `ALLOWIT_TEMPO_BUNDLE` (default `.allowit-tempo/executor.json`)
//! and must belong to the selected rail. The hidden `tempo` group keeps the
//! commands of earlier releases.
use super::{
    Binding, CallRequest, Caps, ExecRequest, Mode, Rail,
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
    /// The public policy of the bundle, checked against the selected wallet.
    fn describe_selected(&self, cx: &mut Ctx) -> Result<serde_json::Value> {
        let wallet = cx.wallet(self.0)?;
        self.describe(wallet.as_deref())
    }
    /// A vault amount as a decimal. The amount must name the token that the
    /// bundle's vault holds.
    fn decimal(&self, amount: &Amount, policy: &serde_json::Value) -> Result<String> {
        let installed = policy["token"].as_str().unwrap_or_default();
        let ok = match &amount.asset {
            Asset::Installed => true,
            Asset::Token {
                issuer: Some(token),
                ..
            } => !installed.is_empty() && token.eq_ignore_ascii_case(installed),
            _ => false,
        };
        if !ok {
            return Err(Error {
                code: 4,
                message: format!(
                    "the vault holds only its token {installed}; {} names another asset",
                    amount.text
                ),
                status: None,
                config: false,
            });
        }
        let units = amount
            .units()
            .ok_or_else(|| Error::usage("an exact amount is required"))?;
        Ok(super::asset::format_units(units, DECIMALS))
    }
    /// A paid-call maximum: a decimal of the vault token, or an amount.
    fn max_amount(&self, text: &str) -> Result<Amount> {
        if text.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
            let units = super::asset::decimal_units(text, DECIMALS)?;
            if units == 0 {
                return Err(Error::usage("--max-amount must be positive"));
            }
            return Ok(Amount {
                asset: Asset::Installed,
                quantity: super::asset::Quantity::Units(units),
                decimals: Some(DECIMALS),
                text: text.into(),
            });
        }
        self.parse_amount(text, super::asset::Use::Spend)
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
            address_is_key: false,
            // Tempo testnet has no USDC; AlphaUSD is its TIP-20 USD token.
            service_asset: "AlphaUSD",
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
    fn legacy(&self, cx: &mut Ctx, verb: &str, args: &[String]) -> Result<i32> {
        if verb != "tempo" {
            return Err(self.na(verb));
        }
        let out = &mut cx.out;
        legacy(args, &mut out.stdout, &mut out.stderr)
    }
    fn policy_show(&self, cx: &mut Ctx, source: bool) -> Result<i32> {
        self.testnet_only("policy show")?;
        if source {
            return Err(self.na("policy show --source"));
        }
        let policy = self.describe_selected(cx)?;
        cx.out.value(&policy);
        Ok(0)
    }
    fn exec_binding(&self, _: &mut Ctx, wallet: Option<&str>) -> Result<Binding> {
        let policy = self.describe(wallet)?;
        let text = |key: &str| {
            policy[key]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| Error::config(format!("the Tempo bundle names no {key}")))
        };
        Ok(Binding {
            wallet: Some(text("vault")?),
            policy: Some(format!("{}@{}", text("id")?, text("policyDigest")?)),
        })
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
        let policy = self.describe(r.wallet.as_deref())?;
        let amount = self.decimal(&r.amount, &policy)?;
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
        let max = c
            .max_amount
            .as_deref()
            .ok_or_else(|| Error::usage("an MPP call needs --max-amount"))?;
        let max = self.max_amount(max)?;
        let policy = self.describe_selected(cx)?;
        let units = self.decimal(&max, &policy)?;
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
    // Help anywhere in the words prints the group usage.
    if args.first().is_some_and(|s| s == "help")
        || args.iter().any(|s| matches!(s.as_str(), "-h" | "--help"))
    {
        return allowit_tempo::run(&["help".to_owned()], stdout, stderr);
    }
    if let Some(selected) = selected() {
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
        let policy = serde_json::json!({"token": "0x20C0000000000000000000000000000000000001"});
        let a = rail
            .parse_amount("AlphaUSD:1.5", super::super::asset::Use::Spend)
            .unwrap();
        assert_eq!(rail.decimal(&a, &policy).unwrap(), "1.5");
        let base = rail
            .parse_amount("1500000", super::super::asset::Use::Spend)
            .unwrap();
        assert_eq!(rail.decimal(&base, &policy).unwrap(), "1.5");
    }
    #[test]
    fn amounts_of_another_token_exit_4() {
        let rail = Tempo(crate::routing::info("tempo:testnet").unwrap());
        let policy = serde_json::json!({"token": TOKENS[0].issuer.unwrap()});
        for other in [
            "pathUSD:1",
            "AlphaUSD:0x20c0000000000000000000000000000000000000:1",
            "FOO:0x20c00000000000000000000000000000000000ff:1",
        ] {
            let a = rail
                .parse_amount(other, super::super::asset::Use::Spend)
                .unwrap();
            assert_eq!(rail.decimal(&a, &policy).unwrap_err().code, 4, "{other}");
        }
        let explicit = rail
            .parse_amount(
                "USD:0x20C0000000000000000000000000000000000001:2",
                super::super::asset::Use::Spend,
            )
            .unwrap();
        assert_eq!(rail.decimal(&explicit, &policy).unwrap(), "2");
        let none = serde_json::json!({});
        let base = rail
            .parse_amount("1", super::super::asset::Use::Spend)
            .unwrap();
        assert_eq!(rail.decimal(&base, &none).unwrap(), "0.000001");
        let named = rail
            .parse_amount("AlphaUSD:1", super::super::asset::Use::Spend)
            .unwrap();
        assert_eq!(rail.decimal(&named, &none).unwrap_err().code, 4);
    }
    #[test]
    fn paid_call_maximums_are_decimals_of_the_vault_token() {
        let rail = Tempo(crate::routing::info("tempo:testnet").unwrap());
        let policy = serde_json::json!({"token": TOKENS[0].issuer.unwrap()});
        for (text, decimal) in [("0.05", "0.05"), ("1", "1"), ("AlphaUSD:0.1", "0.1")] {
            let max = rail.max_amount(text).unwrap();
            assert_eq!(rail.decimal(&max, &policy).unwrap(), decimal, "{text}");
        }
        for bad in ["0", "0.0000001", ".5", "1."] {
            assert_eq!(rail.max_amount(bad).unwrap_err().code, 2, "{bad}");
        }
        let other = rail.max_amount("pathUSD:0.1").unwrap();
        assert_eq!(rail.decimal(&other, &policy).unwrap_err().code, 4);
    }
}
