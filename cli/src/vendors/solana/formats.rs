//! Solana address and asset formats: base58 public keys. The decimals of a
//! mint outside the token table are unknown.
use super::super::Formats;

fn base58(s: &str) -> bool {
    crate::request::solana_address(s)
}

pub(crate) const FORMATS: Formats = Formats {
    account: (base58, "a base58 public key"),
    wallet: (base58, "a base58 vault address"),
    recipient: (base58, "a base58 address"),
    token: (base58, "a base58 mint"),
    asset_contract: |_| None,
    issuer_decimals: None,
};
