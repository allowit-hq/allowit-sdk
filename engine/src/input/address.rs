//! Address format checks. A wallet is identified only by its address, so
//! every command checks that an address matches the rail. Each rail defines
//! its formats in `vendors/<rail>/formats.rs`.
use super::{Check, formats};
use crate::{
    error::{Error, Result, quoted},
    routing::Family,
};

fn check(what: &str, family: Family, (ok, expected): Check, s: &str) -> Result<()> {
    if ok(s) {
        return Ok(());
    }
    Err(Error::usage(format!(
        "{} is not a {} {what} ({expected})",
        quoted(s),
        family.name()
    )))
}

/// A Stellar G account.
pub fn check_account(family: Family, s: &str) -> Result<()> {
    check("account", family, formats(family)?.account, s)
}

/// A Stellar G classic account or C policy contract.
pub fn check_wallet(family: Family, s: &str) -> Result<()> {
    check("wallet address", family, formats(family)?.wallet, s)
}

/// A Stellar G or C transfer recipient.
pub fn check_recipient(family: Family, s: &str) -> Result<()> {
    check("recipient", family, formats(family)?.recipient, s)
}

/// A Stellar G issuer or C token contract.
pub fn check_token(family: Family, s: &str) -> Result<()> {
    check("token", family, formats(family)?.token, s)
}

/// Whether two case-sensitive Stellar addresses name the same wallet.
pub fn same_wallet(_family: Family, a: &str, b: &str) -> bool {
    a == b
}

#[cfg(all(test, feature = "stellar"))]
mod tests {
    use super::*;
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
