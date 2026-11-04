//! Shared address and amount input. The selected native adapter supplies format checks.
pub mod address;
pub mod asset;
use crate::routing::Family;
pub(crate) type Check = (fn(&str) -> bool, &'static str);
pub(crate) struct Formats {
    pub account: Check,
    pub wallet: Check,
    pub wallet_error: fn(&str) -> Option<String>,
    pub recipient: Check,
    pub token: Check,
    pub signing_digest: fn(&str) -> [u8; 32],
    pub account_key: fn(&str) -> Option<[u8; 32]>,
    pub address_candidate: fn(&str) -> bool,
    pub resolve_asset:
        fn(&str, &str, Option<&str>, bool) -> crate::error::Result<asset::ResolvedAsset>,
    pub same_wallet: fn(&str, &str) -> bool,
    pub asset_code: fn(&str) -> bool,
    pub asset_contract: fn(&str) -> Option<u8>,
    pub issuer_decimals: Option<u8>,
}
pub(crate) fn formats(family: Family) -> crate::error::Result<&'static Formats> {
    crate::vendors::formats(family)
}
