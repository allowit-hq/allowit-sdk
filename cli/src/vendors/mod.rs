//! Rail adapters. Command files reach a chain only through [`rail`] and the
//! [`Rail`] trait; rail code lives only in `vendors/<rail>/`, as thin adapters
//! over `allowit-stellar-wallet`, `allowit-native` and `allowit-tempo`.
//!
//! Every verb has a default that exits 3 with "VERB is not available on RAIL
//! yet". An adapter overrides only the verbs whose backend works, and lists
//! them in [`Caps::verbs`], which `rail show` prints.
pub(crate) mod address;
pub(crate) mod asset;
pub(crate) mod solana;
pub(crate) mod stellar;
pub(crate) mod tempo;
pub(crate) mod typed;

use crate::{
    ctx::Ctx,
    error::{Error, Result},
    exit::not_available,
    routing::{Family, RailInfo},
    settings::keys::Key,
};
use asset::{Amount, Token, Use};
use std::{path::Path, time::Duration};

/// Every verb a rail may serve, in `rail show` order.
pub(crate) const VERBS: &[&str] = &[
    "keys fund",
    "keys import",
    "wallet create",
    "wallet show",
    "wallet fund",
    "wallet withdraw",
    "wallet update",
    "wallet pause",
    "wallet resume",
    "wallet close",
    "policy new",
    "policy show",
    "policy build",
    "policy deploy",
    "policy params",
    "policy params set",
    "exec",
    "eval",
    "status",
    "tx send",
    "call",
    "skills",
    "curl",
    "http",
    "mcp",
];

/// The canonical policy parameter keys of the contracts params map.
pub(crate) const PARAM_KEYS: &[&str] = &[
    "transaction_cap",
    "spent_cap",
    "day_spent_cap",
    "day_total_cap",
    "call_cap",
    "recipients",
    "tokens",
    "expiry",
    "operations",
];

/// What a rail supports. It drives `rail show`, help and the exit-3 errors.
pub(crate) struct Caps {
    pub curve: &'static str,
    /// True when an account address is its public key (ed25519 rails).
    pub address_is_key: bool,
    /// The asset `setup` chooses for paid services.
    pub service_asset: &'static str,
    /// `exec --op` values that work.
    pub ops: &'static [&'static str],
    /// `policy params set` keys that the rail can change.
    pub param_keys: &'static [&'static str],
    /// Owner signing modes that work, per owner verb: `key`, `link`,
    /// `build-only`.
    pub signing: &'static [(&'static str, &'static [&'static str])],
    /// Verbs from [`VERBS`] that work.
    pub verbs: &'static [&'static str],
}
impl Caps {
    /// The owner signing modes that work for one owner verb.
    pub(crate) fn signing_modes(&self, verb: &str) -> &'static [&'static str] {
        self.signing
            .iter()
            .find(|(v, _)| *v == verb)
            .map(|(_, modes)| *modes)
            .unwrap_or(&[])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Exec,
    Eval,
}
impl Mode {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Mode::Exec => "exec",
            Mode::Eval => "eval",
        }
    }
}

/// An agent request built from the `exec`/`eval` flags.
pub(crate) struct ExecRequest {
    pub mode: Mode,
    pub op: String,
    pub to: String,
    pub amount: Amount,
    pub token_out: Option<Amount>,
    pub args: Option<serde_json::Value>,
    pub request_id: String,
    pub wait: Duration,
    pub build_only: bool,
    pub wallet: Option<String>,
}

/// What an agent request runs under: the wallet and the policy that the rail
/// resolves from the selection and its installed profile. The journal binds a
/// request ID to it, so a retry cannot run under another wallet or policy.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Binding {
    pub wallet: Option<String>,
    pub policy: Option<String>,
}

/// How an owner transaction is signed.
pub(crate) enum SignWith {
    /// Print the owner signing link (the default).
    Link,
    /// Sign with a local key (testnet and development).
    Key(Key),
    /// Print the unsigned transaction.
    BuildOnly,
}
impl SignWith {
    pub(crate) fn mode(&self) -> &'static str {
        match self {
            SignWith::Link => "link",
            SignWith::Key(_) => "key",
            SignWith::BuildOnly => "build-only",
        }
    }
}
pub(crate) struct Signing {
    pub with: SignWith,
    /// How long to wait for the owner. No adapter serves it yet, so
    /// [`require_signing`] refuses it with exit 3.
    pub wait: Option<Duration>,
    /// The owner operation's request ID; a retry reuses it.
    pub request_id: Option<String>,
    /// An explicitly additional fund or withdraw after an uncertain one.
    pub additional: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WalletKind {
    Simple,
    Common,
    Custom,
}

pub(crate) struct WalletCreate {
    pub kind: Option<WalletKind>,
    pub asset: String,
    /// The initial deposit, where the rail funds the wallet at creation.
    pub fund: Option<Amount>,
    pub policy: std::path::PathBuf,
    pub owner: String,
    pub agent: Key,
    pub x402: bool,
    pub signing: Signing,
}

/// Read by adapters that serve `wallet update`; no rail serves it yet.
#[allow(dead_code)]
pub(crate) struct WalletUpdate {
    pub owner: Option<String>,
    pub agent: Option<Key>,
    pub x402: Option<bool>,
    pub signing: Signing,
}

/// One `KEY=VALUE` policy parameter.
#[derive(Debug)]
pub(crate) struct Param {
    pub key: String,
    /// The value as given. Read by adapters whose keys are not amounts.
    #[allow(dead_code)]
    pub value: String,
    /// The parsed amount for an amount-kind key.
    pub amount: Option<Amount>,
}

/// A paid call: `call SERVICE|URL`.
pub(crate) struct CallRequest {
    pub target: String,
    pub input: Option<serde_json::Value>,
    pub max_amount: Option<String>,
    pub request_id: Option<String>,
}

/// A rail adapter. Defaults exit 3: the verb is not available on this rail.
pub(crate) trait Rail {
    fn info(&self) -> &'static RailInfo;
    fn tokens(&self) -> &'static [Token];
    fn caps(&self) -> Caps;
    fn parse_amount(&self, text: &str, usage: Use) -> Result<Amount> {
        asset::parse(self.info().family, self.tokens(), text, usage)
    }
    fn na(&self, verb: &str) -> Error {
        not_available(verb, self.info().id)
    }
    /// Resolves the wallet and policy that an agent request runs under, from
    /// the selected wallet (if any) and the rail's installed profile. Refuses
    /// a selection that the request path cannot confirm. Runs before a
    /// request ID is spent; `status` also runs it to check the journal.
    fn exec_binding(&self, _: &mut Ctx, wallet: Option<&str>) -> Result<Binding> {
        Ok(Binding {
            wallet: wallet.map(str::to_owned),
            policy: None,
        })
    }
    /// Settings that every current command on this rail runs with, in place
    /// of environment values that the user did not set.
    fn settings(&self, _: &mut Ctx) -> Result<Vec<(&'static str, String)>> {
        Ok(vec![])
    }
    /// Builds a policy source for this rail: the generated Rust source.
    fn policy_build(&self, _: &str) -> Result<String> {
        Err(self.na("policy build"))
    }
    /// A hidden command of earlier releases: VERB names it, ARGS are its words.
    fn legacy(&self, _: &mut Ctx, verb: &str, _: &[String]) -> Result<i32> {
        Err(self.na(verb))
    }
    fn keys_fund(&self, _: &mut Ctx, _: &Key) -> Result<i32> {
        Err(self.na("keys fund"))
    }
    /// Imports an executor bundle (a JSON object) under a key name.
    fn keys_import(&self, _: &mut Ctx, _: &str, _: &Path, _: &serde_json::Value) -> Result<i32> {
        Err(self.na("keys import of an executor bundle"))
    }
    fn wallet_create(&self, _: &mut Ctx, _: &WalletCreate) -> Result<i32> {
        Err(self.na("wallet create"))
    }
    fn wallet_show(&self, _: &mut Ctx, _: Option<&str>) -> Result<i32> {
        Err(self.na("wallet show"))
    }
    fn wallet_fund(&self, _: &mut Ctx, _: &Amount, _: Option<&Key>, _: &Signing) -> Result<i32> {
        Err(self.na("wallet fund"))
    }
    fn wallet_withdraw(
        &self,
        _: &mut Ctx,
        _: &Amount,
        _: Option<&str>,
        _: &Signing,
    ) -> Result<i32> {
        Err(self.na("wallet withdraw"))
    }
    fn wallet_update(&self, _: &mut Ctx, _: &WalletUpdate) -> Result<i32> {
        Err(self.na("wallet update"))
    }
    fn wallet_pause(&self, _: &mut Ctx, _: &Signing) -> Result<i32> {
        Err(self.na("wallet pause"))
    }
    fn wallet_resume(&self, _: &mut Ctx, _: &Signing) -> Result<i32> {
        Err(self.na("wallet resume"))
    }
    fn wallet_close(&self, _: &mut Ctx, _: &Signing) -> Result<i32> {
        Err(self.na("wallet close"))
    }
    fn policy_new(&self, _: &mut Ctx, _: &str, _: Option<&Path>) -> Result<i32> {
        Err(self.na("policy new"))
    }
    fn policy_show(&self, _: &mut Ctx, _: bool) -> Result<i32> {
        Err(self.na("policy show"))
    }
    fn policy_deploy(&self, _: &mut Ctx, _: &Path, _: Option<bool>, _: &Signing) -> Result<i32> {
        Err(self.na("policy deploy"))
    }
    fn policy_params(&self, _: &mut Ctx) -> Result<i32> {
        Err(self.na("policy params"))
    }
    fn policy_params_set(&self, _: &mut Ctx, _: &[Param], _: &Signing) -> Result<i32> {
        Err(self.na("policy params set"))
    }
    fn exec(&self, _: &mut Ctx, _: &ExecRequest) -> Result<i32> {
        Err(self.na("exec"))
    }
    fn eval(&self, _: &mut Ctx, _: &ExecRequest) -> Result<i32> {
        Err(self.na("eval"))
    }
    fn status(&self, _: &mut Ctx, _: &str, _: Option<&str>, _: Duration) -> Result<i32> {
        Err(self.na("status"))
    }
    fn tx_send(&self, _: &mut Ctx, _: &[u8]) -> Result<i32> {
        Err(self.na("tx send"))
    }
    /// A rail-native paid call. `None` sends the call through the engine
    /// gateway instead.
    fn call(&self, _: &mut Ctx, _: &CallRequest) -> Result<Option<i32>> {
        Ok(None)
    }
    /// Paid services through the engine gateway: the catalog, `call SERVICE`,
    /// `curl`, `http` and the MCP tools. The engine serves no paid-service
    /// route yet, so no adapter overrides this.
    fn gateway(&self, verb: &str) -> Result<()> {
        Err(self.na(verb))
    }
}

/// Points stderr back at the terminal where an adapter silenced a rail
/// crate's own progress output.
pub(crate) fn restore_stderr() {
    tempo::restore_stderr();
}

/// One address check: the test and the expected form that its error names.
pub(crate) type Check = (fn(&str) -> bool, &'static str);

/// The address and asset formats of a chain family. Each rail defines its
/// own in `vendors/<rail>/formats.rs`; [`formats`] picks it.
pub(crate) struct Formats {
    /// An account that holds a key.
    pub account: Check,
    /// A policy wallet.
    pub wallet: Check,
    /// A transfer recipient.
    pub recipient: Check,
    /// A token identity: an issuer, mint or token address.
    pub token: Check,
    /// The decimals of an asset contract given in place of a token code, or
    /// `None` when the input is not one.
    pub asset_contract: fn(&str) -> Option<u8>,
    /// The decimals of a token that its issuer names and the rail's table
    /// does not list, when the family fixes them.
    pub issuer_decimals: Option<u8>,
}

/// The families in the order that address inference tries them.
pub(crate) const FAMILIES: &[Family] = &[Family::Stellar, Family::Tempo, Family::Solana];

/// The address and asset formats of a family.
pub(crate) fn formats(family: Family) -> &'static Formats {
    match family {
        Family::Stellar => &stellar::formats::FORMATS,
        Family::Solana => &solana::formats::FORMATS,
        Family::Tempo => &tempo::formats::FORMATS,
    }
}

/// The adapter for a rail.
pub(crate) fn rail(info: &'static RailInfo) -> Box<dyn Rail> {
    match info.family {
        Family::Stellar => Box::new(stellar::Stellar(info)),
        Family::Solana => Box::new(solana::Solana(info)),
        Family::Tempo => Box::new(tempo::Tempo(info)),
    }
}

/// Refuses an operation other than those the rail lists.
pub(crate) fn require_op(rail: &dyn Rail, op: &str) -> Result<()> {
    if rail.caps().ops.contains(&op) {
        Ok(())
    } else {
        Err(not_available(&format!("exec --op {op}"), rail.info().id))
    }
}

/// Refuses a signing mode, or `--wait`, that the rail does not serve yet.
pub(crate) fn require_signing(rail: &dyn Rail, verb: &str, signing: &Signing) -> Result<()> {
    if signing.wait.is_some() {
        return Err(not_available(&format!("{verb} --wait"), rail.info().id));
    }
    if rail
        .caps()
        .signing_modes(verb)
        .contains(&signing.with.mode())
    {
        Ok(())
    } else {
        let flag = match signing.with {
            SignWith::Link => "--sign-with-link",
            SignWith::Key(_) => "--sign-with-key",
            SignWith::BuildOnly => "--build-only",
        };
        Err(not_available(&format!("{verb} {flag}"), rail.info().id))
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::{commands::Global, routing::RAILS, term::Out};

    pub(super) fn ctx() -> Ctx {
        let dir = crate::settings::temp_dir("vendors");
        let global = Global {
            config_dir: Some(dir),
            ..Global::default()
        };
        Ctx::new(global, Out::new(false, true, false, true))
    }
    fn key() -> Key {
        Key {
            name: "k".into(),
            rail: "stellar:testnet".into(),
            address: "G".into(),
            public_key: None,
            curve: "ed25519".into(),
            secret: false,
        }
    }
    fn signing() -> Signing {
        Signing {
            with: SignWith::Link,
            wait: None,
            request_id: None,
            additional: false,
        }
    }
    /// Calls one verb with inert arguments. Only verbs a rail does not list
    /// are called, so no call reaches a network or a file.
    fn call(r: &dyn Rail, cx: &mut Ctx, verb: &str) -> Result<i32> {
        let amount = Amount {
            asset: asset::Asset::Installed,
            quantity: asset::Quantity::Units(1),
            decimals: None,
            text: "1".into(),
        };
        let request = ExecRequest {
            mode: Mode::Exec,
            op: "transfer".into(),
            to: String::new(),
            amount: amount.clone(),
            token_out: None,
            args: None,
            request_id: "request-0001".into(),
            wait: Duration::ZERO,
            build_only: false,
            wallet: None,
        };
        let p = Path::new("/nonexistent");
        match verb {
            "keys fund" => r.keys_fund(cx, &key()),
            "keys import" => r.keys_import(cx, "k", p, &serde_json::json!({})),
            "wallet create" => r.wallet_create(
                cx,
                &WalletCreate {
                    kind: None,
                    asset: "USDC".into(),
                    fund: None,
                    policy: p.into(),
                    owner: String::new(),
                    agent: key(),
                    x402: false,
                    signing: signing(),
                },
            ),
            "wallet show" => r.wallet_show(cx, None),
            "wallet fund" => r.wallet_fund(cx, &amount, None, &signing()),
            "wallet withdraw" => r.wallet_withdraw(cx, &amount, None, &signing()),
            "wallet update" => r.wallet_update(
                cx,
                &WalletUpdate {
                    owner: None,
                    agent: None,
                    x402: None,
                    signing: signing(),
                },
            ),
            "wallet pause" => r.wallet_pause(cx, &signing()),
            "wallet resume" => r.wallet_resume(cx, &signing()),
            "wallet close" => r.wallet_close(cx, &signing()),
            "policy new" => r.policy_new(cx, "prompt", None),
            "policy show" => r.policy_show(cx, false),
            "policy build" => r.policy_build("").map(|_| 0),
            "policy deploy" => r.policy_deploy(cx, p, None, &signing()),
            "policy params" => r.policy_params(cx),
            "policy params set" => r.policy_params_set(cx, &[], &signing()),
            "exec" => r.exec(cx, &request),
            "eval" => r.eval(cx, &request),
            "status" => r.status(cx, "request-0001", None, Duration::ZERO),
            "tx send" => r.tx_send(cx, b""),
            "call" => r
                .call(
                    cx,
                    &CallRequest {
                        target: "service".into(),
                        input: None,
                        max_amount: None,
                        request_id: None,
                    },
                )
                .map(|v| v.unwrap_or(0)),
            "skills" | "curl" | "http" | "mcp" => r.gateway(verb).map(|_| 0),
            _ => unreachable!("{verb}"),
        }
    }

    #[test]
    fn every_unlisted_verb_exits_3_as_not_available() {
        for info in RAILS {
            let r = rail(info);
            let caps = r.caps();
            for verb in caps.verbs {
                assert!(
                    VERBS.contains(verb),
                    "{} lists unknown verb {verb}",
                    info.id
                );
            }
            for key in caps.param_keys {
                assert!(PARAM_KEYS.contains(key), "{key}");
            }
            for (verb, modes) in caps.signing {
                assert!(
                    caps.verbs.contains(verb),
                    "{} signs unlisted {verb}",
                    info.id
                );
                for mode in *modes {
                    assert!(["key", "link", "build-only"].contains(mode), "{mode}");
                }
            }
            let mut cx = ctx();
            for verb in VERBS.iter().filter(|v| !caps.verbs.contains(v)) {
                if *verb == "call" {
                    // An unlisted rail-native call falls back to the gateway.
                    let none = r
                        .call(
                            &mut cx,
                            &CallRequest {
                                target: "s".into(),
                                input: None,
                                max_amount: None,
                                request_id: None,
                            },
                        )
                        .unwrap();
                    assert!(none.is_none(), "{}", info.id);
                    continue;
                }
                let error = call(r.as_ref(), &mut cx, verb).unwrap_err();
                assert_eq!(error.code, 3, "{} {verb}: {}", info.id, error.message);
                assert!(
                    error.message.contains("not available on") && error.message.contains(info.id),
                    "{} {verb}: {}",
                    info.id,
                    error.message
                );
            }
        }
    }

    #[test]
    fn unlisted_ops_and_signing_modes_exit_3() {
        for info in RAILS {
            let r = rail(info);
            for op in ["swap", "deposit", "redeem", "custom"] {
                assert_eq!(require_op(r.as_ref(), op).unwrap_err().code, 3);
            }
            let build = Signing {
                with: SignWith::BuildOnly,
                wait: None,
                request_id: None,
                additional: false,
            };
            let wait = Signing {
                with: SignWith::Link,
                wait: Some(Duration::from_secs(300)),
                request_id: None,
                additional: false,
            };
            assert_eq!(
                require_signing(r.as_ref(), "wallet create", &wait)
                    .unwrap_err()
                    .code,
                3
            );
            if !r
                .caps()
                .signing_modes("wallet pause")
                .contains(&"build-only")
            {
                assert_eq!(
                    require_signing(r.as_ref(), "wallet pause", &build)
                        .unwrap_err()
                        .code,
                    3
                );
            }
        }
    }
}
