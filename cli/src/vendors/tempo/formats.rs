//! Tempo address and asset formats: 0x… EVM addresses and TIP-20 tokens.
use super::super::Formats;

fn evm(s: &str) -> bool {
    s.len() == 42 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_hexdigit())
}

pub(crate) const FORMATS: Formats = Formats {
    account: (evm, "a 0x address"),
    wallet: (evm, "a 0x vault address"),
    recipient: (evm, "a 0x address"),
    token: (evm, "a 0x token"),
    asset_contract: |_| None,
    // TIP-20 USD tokens have six decimals.
    issuer_decimals: Some(6),
};
