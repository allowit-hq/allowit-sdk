//! Native adapter cases for common input dispatch.
mod address {
    use crate::input::address::*;
    const G: &str = "GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5";
    #[test]
    fn formats_match_their_family_only() {
        let native = allowit_stellar_wallet::asset_from_identity("XLM", None).unwrap();
        let contract = allowit_stellar_wallet::sac_address("stellar:testnet", &native).unwrap();
        #[allow(non_snake_case)]
        let C = contract.as_str();
        check_account(crate::routing::info("stellar:testnet").unwrap().family, G).unwrap();
        check_wallet(crate::routing::info("stellar:testnet").unwrap().family, C).unwrap();
        check_wallet(crate::routing::info("stellar:testnet").unwrap().family, G).unwrap();
        check_recipient(crate::routing::info("stellar:testnet").unwrap().family, C).unwrap();
        assert!(check_account(crate::routing::info("stellar:testnet").unwrap().family, C).is_err());
        assert!(
            check_account(
                crate::routing::info("stellar:testnet").unwrap().family,
                &G.replace('A', "B")
            )
            .is_err()
        );
    }
    #[test]
    fn stellar_wallets_compare_case_sensitively() {
        assert!(same_wallet(
            crate::routing::info("stellar:testnet").unwrap().family,
            G,
            G
        ));
        assert!(!same_wallet(
            crate::routing::info("stellar:testnet").unwrap().family,
            G,
            &G.to_lowercase()
        ));
    }
}

mod asset {
    use crate::input::asset::*;
    use crate::routing::Family;
    const STELLAR: &[Token] = &[
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
    fn units(family: Family, tokens: &[Token], s: &str) -> u128 {
        parse(family, tokens, s, Use::Spend).unwrap().units()
    }
    #[test]
    fn units_parse_with_stellar_decimals() {
        assert_eq!(
            units(
                crate::routing::info("stellar:testnet").unwrap().family,
                STELLAR,
                "XLM:5"
            ),
            50_000_000
        );
        assert_eq!(
            units(
                crate::routing::info("stellar:testnet").unwrap().family,
                STELLAR,
                "USDC:0.25"
            ),
            2_500_000
        );
        assert_eq!(decimal_units("0.000001", 6).unwrap(), 1);
    }
    #[test]
    fn native_and_xlm_name_the_same_asset() {
        let a = parse(
            crate::routing::info("stellar:testnet").unwrap().family,
            STELLAR,
            "native:1",
            Use::Spend,
        )
        .unwrap();
        let b = parse(
            crate::routing::info("stellar:testnet").unwrap().family,
            STELLAR,
            "XLM:1",
            Use::Spend,
        )
        .unwrap();
        assert_eq!(a.asset, b.asset);
        assert_eq!(a.units(), b.units());
        assert_eq!(a.asset, Asset::Native { code: "XLM".into() });
    }
    #[test]
    fn issuer_contract_and_installed_forms() {
        let issued = parse(
            crate::routing::info("stellar:testnet").unwrap().family,
            STELLAR,
            "EURC:GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5:1.5",
            Use::Spend,
        )
        .unwrap();
        assert_eq!(issued.units(), 15_000_000);
        assert!(matches!(issued.asset, Asset::Token { ref issuer, .. } if issuer.is_some()));
        let native = allowit_stellar_wallet::asset_from_identity("XLM", None).unwrap();
        let sac = allowit_stellar_wallet::sac_address("stellar:testnet", &native).unwrap();
        let contract = parse(
            crate::routing::info("stellar:testnet").unwrap().family,
            STELLAR,
            &format!("{sac}:2"),
            Use::Spend,
        )
        .unwrap();
        assert_eq!(contract.asset, Asset::Contract(sac.clone()));
        assert_eq!(contract.units(), 20_000_000);
        let installed = parse(
            crate::routing::info("stellar:testnet").unwrap().family,
            STELLAR,
            "100",
            Use::Spend,
        )
        .unwrap();
        assert_eq!(installed.asset, Asset::Installed);
        assert_eq!(installed.decimals, None);
    }
    #[test]
    fn minimum_and_bad_input() {
        let huge = decimal_units(&u128::MAX.to_string(), 7).unwrap_err();
        assert_eq!(huge.code, 2);
        assert!(huge.message.contains("is too large"), "{}", huge.message);
        assert!(
            parse(
                crate::routing::info("stellar:testnet").unwrap().family,
                STELLAR,
                "XLM:max",
                Use::Spend
            )
            .is_err()
        );
        assert!(
            parse(
                crate::routing::info("stellar:testnet").unwrap().family,
                STELLAR,
                "0",
                Use::Spend
            )
            .is_err()
        );
        let min = parse(
            crate::routing::info("stellar:testnet").unwrap().family,
            STELLAR,
            "USDC:0",
            Use::Minimum,
        )
        .unwrap();
        assert_eq!(min.units(), 0);
        assert!(
            parse(
                crate::routing::info("stellar:testnet").unwrap().family,
                STELLAR,
                "10",
                Use::Minimum
            )
            .is_err()
        );
        for bad in [
            "",
            "0",
            "XLM:0",
            "XLM:-1",
            "XLM:1e3",
            "XLM:01",
            "XLM:1.",
            "XLM:.5",
            "XLM:0.00000001",
            "DOGE:1",
            "a:b:c:d",
            "XLM:G:1",
            "USDC:1,5",
        ] {
            assert_eq!(
                parse(
                    crate::routing::info("stellar:testnet").unwrap().family,
                    STELLAR,
                    bad,
                    Use::Spend
                )
                .unwrap_err()
                .code,
                2,
                "{bad}"
            );
        }
    }
}

#[test]
fn every_registered_alias_uses_the_canonical_network_asset_defaults() {
    for network in crate::routing::all() {
        let expected = crate::execution::tokens(network.id).unwrap();
        for input in std::iter::once(network.id).chain(network.aliases.iter().copied()) {
            assert_eq!(
                crate::execution::tokens(input).unwrap(),
                expected,
                "{input}"
            );
            assert_eq!(
                crate::input::asset::resolve(input, "USDC", None, true).unwrap(),
                crate::input::asset::resolve(network.id, "USDC", None, true).unwrap(),
                "{input}"
            );
        }
    }
}

#[test]
fn registered_protocol_and_presentation_aliases_preserve_the_existing_inventory() {
    for native in allowit_stellar_wallet::networks::NETWORKS {
        let registered = crate::routing::info(native.id).unwrap();
        let presentation = if native.testnet {
            "stellar-testnet"
        } else {
            "stellar-mainnet"
        };
        let expected: Vec<_> = native
            .aliases
            .iter()
            .copied()
            .chain(std::iter::once(presentation))
            .collect();
        assert_eq!(registered.aliases, expected);
        for alias in expected {
            assert_eq!(crate::routing::normalize(alias).unwrap(), native.id);
        }
    }
    assert_eq!(
        crate::routing::info("stellar:pubnet").unwrap().aliases,
        &["stellar:mainnet", "stellar-mainnet"]
    );
    assert_eq!(
        crate::routing::info("stellar:testnet").unwrap().aliases,
        &["stellar-testnet"]
    );
}
