//! Wallet kinds and asset units for hosts. Core describes the four kinds; each registered rail
//! advertises the kinds it supports and supplies the unit of every amount. No function here
//! names a vendor.
use crate::{error::Result, plugins::for_network, routing::normalize};
pub use allowit_core::vendor::input::asset::{AssetDisplay, units_decimal};
pub use allowit_core::vendor::kinds::{
    Enforcement, STANDARD_PARAMS, STANDARD_PARAMS_URL, WALLET_KINDS, WalletCustody, WalletKind,
    WalletKindInfo,
};

/// The kinds that the rail of `network` supports, in selection order, with their descriptions.
pub fn wallet_kinds(network: &str) -> Result<Vec<&'static WalletKindInfo>> {
    let network = normalize(network)?;
    Ok(for_network(network)?
        .wallet_kinds(network)
        .into_iter()
        .map(WalletKind::info)
        .collect())
}

/// The kind that a public wallet profile document binds.
pub fn wallet_kind(network: &str, document: &[u8]) -> Result<WalletKind> {
    let network = normalize(network)?;
    for_network(network)?.wallet_kind(network, document)
}

/// Display metadata of the network's native asset.
pub fn native_asset(network: &str) -> Result<AssetDisplay> {
    let network = normalize(network)?;
    for_network(network)?.native_asset(network)
}

/// Display metadata of an asset reference: a code or a native asset identity of the rail.
pub fn asset_display(network: &str, asset: &str) -> Result<AssetDisplay> {
    let network = normalize(network)?;
    for_network(network)?.asset_display(network, asset)
}

/// Base units of `asset` as an amount with the rail's unit, such as `5 XYZ`. Hosts print every
/// amount through this function or [`AssetDisplay::format`]; an unknown asset is an error, never
/// a placeholder unit.
pub fn format_amount(network: &str, asset: &str, units: i128) -> Result<String> {
    Ok(asset_display(network, asset)?.format(units))
}
