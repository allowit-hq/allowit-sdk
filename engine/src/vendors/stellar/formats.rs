//! Stellar address and asset formats: G… accounts, C… contracts and
//! Stellar Asset Contracts in place of a token code.
use crate::input::Formats;

fn account(s: &str) -> bool {
    s.starts_with('G') && allowit_stellar_wallet::account_key(s).is_ok()
}
fn contract(s: &str) -> bool {
    s.starts_with('C') && allowit_stellar_wallet::address(s).is_ok()
}
fn account_or_contract(s: &str) -> bool {
    account(s) || contract(s)
}
fn asset_code(code: &str) -> bool {
    !code.is_empty() && code.len() <= 12 && code.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Stellar Asset Contracts use the classic seven decimals.
fn asset_contract(s: &str) -> Option<u8> {
    contract(s).then_some(7)
}

pub(crate) const FORMATS: Formats = Formats {
    account: (account, "a G… strkey"),
    wallet: (account_or_contract, "a G… account or C… contract strkey"),
    recipient: (account_or_contract, "a G… or C… strkey"),
    token: (account_or_contract, "a G… issuer or C… contract"),
    same_wallet: |a, b| a == b,
    asset_code,
    asset_contract,
    // Classic Stellar assets have seven decimals.
    issuer_decimals: Some(7),
};

#[cfg(test)]
mod tests {
    use super::asset_code;

    #[test]
    fn classic_asset_codes_obey_native_limits() {
        for valid in ["A", "USDC", "a1", "ABCDEFGHIJKL"] {
            assert!(asset_code(valid), "{valid}");
        }
        for invalid in ["", "ABCDEFGHIJKLM", "USD-", "US D", "ÉUR", "USD\n"] {
            assert!(!asset_code(invalid), "{invalid:?}");
        }
    }
}
