//! Address format checks per chain family. A wallet is identified only by its
//! address, so every command checks that an address matches the rail.
use crate::{
    error::{Error, Result, quoted},
    routing::Family,
};

fn stellar_account(s: &str) -> bool {
    s.starts_with('G') && allowit_stellar_wallet::account_key(s).is_ok()
}
fn stellar_contract(s: &str) -> bool {
    s.starts_with('C') && allowit_stellar_wallet::address(s).is_ok()
}
fn solana(s: &str) -> bool {
    crate::request::solana_address(s)
}
fn evm(s: &str) -> bool {
    s.len() == 42 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_hexdigit())
}

fn bad(what: &str, family: Family, s: &str, expected: &str) -> Error {
    Error::usage(format!(
        "{} is not a {} {what} ({expected})",
        quoted(s),
        family.name()
    ))
}

/// An account that holds a key: a Stellar G account, a Solana base58 key or
/// a Tempo 0x account.
pub(crate) fn check_account(family: Family, s: &str) -> Result<()> {
    let (ok, expected) = match family {
        Family::Stellar => (stellar_account(s), "a G… strkey"),
        Family::Solana => (solana(s), "a base58 public key"),
        Family::Tempo => (evm(s), "a 0x address"),
    };
    if ok {
        Ok(())
    } else {
        Err(bad("account", family, s, expected))
    }
}

/// A policy wallet: a Stellar C contract, a Solana vault address or a Tempo
/// 0x vault.
pub(crate) fn check_wallet(family: Family, s: &str) -> Result<()> {
    let (ok, expected) = match family {
        Family::Stellar => (stellar_contract(s), "a C… contract strkey"),
        Family::Solana => (solana(s), "a base58 vault address"),
        Family::Tempo => (evm(s), "a 0x vault address"),
    };
    if ok {
        Ok(())
    } else {
        Err(bad("wallet address", family, s, expected))
    }
}

/// A transfer recipient: a Stellar G or C address, a Solana base58 address or
/// a Tempo 0x address.
pub(crate) fn check_recipient(family: Family, s: &str) -> Result<()> {
    let (ok, expected) = match family {
        Family::Stellar => (
            stellar_account(s) || stellar_contract(s),
            "a G… or C… strkey",
        ),
        Family::Solana => (solana(s), "a base58 address"),
        Family::Tempo => (evm(s), "a 0x address"),
    };
    if ok {
        Ok(())
    } else {
        Err(bad("recipient", family, s, expected))
    }
}

/// A token identity: a Stellar G issuer or C contract, a Solana mint or a
/// Tempo 0x token.
pub(crate) fn check_token(family: Family, s: &str) -> Result<()> {
    let (ok, expected) = match family {
        Family::Stellar => (
            stellar_account(s) || stellar_contract(s),
            "a G… issuer or C… contract",
        ),
        Family::Solana => (solana(s), "a base58 mint"),
        Family::Tempo => (evm(s), "a 0x token"),
    };
    if ok {
        Ok(())
    } else {
        Err(bad("token", family, s, expected))
    }
}

/// True for a Stellar C contract strkey.
pub(crate) fn is_stellar_contract(s: &str) -> bool {
    stellar_contract(s)
}

/// The family that an address format names, when it names exactly one.
pub(crate) fn infer_family(s: &str) -> Option<Family> {
    if stellar_account(s) || stellar_contract(s) {
        Some(Family::Stellar)
    } else if evm(s) {
        Some(Family::Tempo)
    } else if solana(s) {
        Some(Family::Solana)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const G: &str = "GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5";
    const SOL: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    const EVM: &str = "0x20c0000000000000000000000000000000000001";
    #[test]
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
}
