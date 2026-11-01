//! Address format checks. A wallet is identified only by its address, so
//! every command checks that an address matches the rail. Each rail defines
//! its formats in `vendors/<rail>/formats.rs`.
use super::{Check, FAMILIES, formats};
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

/// An account that holds a key: a Stellar G account, a Solana base58 key or
/// a Tempo 0x account.
pub(crate) fn check_account(family: Family, s: &str) -> Result<()> {
    check("account", family, formats(family).account, s)
}

/// A policy wallet: a Stellar C contract, a Solana vault address or a Tempo
/// 0x vault.
pub(crate) fn check_wallet(family: Family, s: &str) -> Result<()> {
    check("wallet address", family, formats(family).wallet, s)
}

/// A transfer recipient: a Stellar G or C address, a Solana base58 address or
/// a Tempo 0x address.
pub(crate) fn check_recipient(family: Family, s: &str) -> Result<()> {
    check("recipient", family, formats(family).recipient, s)
}

/// A token identity: a Stellar G issuer or C contract, a Solana mint or a
/// Tempo 0x token.
pub(crate) fn check_token(family: Family, s: &str) -> Result<()> {
    check("token", family, formats(family).token, s)
}

/// Whether two wallet addresses name the same wallet. Tempo 0x addresses
/// ignore hex case; Stellar and Solana addresses are case-sensitive.
pub(crate) fn same_wallet(family: Family, a: &str, b: &str) -> bool {
    match family {
        Family::Tempo => a.eq_ignore_ascii_case(b),
        Family::Stellar | Family::Solana => a == b,
    }
}

/// The family that an address format names, when it names exactly one.
pub(crate) fn infer_family(s: &str) -> Option<Family> {
    FAMILIES.iter().copied().find(|family| {
        let f = formats(*family);
        [f.account, f.wallet, f.recipient, f.token]
            .iter()
            .any(|(ok, _)| ok(s))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const G: &str = "GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5";
    const SOL: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    const EVM: &str = "0x20c0000000000000000000000000000000000001";
    #[test]
    #[cfg_attr(
        not(all(feature = "solana", feature = "tempo")),
        ignore = "needs the Solana and Tempo rails"
    )]
    fn formats_match_their_family_only() {
        let native = allowit_stellar_wallet::asset_from_identity("XLM", None).unwrap();
        let contract = allowit_stellar_wallet::sac_address("stellar:testnet", &native).unwrap();
        #[allow(non_snake_case)]
        let C = contract.as_str();
        check_account(Family::Stellar, G).unwrap();
        check_wallet(Family::Stellar, C).unwrap();
        assert_eq!(check_wallet(Family::Stellar, G).unwrap_err().code, 2);
        check_recipient(Family::Stellar, C).unwrap();
        assert!(check_account(Family::Stellar, C).is_err());
        assert!(check_account(Family::Stellar, &G.replace('A', "B")).is_err());
        check_account(Family::Solana, SOL).unwrap();
        assert!(check_account(Family::Solana, G).is_err());
        check_account(Family::Tempo, EVM).unwrap();
        assert!(check_account(Family::Tempo, "0x123").is_err());
        assert_eq!(infer_family(G), Some(Family::Stellar));
        assert_eq!(infer_family(C), Some(Family::Stellar));
        assert_eq!(infer_family(SOL), Some(Family::Solana));
        assert_eq!(infer_family(EVM), Some(Family::Tempo));
        assert_eq!(infer_family("nope"), None);
        assert_eq!(check_wallet(Family::Tempo, G).unwrap_err().code, 2);
    }
    #[test]
    fn tempo_wallets_compare_without_case() {
        let upper = "0xABABABABABABABABABABABABABABABABABABABAB";
        assert!(same_wallet(Family::Tempo, upper, &upper.to_lowercase()));
        assert!(!same_wallet(Family::Tempo, upper, EVM));
        assert!(!same_wallet(Family::Solana, SOL, &SOL.to_lowercase()));
        assert!(same_wallet(Family::Stellar, G, G));
    }
}
