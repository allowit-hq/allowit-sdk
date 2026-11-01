//! Stellar address and asset formats: G… accounts, C… contracts and
//! Stellar Asset Contracts in place of a token code.
use super::super::Formats;

fn account(s: &str) -> bool {
    s.starts_with('G') && allowit_stellar_wallet::account_key(s).is_ok()
}
fn contract(s: &str) -> bool {
    s.starts_with('C') && allowit_stellar_wallet::address(s).is_ok()
}
fn account_or_contract(s: &str) -> bool {
    account(s) || contract(s)
}
/// Stellar Asset Contracts use the classic seven decimals.
fn asset_contract(s: &str) -> Option<u8> {
    contract(s).then_some(7)
}

pub(crate) const FORMATS: Formats = Formats {
    account: (account, "a G… strkey"),
    wallet: (contract, "a C… contract strkey"),
    recipient: (account_or_contract, "a G… or C… strkey"),
    token: (account_or_contract, "a G… issuer or C… contract"),
    asset_contract,
    // Classic Stellar assets have seven decimals.
    issuer_decimals: Some(7),
};
