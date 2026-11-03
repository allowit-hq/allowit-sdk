//! Shared address and amount input. The selected native adapter supplies format checks.
pub mod address;
pub mod asset;
use crate::routing::Family;
pub(crate) type Check = (fn(&str) -> bool, &'static str);
pub(crate) struct Formats {
    pub account: Check,
    pub wallet: Check,
    pub recipient: Check,
    pub token: Check,
    pub same_wallet: fn(&str, &str) -> bool,
    pub asset_code: fn(&str) -> bool,
    pub asset_contract: fn(&str) -> Option<u8>,
    pub issuer_decimals: Option<u8>,
}
pub(crate) fn formats(family: Family) -> crate::error::Result<&'static Formats> {
    crate::vendors::formats(family)
}
