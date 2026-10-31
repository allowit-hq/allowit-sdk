//! Solana adapter over `allowit-native` and the engine harness.
//!
//! - `keys fund`: an RPC airdrop on devnet and testnet.
//! - `keys import` of an owner-issued executor bundle, `wallet show`,
//!   `wallet fund`, `wallet withdraw`, `wallet pause`, `wallet close`,
//!   `policy new` and `policy params set`: the native vault lifecycle, test
//!   networks only. Owner operations sign with `--sign-with-key`.
//! - `exec`, `eval`, `status`, `policy show`: the engine harness, through
//!   `ALLOWIT_TOKEN`.
pub(crate) mod audit;
pub mod handoff;
pub(crate) mod harness;
pub(crate) mod policy;

use super::{
    Caps, ExecRequest, Mode, Param, Rail, SignWith, Signing, WalletCreate,
    asset::{Amount, Asset, Token},
    require_op, require_signing,
};
use crate::{
    config,
    ctx::Ctx,
    error::{Error, Result},
    request::Flags,
    routing::RailInfo,
    settings::keys::{self, Key},
};
use sha2::Digest;
use std::{path::Path, time::Duration};

pub(crate) struct Solana(pub &'static RailInfo);

const DEVNET: &[Token] = &[
    Token {
        code: "SOL",
        decimals: 9,
        native: true,
        issuer: None,
    },
    Token {
        code: "USDC",
        decimals: 6,
        native: false,
        issuer: Some("4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU"),
    },
];
const TESTNET: &[Token] = &[
    Token {
        code: "SOL",
        decimals: 9,
        native: true,
        issuer: None,
    },
    Token {
        code: "USDC",
        decimals: 6,
        native: false,
        issuer: None,
    },
];
const MAINNET: &[Token] = &[
    Token {
        code: "SOL",
        decimals: 9,
        native: true,
        issuer: None,
    },
    Token {
        code: "USDC",
        decimals: 6,
        native: false,
        issuer: Some("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
    },
];

/// The vault token's decimals. The native lifecycle and the harness take
/// six-decimal display amounts.
const VAULT_DECIMALS: u8 = 6;

impl Solana {
    /// The native lifecycle runs on test networks only.
    fn testnet_only(&self, verb: &str) -> Result<()> {
        if self.0.testnet {
            Ok(())
        } else {
            Err(self.na(verb))
        }
    }
    /// Runs one native lifecycle operation with resolved settings.
    fn native(
        &self,
        cx: &mut Ctx,
        name: &str,
        pos: &[String],
        mut settings: Vec<(&'static str, String)>,
    ) -> Result<i32> {
        if let Some(url) = cx.rpc_url(self.0)? {
            settings.push(("ALLOWIT_RPC_URL", url));
        }
        let json = cx.out.json;
        let out = &mut cx.out;
        config::with_settings(&settings, || {
            policy::run(name, pos, json, &mut out.stdout, &mut out.stderr)
        })
    }
    /// The owner key file for an owner operation.
    fn owner_signer(
        &self,
        cx: &mut Ctx,
        verb: &str,
        s: &Signing,
    ) -> Result<(&'static str, String)> {
        self.testnet_only(verb)?;
        require_signing(self, verb, s)?;
        let SignWith::Key(key) = &s.with else {
            return Err(self.na(&format!("{verb} --sign-with-link")));
        };
        key.check_rail(self.0.id)?;
        let path = cx.signer(key)?;
        Ok(("ALLOWIT_OWNER_KEYPAIR", path.to_string_lossy().into_owned()))
    }
    /// A vault amount as a six-decimal display amount.
    fn vault_amount(&self, amount: &Amount) -> Result<String> {
        match &amount.asset {
            Asset::Installed => {}
            Asset::Token { code, .. } if code == "USDC" => {}
            _ => {
                return Err(Error {
                    code: 4,
                    message: format!(
                        "the vault holds only its token; {} names another asset",
                        amount.text
                    ),
                    status: None,
                    config: false,
                });
            }
        }
        let units = amount
            .units()
            .ok_or_else(|| Error::usage("an exact amount is required"))?;
        Ok(super::asset::format_units(units, VAULT_DECIMALS))
    }
    fn harness_flags(&self, r: &ExecRequest) -> Result<Flags> {
        let code = match &r.amount.asset {
            Asset::Installed => "USDC".to_owned(),
            Asset::Native { code } | Asset::Token { code, .. } => code.clone(),
            Asset::Contract(_) => return Err(Error::usage("Solana has no asset contracts")),
        };
        let decimals = r.amount.decimals.unwrap_or(VAULT_DECIMALS);
        let units = r
            .amount
            .units()
            .ok_or_else(|| Error::usage("an exact amount is required"))?;
        let mut flags = Flags::default();
        for (k, v) in [
            ("action", "transfer".to_owned()),
            ("rail", "solana".to_owned()),
            ("op", format!("transfer{code}")),
            ("addr", r.to.clone()),
            ("amount", super::asset::format_units(units, decimals)),
        ] {
            flags.0.insert(k.into(), v);
        }
        Ok(flags)
    }
}

impl Rail for Solana {
    fn info(&self) -> &'static RailInfo {
        self.0
    }
    fn tokens(&self) -> &'static [Token] {
        match self.0.name() {
            "solana:devnet" => DEVNET,
            "solana:testnet" => TESTNET,
            _ => MAINNET,
        }
    }
    fn caps(&self) -> Caps {
        Caps {
            curve: "ed25519",
            ops: &["transfer"],
            param_keys: &["transaction_cap", "day_total_cap"],
            signing: &["key", "link"],
            verbs: if self.0.testnet {
                &[
                    "keys fund",
                    "keys import",
                    "wallet create",
                    "wallet show",
                    "wallet fund",
                    "wallet withdraw",
                    "wallet pause",
                    "wallet close",
                    "policy new",
                    "policy show",
                    "policy params set",
                    "exec",
                    "eval",
                    "status",
                ]
            } else {
                &["policy show", "exec", "eval", "status"]
            },
        }
    }
    fn keys_fund(&self, cx: &mut Ctx, key: &Key) -> Result<i32> {
        if !self.0.testnet {
            return Err(self.na("keys fund"));
        }
        key.check_rail(self.0.id)?;
        let url = match cx.rpc_url(self.0)? {
            Some(url) => url,
            None if self.0.name() == "solana:devnet" => "https://api.devnet.solana.com".into(),
            None => "https://api.testnet.solana.com".into(),
        };
        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "requestAirdrop",
            "params": [key.address, 1_000_000_000u64]
        });
        let reply: serde_json::Value = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .and_then(|c| c.post(url).json(&body).send())
            .and_then(|r| r.json())
            .map_err(|_| {
                Error::uncertain(
                    "the airdrop request has no answer; check the balance before retrying",
                )
            })?;
        let signature = reply["result"].as_str().ok_or_else(|| {
            Error::config(format!(
                "the RPC refused the airdrop: {}",
                reply["error"]["message"]
                    .as_str()
                    .unwrap_or("no reason given")
            ))
        })?;
        cx.out.value(&serde_json::json!({
            "key": key.name,
            "address": key.address,
            "rail": self.0.id,
            "lamports": 1_000_000_000u64,
            "signature": signature,
        }));
        Ok(0)
    }
    fn keys_import(
        &self,
        cx: &mut Ctx,
        name: &str,
        file: &Path,
        bundle: &serde_json::Value,
    ) -> Result<i32> {
        self.testnet_only("keys import")?;
        let executor = bundle["context"]["executor"]
            .as_str()
            .ok_or_else(|| Error::usage("the executor bundle names no executor"))?
            .to_owned();
        let dir = cx.dir()?;
        keys::valid_name(name)?;
        if keys::exists(&dir, name) {
            return Err(Error::config(format!("key {name} already exists")));
        }
        let code = self.native(cx, "import", &[file.to_string_lossy().into_owned()], vec![])?;
        keys::add(&dir, name, self.0.id, &executor)?;
        Ok(code)
    }
    fn wallet_create(&self, cx: &mut Ctx, w: &WalletCreate) -> Result<i32> {
        self.testnet_only("wallet create")?;
        if !matches!(w.signing.with, SignWith::Link) {
            return Err(self.na(&format!(
                "wallet create --{}",
                if matches!(w.signing.with, SignWith::Key(_)) {
                    "sign-with-key"
                } else {
                    "build-only"
                }
            )));
        }
        if w.kind.is_some() {
            return Err(Error::usage("--kind does not apply to Solana vaults"));
        }
        if w.x402 {
            return Err(self.na("wallet create --x402"));
        }
        if w.asset != "USDC" {
            return Err(Error::usage("Solana vaults hold USDC"));
        }
        let fund = w.fund.as_ref().ok_or_else(|| {
            Error::usage("Solana vaults are funded at creation; give --fund AMOUNT")
        })?;
        let amount = self.vault_amount(fund)?;
        let policy = std::fs::canonicalize(&w.policy)
            .map_err(|_| Error::usage(format!("cannot read {}", w.policy.display())))?;
        // A retry of the same creation yields the same intent and request ID.
        let digest = sha2::Sha256::digest(
            serde_json::to_vec(&serde_json::json!([
                self.0.id,
                w.owner,
                w.agent.address,
                policy.to_string_lossy(),
                amount
            ]))
            .unwrap_or_default(),
        );
        let request_id = format!(
            "create-{}",
            digest[..8]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        cx.out.progress(&request_id);
        self.native(
            cx,
            "deploy",
            &[amount],
            vec![
                ("ALLOWIT_OWNER", w.owner.clone()),
                ("ALLOWIT_EXECUTOR", w.agent.address.clone()),
                ("ALLOWIT_POLICY_FILE", policy.to_string_lossy().into_owned()),
                ("ALLOWIT_REQUEST_ID", request_id),
            ],
        )
    }
    fn wallet_show(&self, cx: &mut Ctx, _: Option<&str>) -> Result<i32> {
        self.testnet_only("wallet show")?;
        self.native(cx, "status", &[], vec![])
    }
    fn wallet_fund(
        &self,
        cx: &mut Ctx,
        amount: &Amount,
        from: Option<&Key>,
        s: &Signing,
    ) -> Result<i32> {
        let signer = self.owner_signer(cx, "wallet fund", s)?;
        if let (Some(from), SignWith::Key(key)) = (from, &s.with)
            && from.address != key.address
        {
            return Err(Error::usage(
                "the Solana vault takes deposits from its owner; --from must be the --sign-with-key key",
            ));
        }
        let value = self.vault_amount(amount)?;
        self.native(cx, "fund", &[value], vec![signer])
    }
    fn wallet_withdraw(
        &self,
        cx: &mut Ctx,
        amount: &Amount,
        to: Option<&str>,
        s: &Signing,
    ) -> Result<i32> {
        if to.is_some() {
            return Err(self.na("wallet withdraw --to"));
        }
        if amount.units().is_none() {
            return Err(self.na("wallet withdraw CODE:max"));
        }
        let signer = self.owner_signer(cx, "wallet withdraw", s)?;
        let value = self.vault_amount(amount)?;
        self.native(cx, "withdraw", &[value], vec![signer])
    }
    fn wallet_pause(&self, cx: &mut Ctx, s: &Signing) -> Result<i32> {
        let signer = self.owner_signer(cx, "wallet pause", s)?;
        self.native(cx, "revoke", &[], vec![signer])
    }
    fn wallet_close(&self, cx: &mut Ctx, s: &Signing) -> Result<i32> {
        let signer = self.owner_signer(cx, "wallet close", s)?;
        self.native(cx, "close", &[], vec![signer])
    }
    fn policy_new(&self, cx: &mut Ctx, prompt: &str, output: Option<&Path>) -> Result<i32> {
        self.testnet_only("policy new")?;
        let mut settings = vec![];
        if let Some(path) = output {
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .map_err(|_| Error::config("cannot locate the working directory"))?
                    .join(path)
            };
            settings.push(("ALLOWIT_POLICY_FILE", path.to_string_lossy().into_owned()));
        }
        let code = self.native(cx, "generate", &[prompt.to_owned()], settings)?;
        if !cx.out.json {
            cx.out.kind = crate::term::Kind::Rust;
        }
        Ok(code)
    }
    fn policy_show(&self, cx: &mut Ctx, source: bool) -> Result<i32> {
        let json = cx.out.json;
        let code = harness::show(source, json, &mut cx.out.stdout)?;
        if source {
            cx.out.kind = crate::term::Kind::Rust;
        }
        Ok(code)
    }
    fn policy_params_set(&self, cx: &mut Ctx, params: &[Param], s: &Signing) -> Result<i32> {
        let signer = self.owner_signer(cx, "policy params set", s)?;
        let mut operations = vec![];
        for p in params {
            let method = match p.key.as_str() {
                "day_total_cap" => "tune",
                "transaction_cap" => "tune-action",
                other => return Err(self.na(&format!("policy params set {other}"))),
            };
            let amount = p
                .amount
                .as_ref()
                .ok_or_else(|| Error::usage(format!("{} needs an amount", p.key)))?;
            operations.push((method, self.vault_amount(amount)?));
        }
        let mut code = 0;
        for (method, value) in operations {
            code = self.native(cx, method, &[value], vec![signer.clone()])?;
            if code != 0 {
                break;
            }
        }
        Ok(code)
    }
    fn exec(&self, cx: &mut Ctx, r: &ExecRequest) -> Result<i32> {
        self.request(cx, r)
    }
    fn eval(&self, cx: &mut Ctx, r: &ExecRequest) -> Result<i32> {
        self.request(cx, r)
    }
    fn status(&self, cx: &mut Ctx, id: &str, _: Option<&str>, wait: Duration) -> Result<i32> {
        let json = cx.out.json;
        let out = &mut cx.out;
        harness::status(id, wait, json, &mut out.stdout, &mut out.stderr)
    }
}

impl Solana {
    fn request(&self, cx: &mut Ctx, r: &ExecRequest) -> Result<i32> {
        require_op(self, &r.op)?;
        if r.build_only {
            return Err(self.na("exec --build-only"));
        }
        if r.token_out.is_some() || r.args.is_some() {
            return Err(Error::usage(
                "--token-out and --args apply to swap and custom operations, not transfer",
            ));
        }
        super::address::check_recipient(self.0.family, &r.to)?;
        let flags = self.harness_flags(r)?;
        let json = cx.out.json;
        let out = &mut cx.out;
        let command = match r.mode {
            Mode::Exec => "exec",
            Mode::Eval => "eval",
        };
        harness::submit(
            command,
            &flags,
            &r.request_id,
            r.wait,
            json,
            &mut out.stdout,
            &mut out.stderr,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vendors::asset::{Use, parse};

    #[test]
    fn mints_are_valid_and_amounts_map_to_six_decimals() {
        for tokens in [DEVNET, TESTNET, MAINNET] {
            for t in tokens {
                if let Some(mint) = t.issuer {
                    assert!(crate::request::solana_address(mint), "{mint}");
                }
            }
        }
        let rail = Solana(crate::routing::info("solana:devnet").unwrap());
        let usdc = parse(
            crate::routing::Family::Solana,
            DEVNET,
            "USDC:2.5",
            Use::Spend,
        )
        .unwrap();
        assert_eq!(rail.vault_amount(&usdc).unwrap(), "2.5");
        let base = parse(crate::routing::Family::Solana, DEVNET, "250000", Use::Spend).unwrap();
        assert_eq!(rail.vault_amount(&base).unwrap(), "0.25");
        let sol = parse(crate::routing::Family::Solana, DEVNET, "SOL:1", Use::Spend).unwrap();
        assert_eq!(rail.vault_amount(&sol).unwrap_err().code, 4);
    }

    #[test]
    fn harness_flags_name_the_transfer() {
        let rail = Solana(crate::routing::info("solana:devnet").unwrap());
        let r = ExecRequest {
            mode: Mode::Exec,
            op: "transfer".into(),
            to: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            amount: parse(crate::routing::Family::Solana, DEVNET, "USDC:2", Use::Spend).unwrap(),
            token_out: None,
            args: None,
            request_id: "pay-0001".into(),
            wait: Duration::ZERO,
            build_only: false,
            wallet: None,
        };
        let flags = rail.harness_flags(&r).unwrap();
        assert_eq!(flags.get("op"), "transferUSDC");
        assert_eq!(flags.get("amount"), "2");
        assert_eq!(flags.get("action"), "transfer");
        assert_eq!(flags.get("rail"), "solana");
    }
}
