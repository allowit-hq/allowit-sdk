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
        sdk::test_support::request_config(route, usdc)
    }
    fn vault(n: u8) -> String {
        sdk::test_support::vault(n)
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
    #[test]
    fn build_only_never_opens_the_sdk_provider_journal() {
        let fixtures = sdk::test_support::portable_descriptors();
        let descriptor = serde_json::value::RawValue::from_string(
            serde_json::to_string(&fixtures[0]["bundle"]).unwrap(),
        )
        .unwrap();
        let (description, binding) =
            crate::vendors::decode_profile("stellar:testnet", descriptor.get().as_bytes()).unwrap();
        let profile = crate::profiles::Profile {
            version: "build-only".into(),
            inner: allowit_core::vendor::profiles::Profile {
                description,
                binding,
                descriptor,
                backend: "http://localhost:8080".into(),
                authority: None,
            },
        };
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("unopened");
        let request = request("transfer", &sdk::account_for_seed(&[6; 32]), "1", None);
        let wire = crate::execution::build(&profile, &root, None, &request).unwrap();
        assert_eq!(wire["requestId"], request.request_id);
        assert!(
            !root.exists(),
            "A build-only request must not invoke the host store opener"
        );
    }
}
