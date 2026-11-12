//! Address format checks. A wallet is identified only by its address, so
//! every command checks that an address matches the rail. The selected rail
//! crate supplies the formats.
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

/// An account accepted by the selected rail.
pub fn check_account(family: Family, s: &str) -> Result<()> {
    check("account", family, formats(family)?.account, s)
}

/// A wallet address accepted by the selected rail.
pub fn check_wallet(family: Family, s: &str) -> Result<()> {
    check("wallet address", family, formats(family)?.wallet, s)
}

/// A transfer recipient accepted by the selected rail.
pub fn check_recipient(family: Family, s: &str) -> Result<()> {
    check("recipient", family, formats(family)?.recipient, s)
}

/// A token identity accepted by the selected rail.
pub fn check_token(family: Family, s: &str) -> Result<()> {
    check("token", family, formats(family)?.token, s)
}

/// Whether the selected rail treats two addresses as the same wallet.
pub fn same_wallet(family: Family, a: &str, b: &str) -> bool {
    formats(family).is_ok_and(|formats| (formats.same_wallet)(a, b))
}

/// Address-shaped source tokens, including invalid values that admission must reject.
pub fn candidates(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| {
            crate::routing::all().iter().any(|rail| {
                formats(rail.family).is_ok_and(|format| (format.address_candidate)(token))
            })
        })
}
/// Whether a compiled adapter accepts this wallet identity.
pub fn valid_wallet(address: &str) -> bool {
    crate::routing::all()
        .iter()
        .any(|rail| check_wallet(rail.family, address).is_ok())
}

/// Public signing key decoded by a compiled account adapter.
pub fn account_key(address: &str) -> Option<[u8; 32]> {
    crate::routing::all().iter().find_map(|rail| {
        formats(rail.family)
            .ok()
            .and_then(|format| (format.account_key)(address))
    })
}

/// Digest for a wallet message, with the selected account adapter's signing domain.
pub fn signing_digest(identity: &str, message: &str) -> Result<[u8; 32]> {
    crate::routing::all()
        .iter()
        .find_map(|rail| {
            let formats = formats(rail.family).ok()?;
            (formats.account.0)(identity).then(|| (formats.signing_digest)(message))
        })
        .ok_or_else(|| Error::usage("No compiled account adapter accepts this signing identity"))
}

/// Native diagnostic for an identity rejected by all compiled wallet adapters.
pub fn wallet_error(address: &str) -> Option<String> {
    if valid_wallet(address) {
        return None;
    }
    crate::routing::all()
        .iter()
        .find_map(|rail| {
            formats(rail.family)
                .ok()
                .and_then(|format| (format.wallet_error)(address))
        })
        .or_else(|| Some("No compiled wallet adapter accepts this address.".into()))
}
