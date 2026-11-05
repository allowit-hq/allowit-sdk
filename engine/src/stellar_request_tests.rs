#[cfg(all(test, feature = "stellar"))]
mod tests {
    use crate::execution::Request as ExecRequest;
    use crate::input::asset::{exec_uses, parse};
    use crate::typed_request::operation_nonce;
    use crate::vendors::stellar::request::{build, rebuild, state};
    use allowit_stellar_wallet::workflow::ExecutionRequest;
    use allowit_stellar_wallet::{self as sdk, WalletConfig, WalletRoute};
    use std::time::Duration;

    fn config(route: WalletRoute, usdc: bool) -> WalletConfig {
        let owner = sdk::account_for_seed(&[1; 32]);
        let source = "use allowit::prelude::*; pub fn execute(ctx: &AllowitContext) -> allowit::Result { Ok(()) }".to_string();
        let mut c = WalletConfig {
            network: "stellar:testnet".into(),
            policy_instance: [7; 32],
            wallet: owner.clone(),
            owner,
            agent: sdk::account_for_seed(&[2; 32]),
            server: sdk::account_for_seed(&[3; 32]),
            route: route.clone(),
            tier: (route == WalletRoute::PolicyWallet).then_some(sdk::PolicyTier::Simple),
            language: sdk::PolicyLanguage::Portable,
            rules: vec![],
            params: Default::default(),
            source_digest: [0; 32],
            asset: String::new(),
            asset_symbol: if usdc { "USDC" } else { "XLM" }.into(),
            asset_issuer: usdc.then(|| {
                let tokens = crate::execution::tokens("stellar:testnet").unwrap();
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
        let tokens = crate::execution::tokens("stellar:testnet").unwrap();
        let (spend, minimum) = exec_uses(op);
        ExecRequest {
            op: op.into(),
            to: to.into(),
            amount: parse(
                crate::routing::info("stellar:testnet").unwrap().family,
                tokens,
                amount,
                spend,
            )
            .unwrap(),
            token_out: token_out.map(|t| {
                parse(
                    crate::routing::info("stellar:testnet").unwrap().family,
                    tokens,
                    t,
                    minimum,
                )
                .unwrap()
            }),
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
    fn fee_payer(r: &ExecutionRequest) -> allowit_stellar_wallet::workflow::AccountId {
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
}
