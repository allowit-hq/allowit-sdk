//! Builds the typed request of one `exec` operation with the request builders
//! of `allowit-stellar-wallet`. A transfer moves the wallet's asset to a G or C
//! address. A policy wallet also deposits its asset into a SEP-56 vault for a
//! minimum of shares, and redeems vault shares for a minimum of its asset.
use crate::{
    error::{Error, Result},
    typed_request::operation_nonce,
    vendors::{
        ExecRequest,
        asset::{Amount, Asset},
        typed::TypedRequest,
    },
};
use allowit_sdk::typed_workflow::{AccountKind, ExecutionRequest};
use allowit_stellar_wallet::{self as sdk, AccountState, TrustedState, WalletConfig, WalletRoute};

/// Checks the operation flags alone, before any profile, journal or network
/// access.
pub(crate) fn check(op: &str, to: &str, token_out: Option<&Amount>, args: bool) -> Result<()> {
    if args {
        return Err(Error::usage("--args applies to custom operations"));
    }
    let vault = sdk::request::account(to).is_ok_and(|a| a.kind == AccountKind::Contract);
    match (op, token_out) {
        ("transfer", None) => Ok(()),
        ("transfer", Some(_)) => Err(Error::usage(
            "--token-out applies to swap, deposit and redeem, not transfer",
        )),
        ("deposit" | "redeem", _) if !vault => Err(Error::usage(format!(
            "a {op} goes to a C… vault contract, not {to}"
        ))),
        ("deposit", None) => Err(Error::usage(
            "a deposit needs --token-out MIN_SHARES, the fewest vault shares to receive",
        )),
        ("redeem", None) => Err(Error::usage(
            "a redeem needs --token-out ASSET:MIN, the least of the wallet asset to receive",
        )),
        ("redeem", Some(min)) if min.units() == Some(0) => Err(Error::usage(
            "a redeem needs a positive --token-out minimum",
        )),
        ("deposit" | "redeem", Some(_)) => Ok(()),
        _ => Err(Error::unsupported(format!(
            "exec --op {op} is not available on Stellar yet"
        ))),
    }
}

/// The typed request of one checked operation. Amounts are checked against the
/// installed asset before `state` is read.
pub(crate) fn build(
    policy_id: &str,
    config: &WalletConfig,
    r: &ExecRequest,
    state: impl FnOnce() -> Result<TrustedState>,
) -> Result<TypedRequest> {
    let minimum = || {
        r.token_out
            .as_ref()
            .ok_or_else(|| Error::usage(format!("a {} needs --token-out", r.op)))
    };
    if config.route == WalletRoute::Native && r.op != "transfer" {
        return Err(Error::unsupported(format!(
            "a classic Stellar wallet only transfers; a {} needs a policy wallet",
            r.op
        )));
    }
    let (amount, min) = match r.op.as_str() {
        "transfer" => (installed_units(config, &r.amount)?, 0),
        "deposit" => (installed_units(config, &r.amount)?, shares(minimum()?)?),
        "redeem" => (shares(&r.amount)?, installed_units(config, minimum()?)?),
        op => {
            return Err(Error::unsupported(format!(
                "exec --op {op} is not available on Stellar yet"
            )));
        }
    };
    let state = state()?;
    let id = operation_nonce(policy_id, &r.request_id);
    let request = match r.op.as_str() {
        "transfer" => sdk::request::transfer(config, &state, id, &r.to, amount),
        "deposit" => sdk::request::deposit(config, &state, id, &r.to, amount, min),
        _ => sdk::request::redeem(config, &state, id, &r.to, amount, min),
    }
    .map_err(|e| Error::usage(format!("the {} does not fit this wallet: {e}", r.op)))?;
    TypedRequest::new(policy_id, &r.request_id, request)
}

/// The request of a retried request ID: rebuilt from its flags with the
/// retained fee bounds. The fee payer was chain state at the first attempt, so
/// the journal compares only the operator's intent, and a retry reads no RPC.
pub(crate) fn rebuild(
    policy_id: &str,
    config: &WalletConfig,
    r: &ExecRequest,
    retained: &ExecutionRequest,
) -> Result<TypedRequest> {
    let mut typed = build(policy_id, config, r, || Ok(profile_state(config)))?;
    typed.request.fee_bounds.clone_from(&retained.fee_bounds);
    Ok(typed)
}

/// The chain state that the builders read: the fee payer. A policy wallet pays
/// fees from its current agent account, which Stellar RPC reports. Classic
/// custody pays them from the wallet and the builders read no state for it, so
/// the profile fills it; the engine checks every request against its own.
pub(crate) fn state(c: &WalletConfig, rpc_url: Option<&str>) -> Result<TrustedState> {
    if c.route == WalletRoute::Native {
        return Ok(profile_state(c));
    }
    let url = rpc_url.filter(|u| !u.is_empty()).ok_or_else(|| {
        Error::config(
            "a policy wallet request names its current agent account from Stellar RPC; pass --rpc-url URL or set rpc_url for this rail",
        )
    })?;
    let rpc = sdk::rpc::HttpRpc::new(url).map_err(Error::config)?;
    sdk::rpc::trusted_state_for_discovery(&rpc, c, None)
        .map(|found| found.state)
        .map_err(Error::config)
}

/// The profile's view of the state: the configured agent account pays.
fn profile_state(c: &WalletConfig) -> TrustedState {
    TrustedState {
        contract_version: None,
        policy_state: None,
        source: AccountState {
            address: c.agent.clone(),
            sequence: 0,
            master_weight: 0,
            low_threshold: 0,
            medium_threshold: 0,
            high_threshold: 0,
            signers: vec![],
        },
        nonce: 0,
        deployment_version: 0,
        spent_daily: 0,
        spent_total: 0,
        spent_day: 0,
        calls: 0,
        ledger: 0,
        module_digest: c.module_digest,
        source_hash: c.source_hash,
        owner: c.owner.clone(),
        agent: c.agent.clone(),
        server: c.server.clone(),
        policy_instance: c.policy_instance,
    }
}

/// Checks that the amount names the wallet's installed asset and returns its
/// base units.
pub(crate) fn installed_units(config: &WalletConfig, amount: &Amount) -> Result<i128> {
    let matches = match &amount.asset {
        Asset::Installed => true,
        Asset::Native { .. } => config.asset_symbol == "XLM" && config.asset_issuer.is_none(),
        Asset::Token { code, issuer } => {
            *code == config.asset_symbol
                && issuer
                    .as_deref()
                    .is_none_or(|i| Some(i) == config.asset_issuer.as_deref())
        }
        Asset::Contract(contract) => *contract == config.asset,
    };
    if !matches {
        return Err(Error {
            code: 4,
            message: format!(
                "the wallet holds only its installed asset {}; {} names another asset",
                config.asset_symbol, amount.text
            ),
            status: None,
            config: false,
        });
    }
    units(amount)
}

/// Vault shares: INTEGER base units, never a named asset.
fn shares(amount: &Amount) -> Result<i128> {
    if amount.asset != Asset::Installed {
        return Err(Error::usage(format!(
            "{} is not a share amount; vault shares are INTEGER base units",
            amount.text
        )));
    }
    units(amount)
}

fn units(amount: &Amount) -> Result<i128> {
    let units = amount
        .units()
        .ok_or_else(|| Error::usage("the operation needs an exact amount"))?;
    i128::try_from(units).map_err(|_| Error::usage("the amount is too large"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        commands::{Global, exec::RequestArgs},
        ctx::Ctx,
        routing::Family,
        term::Out,
        vendors::{
            Mode,
            asset::{exec_uses, parse},
        },
    };
    use std::time::Duration;

    fn config(route: WalletRoute, usdc: bool) -> WalletConfig {
        let owner = sdk::account_for_seed(&[1; 32]);
        let source = "use allowit::v1::prelude::*; async fn _execute(ctx: &Context) -> PolicyResult { Ok(()) }".to_string();
        let mut c = WalletConfig {
            network: "stellar:testnet".into(),
            policy_instance: [7; 32],
            wallet: owner.clone(),
            owner,
            agent: sdk::account_for_seed(&[2; 32]),
            server: sdk::account_for_seed(&[3; 32]),
            route: route.clone(),
            tier: (route == WalletRoute::PolicyWallet).then_some(sdk::PolicyTier::Simple),
            language: Default::default(),
            rules: vec![],
            params: Default::default(),
            source_digest: [0; 32],
            asset: String::new(),
            asset_symbol: if usdc { "USDC" } else { "XLM" }.into(),
            asset_issuer: usdc.then(|| {
                let tokens = super::super::tokens_for("stellar:testnet");
                tokens[1].issuer.unwrap().to_owned()
            }),
            source_hash: sdk::hash(&source),
            policy_source: source,
            module_digest: [5; 32],
            max_fee_stroops: 50_000,
        };
        c.asset = sdk::sac_address(&c.network, &sdk::asset_xdr(&c).unwrap()).unwrap();
        if route == WalletRoute::PolicyWallet {
            c.wallet = vault(0);
            // Policy wallets run portable source with an empty Simple binding.
            let source = "use allowit::prelude::*;\npub fn execute(ctx: &AllowitContext) -> allowit::Result {\n    Ok(())\n}\n";
            c.language = sdk::PolicyLanguage::Portable;
            c.policy_source = source.into();
            c.source_hash = sdk::hash(source);
            c.source_digest = sdk::policy::installed_source_digest(
                sdk::PolicyTier::Simple,
                &c.module_digest,
                &c.asset,
                &[],
                source,
            )
            .unwrap();
        }
        c
    }
    /// A contract strkey that names no asset of the test configurations.
    fn vault(n: u8) -> String {
        let asset = sdk::asset_from_identity("VLT", Some(&sdk::account_for_seed(&[20 + n; 32])));
        sdk::sac_address("stellar:testnet", &asset.unwrap()).unwrap()
    }
    fn request(op: &str, to: &str, amount: &str, token_out: Option<&str>) -> ExecRequest {
        let tokens = super::super::tokens_for("stellar:testnet");
        let (spend, minimum) = exec_uses(op);
        ExecRequest {
            mode: Mode::Exec,
            op: op.into(),
            to: to.into(),
            amount: parse(Family::Stellar, tokens, amount, spend).unwrap(),
            token_out: token_out.map(|t| parse(Family::Stellar, tokens, t, minimum).unwrap()),
            args: None,
            request_id: "request-0001".into(),
            wait: Duration::ZERO,
            build_only: true,
            wallet: None,
        }
    }

    #[test]
    fn exec_flags_build_requests_that_decode_to_the_same_operation() {
        let to = sdk::account_for_seed(&[6; 32]);
        let usdc = config(WalletRoute::PolicyWallet, true);
        // The policy wallet's current agent account differs from the profile
        // agent; it pays the fee.
        let mut state = state(&config(WalletRoute::Native, false), None).unwrap();
        state.source.address = sdk::account_for_seed(&[8; 32]);
        let cases = [
            (
                config(WalletRoute::Native, false),
                request("transfer", &to, "XLM:0.1", None),
            ),
            (usdc.clone(), request("transfer", &to, "USDC:0.1", None)),
            (
                usdc.clone(),
                request("deposit", &vault(1), "USDC:5", Some("4900000")),
            ),
            (
                usdc.clone(),
                request("redeem", &vault(1), "2000000", Some("USDC:0.19")),
            ),
        ];
        for (c, r) in cases {
            let typed = build("policy-1", &c, &r, || Ok(state.clone())).unwrap();
            let expected = match r.op.as_str() {
                "transfer" => sdk::StellarOperation::Transfer(sdk::Transfer {
                    token: c.asset.clone(),
                    source: c.wallet.clone(),
                    recipient: to.clone(),
                    amount: 1_000_000,
                    native: c.route == WalletRoute::Native,
                }),
                "deposit" => sdk::StellarOperation::Deposit(sdk::Deposit {
                    vault: vault(1),
                    assets: 50_000_000,
                    min_shares: 4_900_000,
                }),
                _ => sdk::StellarOperation::Redeem(sdk::Redeem {
                    vault: vault(1),
                    shares: 2_000_000,
                    min_assets: 1_900_000,
                }),
            };
            assert_eq!(
                sdk::operation_from_request(&c, &typed.request).unwrap(),
                expected
            );
            let payer = if c.route == WalletRoute::Native {
                &c.wallet
            } else {
                &state.source.address
            };
            assert_eq!(
                fee_payer(&typed.request),
                sdk::request::account(payer).unwrap()
            );
            assert_eq!(
                typed.request.operation.nonce,
                operation_nonce("policy-1", "request-0001")
            );
        }
    }
    fn fee_payer(r: &ExecutionRequest) -> allowit_sdk::typed_workflow::AccountId {
        r.fee_bounds.as_ref().unwrap()[0].payer.clone()
    }

    #[test]
    fn a_retry_keeps_the_retained_fee_payer() {
        let c = config(WalletRoute::PolicyWallet, true);
        let r = request("deposit", &vault(1), "USDC:5", Some("4900000"));
        let mut first = state(&config(WalletRoute::Native, false), None).unwrap();
        first.source.address = sdk::account_for_seed(&[8; 32]);
        let old = build("policy-1", &c, &r, || Ok(first)).unwrap().request;
        // The agent account changed since; the same flags rebuild the same request.
        assert_eq!(rebuild("policy-1", &c, &r, &old).unwrap().request, old);
        let more = request("deposit", &vault(1), "USDC:6", Some("4900000"));
        assert_ne!(rebuild("policy-1", &c, &more, &old).unwrap().request, old);
    }

    #[test]
    fn a_classic_wallet_moves_only_its_installed_asset() {
        let c = config(WalletRoute::Native, false);
        let xlm = request("transfer", "G", "XLM:5", None).amount;
        assert_eq!(installed_units(&c, &xlm).unwrap(), 50_000_000);
        let usdc = request("transfer", "G", "USDC:1", None).amount;
        assert_eq!(installed_units(&c, &usdc).unwrap_err().code, 4);
        // Vault operations need a policy wallet: refused before any state read.
        let r = request("deposit", &vault(1), "XLM:5", Some("4900000"));
        let error = build("policy-1", &c, &r, || unreachable!()).unwrap_err();
        assert_eq!(error.code, 3, "{}", error.message);
    }

    #[test]
    fn a_vault_operation_without_a_vault_or_minimum_stops_before_the_journal() {
        let dir = crate::settings::temp_dir("vault-flags");
        let global = Global {
            config_dir: Some(dir.clone()),
            rail: Some("stellar:testnet".into()),
            ..Global::default()
        };
        let mut cx = Ctx::new(global, Out::new(false, true, false, true));
        let account = sdk::account_for_seed(&[6; 32]);
        for (op, to, amount, token_out, needle) in [
            (
                "redeem",
                account.as_str(),
                "2000000",
                Some("USDC:1"),
                "vault contract",
            ),
            ("redeem", &vault(1), "2000000", None, "--token-out"),
            ("deposit", &vault(1), "USDC:1", None, "--token-out"),
            (
                "redeem",
                &vault(1),
                "USDC:1",
                Some("USDC:1"),
                "share amount",
            ),
        ] {
            let args = RequestArgs {
                op: op.into(),
                to: to.into(),
                amount: amount.into(),
                token_out: token_out.map(Into::into),
                args: None,
                request_id: Some("request-0001".into()),
                wait: Duration::ZERO,
            };
            let error =
                crate::commands::exec::submit(Mode::Exec, args, false, &mut cx).unwrap_err();
            assert_eq!(error.code, 2, "{op} {to}: {}", error.message);
            assert!(error.message.contains(needle), "{}", error.message);
        }
        // No journal, profile or engine was read: the config directory is empty.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }
}
