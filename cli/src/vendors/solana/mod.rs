//! Solana adapter over `allowit-native` and the engine harness.
//!
//! - `keys fund`: an RPC airdrop on devnet and testnet.
//! - `keys import` of an owner-issued executor bundle, `wallet show`,
//!   `wallet fund`, `wallet withdraw`, `wallet pause`, `wallet close`,
//!   `policy new` and `policy params set`: the native vault lifecycle, test
//!   networks only. Owner operations sign with `--sign-with-key`. A selected
//!   wallet must be the vault of the policy in `ALLOWIT_POLICY_DIR`, which
//!   defaults to `vaults/solana-NETWORK` in the config directory.
//! - `exec`, `eval`, `status`, `policy show`: the engine harness, through
//!   `ALLOWIT_TOKEN`. The harness names no vault, so these refuse a selected
//!   wallet.
pub(crate) mod audit;
pub mod handoff;
pub(crate) mod harness;
pub(crate) mod policy;

use super::{
    Binding, Caps, ExecRequest, Mode, Param, Rail, SignWith, Signing, WalletCreate,
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
use policy::RunOptions;
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
        options: &RunOptions,
        mut settings: Vec<(&'static str, String)>,
    ) -> Result<i32> {
        // The private config directory holds the default vault folder.
        cx.dir()?;
        if let Some(url) = cx.rpc_url(self.0)? {
            settings.push(("ALLOWIT_RPC_URL", url));
        }
        let out = &mut cx.out;
        config::with_settings(&settings, || policy::run(name, pos, options, out))
    }
    /// The options of a vault operation: the selected wallet, which must be
    /// the vault of the loaded policy.
    fn vault_options(&self, cx: &mut Ctx, s: Option<&Signing>) -> Result<RunOptions> {
        Ok(RunOptions {
            wallet: cx.wallet(self.0)?,
            request_id: s.and_then(|s| s.request_id.clone()),
            additional: s.is_some_and(|s| s.additional),
            legacy: false,
            retry_of: None,
        })
    }
    /// The engine harness names a policy but no vault, so it cannot confirm a
    /// selected wallet.
    fn harness_wallet(&self, wallet: Option<&str>) -> Result<()> {
        match wallet {
            None => Ok(()),
            Some(w) => Err(Error::config(format!(
                "the engine harness cannot confirm wallet {w} on {}; unset --wallet, ALLOWIT_WALLET and allowit wallet use to send through the policy that ALLOWIT_TOKEN names",
                self.0.id
            ))),
        }
    }
    /// True when a named token is a token of the rail's table with the same
    /// mint. An explicit mint that the table cannot confirm is refused.
    fn known_token(&self, code: &str, issuer: Option<&str>) -> bool {
        self.tokens()
            .iter()
            .any(|t| !t.native && t.code == code && t.issuer == issuer)
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
    /// A vault amount as a six-decimal display amount. A named amount must
    /// resolve to the vault's configured mint.
    fn vault_amount(&self, amount: &Amount) -> Result<String> {
        let mint = if matches!(amount.asset, Asset::Installed) {
            None
        } else {
            policy::configured_mint()?
        };
        vault_amount(amount, mint.as_deref())
    }
    /// Refuses a `--asset` other than the vault's configured mint: the mint
    /// itself, or a code of the rail's table with that mint.
    fn vault_asset(&self, asset: &str) -> Result<()> {
        let mint = policy::configured_mint()?.ok_or_else(no_mint)?;
        let named = self
            .tokens()
            .iter()
            .any(|t| !t.native && t.code == asset && t.issuer == Some(mint.as_str()));
        if asset == mint || named {
            Ok(())
        } else {
            Err(Error {
                code: 4,
                message: format!(
                    "the vault holds mint {mint}; --asset {asset} names another asset"
                ),
                status: None,
                config: false,
            })
        }
    }
    fn harness_flags(&self, r: &ExecRequest) -> Result<Flags> {
        let code = match &r.amount.asset {
            Asset::Installed => "USDC".to_owned(),
            Asset::Native { code } => code.clone(),
            Asset::Token { code, issuer } if self.known_token(code, issuer.as_deref()) => {
                code.clone()
            }
            Asset::Token { .. } => {
                return Err(Error {
                    code: 4,
                    message: format!(
                        "{} names a token that this rail does not list; use one of its codes",
                        r.amount.text
                    ),
                    status: None,
                    config: false,
                });
            }
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

/// Joins the JSON documents printed after START into one array when there
/// are several.
fn one_document(stdout: &mut String, start: usize, compact: bool) {
    let values: Vec<serde_json::Value> = serde_json::Deserializer::from_str(&stdout[start..])
        .into_iter()
        .collect::<std::result::Result<_, _>>()
        .unwrap_or_default();
    if values.len() < 2 {
        return;
    }
    let array = serde_json::Value::Array(values);
    let text = if compact {
        serde_json::to_string(&array)
    } else {
        serde_json::to_string_pretty(&array)
    }
    .unwrap_or_default();
    stdout.truncate(start);
    stdout.push_str(&text);
    stdout.push('\n');
}

fn no_mint() -> Error {
    Error::config(
        "the vault mint is not configured; set ALLOWIT_MINT or import the executor bundle",
    )
}

/// A vault amount for the vault holding `mint`, as a six-decimal display
/// amount. Base units need no mint; a named token must have that mint.
fn vault_amount(amount: &Amount, mint: Option<&str>) -> Result<String> {
    match &amount.asset {
        Asset::Installed => {}
        // Without a configured mint no named asset can be confirmed.
        _ if mint.is_none() => return Err(no_mint()),
        Asset::Token {
            issuer: Some(issuer),
            ..
        } if mint == Some(issuer.as_str()) => {}
        _ => {
            return Err(Error {
                code: 4,
                message: format!(
                    "the vault holds mint {}; {} names another asset; give the amount in base units",
                    mint.unwrap_or("-"),
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
            address_is_key: true,
            service_asset: "USDC",
            ops: &["transfer"],
            // SetDailyLimit counts transferred amounts without gas.
            param_keys: &["transaction_cap", "day_spent_cap"],
            // The vault program takes its first deposit from a signing link;
            // later owner operations sign with a local owner key.
            signing: if self.0.testnet {
                &[
                    ("wallet create", &["link"]),
                    ("wallet fund", &["key"]),
                    ("wallet withdraw", &["key"]),
                    ("wallet pause", &["key"]),
                    ("wallet close", &["key"]),
                    ("policy params set", &["key"]),
                ]
            } else {
                &[]
            },
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
    /// The native vault files of the current commands live in the config
    /// directory, one folder per rail, unless `ALLOWIT_POLICY_DIR` is set.
    fn settings(&self, cx: &mut Ctx) -> Result<Vec<(&'static str, String)>> {
        if config::env_present("ALLOWIT_POLICY_DIR") {
            return Ok(vec![]);
        }
        // Input checks run before the config directory exists; native()
        // creates it before a native operation writes there.
        let base = cx.dir_path()?;
        let base = base.canonicalize().unwrap_or(base);
        let dir = base.join("vaults").join(self.0.name().replace(':', "-"));
        Ok(vec![(
            "ALLOWIT_POLICY_DIR",
            dir.to_string_lossy().into_owned(),
        )])
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
        // The bundle's network decides where its state belongs; nothing is
        // written before it matches the selected rail.
        let network = bundle["context"]["network"].as_str().unwrap_or_default();
        if crate::routing::normalize(network).ok() != Some(self.0.id) {
            return Err(Error::config(format!(
                "the executor bundle belongs to {network}, not {}",
                self.0.id
            )));
        }
        let executor = bundle["context"]["executor"]
            .as_str()
            .ok_or_else(|| Error::usage("the executor bundle names no executor"))?
            .to_owned();
        let dir = cx.dir()?;
        keys::valid_name(name)?;
        if keys::exists(&dir, name) {
            return Err(Error::config(format!("key {name} already exists")));
        }
        let code = self.native(
            cx,
            "import",
            &[file.to_string_lossy().into_owned()],
            &RunOptions::default(),
            vec![],
        )?;
        keys::add(&dir, name, self.0.id, &executor)?;
        Ok(code)
    }
    fn wallet_create(&self, cx: &mut Ctx, w: &WalletCreate) -> Result<i32> {
        self.testnet_only("wallet create")?;
        require_signing(self, "wallet create", &w.signing)?;
        if w.kind.is_some() {
            return Err(Error::usage("--kind does not apply to Solana vaults"));
        }
        if w.x402 {
            return Err(self.na("wallet create --x402"));
        }
        // The native vault takes its first deposit at creation.
        let fund = w
            .fund
            .as_ref()
            .ok_or_else(|| self.na("wallet create without --fund AMOUNT"))?;
        self.vault_asset(&w.asset)?;
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
        let derived = format!(
            "create-{}",
            digest[..8]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        // An explicit new ID replaces the derived preparation once it expired
        // unsigned; the original journal entry stays.
        let (request_id, retry_of) = match &w.signing.request_id {
            Some(id) if *id != derived => (id.clone(), Some(derived)),
            _ => (derived, None),
        };
        if w.signing.request_id.is_none() {
            cx.out.generated_id(&request_id);
        }
        let options = RunOptions {
            request_id: Some(request_id),
            retry_of,
            ..RunOptions::default()
        };
        self.native(
            cx,
            "deploy",
            &[amount],
            &options,
            vec![
                ("ALLOWIT_OWNER", w.owner.clone()),
                ("ALLOWIT_EXECUTOR", w.agent.address.clone()),
                ("ALLOWIT_POLICY_FILE", policy.to_string_lossy().into_owned()),
            ],
        )
    }
    fn wallet_show(&self, cx: &mut Ctx, wallet: Option<&str>) -> Result<i32> {
        self.testnet_only("wallet show")?;
        let mut options = self.vault_options(cx, None)?;
        if let Some(w) = wallet {
            options.wallet = Some(w.to_owned());
        }
        self.native(cx, "status", &[], &options, vec![])
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
        let options = self.vault_options(cx, Some(s))?;
        self.native(cx, "fund", &[value], &options, vec![signer])
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
        let options = self.vault_options(cx, Some(s))?;
        self.native(cx, "withdraw", &[value], &options, vec![signer])
    }
    fn wallet_pause(&self, cx: &mut Ctx, s: &Signing) -> Result<i32> {
        let signer = self.owner_signer(cx, "wallet pause", s)?;
        let options = self.vault_options(cx, Some(s))?;
        self.native(cx, "revoke", &[], &options, vec![signer])
    }
    fn wallet_close(&self, cx: &mut Ctx, s: &Signing) -> Result<i32> {
        let signer = self.owner_signer(cx, "wallet close", s)?;
        let options = self.vault_options(cx, Some(s))?;
        self.native(cx, "close", &[], &options, vec![signer])
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
        self.native(
            cx,
            "generate",
            &[prompt.to_owned()],
            &RunOptions::default(),
            settings,
        )
    }
    fn policy_show(&self, cx: &mut Ctx, source: bool) -> Result<i32> {
        let wallet = cx.wallet(self.0)?;
        self.harness_wallet(wallet.as_deref())?;
        harness::show(source, &mut cx.out)
    }
    fn policy_params_set(&self, cx: &mut Ctx, params: &[Param], s: &Signing) -> Result<i32> {
        let signer = self.owner_signer(cx, "policy params set", s)?;
        let mut operations = vec![];
        for p in params {
            let method = match p.key.as_str() {
                "day_spent_cap" => "tune",
                "transaction_cap" => "tune-action",
                other => return Err(self.na(&format!("policy params set {other}"))),
            };
            let amount = p
                .amount
                .as_ref()
                .ok_or_else(|| Error::usage(format!("{} needs an amount", p.key)))?;
            operations.push((method, self.vault_amount(amount)?));
        }
        let options = self.vault_options(cx, Some(s))?;
        // Each operation prints one result; several results become one JSON
        // array, or text blocks separated by a blank line.
        let start = cx.out.stdout.len();
        let run = || -> Result<i32> {
            let mut code = 0;
            for (i, (method, value)) in operations.into_iter().enumerate() {
                let before = cx.out.stdout.len();
                code = self.native(cx, method, &[value], &options, vec![signer.clone()])?;
                if i > 0 && !cx.out.json && cx.out.stdout.len() > before {
                    cx.out.stdout.insert(before, '\n');
                }
                if code != 0 {
                    break;
                }
            }
            Ok(code)
        };
        let result = run();
        if cx.out.json {
            one_document(&mut cx.out.stdout, start, cx.out.compact);
        }
        result
    }
    fn exec(&self, cx: &mut Ctx, r: &ExecRequest) -> Result<i32> {
        self.request(cx, r)
    }
    fn eval(&self, cx: &mut Ctx, r: &ExecRequest) -> Result<i32> {
        self.request(cx, r)
    }
    fn exec_binding(&self, _: &mut Ctx, wallet: Option<&str>) -> Result<Binding> {
        self.harness_wallet(wallet)?;
        Ok(Binding {
            wallet: None,
            policy: Some(harness::policy()?),
        })
    }
    fn legacy(&self, cx: &mut Ctx, verb: &str, args: &[String]) -> Result<i32> {
        let Some(name) = verb.strip_prefix("policy ") else {
            return Err(self.na(verb));
        };
        if name == "execute"
            && !args
                .first()
                .is_some_and(|r| crate::request::solana_address(r))
        {
            return Err(Error::usage(format!(
                "RECIPIENT {} is not a Solana token account address",
                args.first().map(String::as_str).unwrap_or_default()
            )));
        }
        let options = RunOptions::from_environment(cx.wallet(self.0)?);
        policy::run(name, args, &options, &mut cx.out)
    }
    fn status(&self, cx: &mut Ctx, id: &str, wallet: Option<&str>, wait: Duration) -> Result<i32> {
        self.harness_wallet(wallet)?;
        harness::status(id, wait, &mut cx.out)
    }
}

impl Solana {
    fn request(&self, cx: &mut Ctx, r: &ExecRequest) -> Result<i32> {
        require_op(self, &r.op)?;
        self.harness_wallet(r.wallet.as_deref())?;
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
        let command = match r.mode {
            Mode::Exec => "exec",
            Mode::Eval => "eval",
        };
        harness::submit(command, &flags, &r.request_id, r.wait, &mut cx.out)
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
        let p = |tokens: &[Token], text: &str| {
            parse(crate::routing::Family::Solana, tokens, text, Use::Spend).unwrap()
        };
        let usdc_mint = DEVNET[1].issuer.unwrap();
        let usdc = p(DEVNET, "USDC:2.5");
        assert_eq!(vault_amount(&usdc, Some(usdc_mint)).unwrap(), "2.5");
        let base = p(DEVNET, "250000");
        assert_eq!(vault_amount(&base, None).unwrap(), "0.25");
        assert_eq!(vault_amount(&base, Some("other")).unwrap(), "0.25");
        let sol = p(DEVNET, "SOL:1");
        assert_eq!(vault_amount(&sol, Some(usdc_mint)).unwrap_err().code, 4);
        let explicit = p(DEVNET, &format!("USDC:{usdc_mint}:1"));
        assert_eq!(vault_amount(&explicit, Some(usdc_mint)).unwrap(), "1");
        // A vault configured for another mint refuses the rail's USDC.
        let test_mint = "GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq";
        assert_eq!(vault_amount(&usdc, Some(test_mint)).unwrap_err().code, 4);
        assert_eq!(
            vault_amount(&explicit, Some(test_mint)).unwrap_err().code,
            4
        );
        // Without a configured mint, a named amount cannot be confirmed.
        assert_eq!(vault_amount(&usdc, None).unwrap_err().code, 3);
        assert_eq!(vault_amount(&sol, None).unwrap_err().code, 3);
        assert_eq!(
            vault_amount(&p(DEVNET, "native:1"), None).unwrap_err().code,
            3
        );
        // The testnet table has no USDC mint: a named amount never matches.
        let plain = p(TESTNET, "USDC:5");
        assert_eq!(vault_amount(&plain, Some(test_mint)).unwrap_err().code, 4);
        let other = p(TESTNET, &format!("USDC:{}:5", MAINNET[1].issuer.unwrap()));
        assert_eq!(vault_amount(&other, Some(test_mint)).unwrap_err().code, 4);
    }

    #[test]
    fn several_results_print_one_json_document() {
        let mut stdout = "before\n{\n  \"a\": 1\n}\n{\n  \"b\": 2\n}\n".to_owned();
        one_document(&mut stdout, 7, false);
        let value: serde_json::Value = serde_json::from_str(&stdout[7..]).unwrap();
        assert_eq!(value, serde_json::json!([{"a": 1}, {"b": 2}]));
        assert!(stdout.starts_with("before\n"));
        let mut single = "{\"a\":1}\n".to_owned();
        one_document(&mut single, 0, true);
        assert_eq!(single, "{\"a\":1}\n");
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
        let testnet = Solana(crate::routing::info("solana:testnet").unwrap());
        let other = ExecRequest {
            amount: parse(
                crate::routing::Family::Solana,
                TESTNET,
                &format!("USDC:{}:2", MAINNET[1].issuer.unwrap()),
                Use::Spend,
            )
            .unwrap(),
            ..r
        };
        assert_eq!(testnet.harness_flags(&other).err().map(|e| e.code), Some(4));
        assert_eq!(flags.get("amount"), "2");
        assert_eq!(flags.get("action"), "transfer");
        assert_eq!(flags.get("rail"), "solana");
    }
}
