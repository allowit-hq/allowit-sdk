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

/// One param of a policy for an owner to read: its key, its label and its value in human units.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ParamLine {
    pub key: String,
    pub label: String,
    pub value: String,
}

/// Owner-readable lines for the params of a generated draft, a policy or an installation
/// document of the rail of `network`. The rail reads its own document; this function prints
/// each param: an amount in the unit of the installed asset, an expiry as a UTC time, an asset
/// by its symbol and a canonical key by its Core label.
pub fn param_lines(network: &str, document: &serde_json::Value) -> Result<Vec<ParamLine>> {
    let network = normalize(network)?;
    let read = for_network(network)?.draft_params(network, document)?;
    lines(
        network,
        read.params.iter().map(|(k, v)| (k.as_str(), v)),
        &read.asset,
    )
}

/// Owner-readable lines for the params of an imported wallet profile.
pub fn profile_param_lines(description: &crate::profiles::Description) -> Result<Vec<ParamLine>> {
    let network = normalize(&description.network)?;
    let asset = asset_display(network, &description.asset)
        .or_else(|_| asset_display(network, &description.asset_symbol))?;
    let params = description
        .params
        .iter()
        .map(|(key, value)| Ok((key.as_str(), serde_json::to_value(value)?)))
        .collect::<std::result::Result<Vec<_>, serde_json::Error>>()
        .map_err(|_| crate::error::Error::config("Cannot encode policy parameters"))?;
    lines(network, params.iter().map(|(k, v)| (*k, v)), &asset)
}

fn lines<'a>(
    network: &str,
    params: impl Iterator<Item = (&'a str, &'a serde_json::Value)>,
    asset: &AssetDisplay,
) -> Result<Vec<ParamLine>> {
    let registry = allowit_core::policy::canonical_function_registry()
        .map_err(|_| crate::error::Error::config("Cannot read the Core function registry"))?;
    Ok(params
        .map(|(key, value)| {
            let canonical = registry.functions.iter().find(|f| f.name == key);
            let semantic = canonical.map_or("", |f| f.parameter.semantic_type.as_str());
            ParamLine {
                key: key.into(),
                label: canonical.map_or(key, |f| f.label.as_str()).into(),
                value: text(network, value, semantic, asset),
            }
        })
        .collect())
}

/// One `PolicyValue` JSON as text. A list joins its items; a flag reads Yes or No.
fn text(network: &str, value: &serde_json::Value, semantic: &str, asset: &AssetDisplay) -> String {
    let Some((tag, item)) = value.as_object().and_then(|v| v.iter().next()) else {
        return value.to_string();
    };
    match (tag.as_str(), item) {
        ("List", serde_json::Value::Array(items)) => items
            .iter()
            .map(|item| text(network, item, semantic, asset))
            .collect::<Vec<_>>()
            .join(", "),
        ("Bool", serde_json::Value::Bool(flag)) => if *flag { "Yes" } else { "No" }.into(),
        (_, item) => {
            let raw = item.as_str().map_or_else(|| item.to_string(), String::from);
            leaf(network, &raw, semantic, asset)
        }
    }
}

fn leaf(network: &str, raw: &str, semantic: &str, asset: &AssetDisplay) -> String {
    match semantic {
        "amount" => raw
            .parse::<i128>()
            .map_or_else(|_| raw.into(), |units| asset.format(units)),
        "timestamp" => raw
            .parse::<i64>()
            .ok()
            .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
            .map_or_else(
                || raw.into(),
                |time| time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            ),
        "assets" => asset_display(network, raw).map_or_else(|_| raw.into(), |known| known.symbol),
        "operations" if raw == "*" => "Any".into(),
        _ => raw.into(),
    }
}
