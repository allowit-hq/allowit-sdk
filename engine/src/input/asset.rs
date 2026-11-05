//! Amount and asset input, the same on every rail. Each rail supplies its
//! token table and decimals.
//!
//! | Form | Meaning |
//! |---|---|
//! | `CODE:DECIMAL` | display units of a token from the rail's table; `native` names the native asset |
//! | `CODE:ISSUER:DECIMAL` | ISSUER is validated by the selected rail |
//! | `CONTRACT:DECIMAL` | an asset contract supported by the selected rail |
//! | `INTEGER` | base units of the wallet's installed asset, or vault shares |
//!
//! `--token-out` takes `ASSET:MIN` in the same forms; MIN may be zero. Vault
//! shares, the `--amount` of a redeem and the `--token-out` of a deposit, are
//! INTEGER base units only.
use crate::{
    error::{Error, Result, quoted},
    routing::Family,
};

pub use allowit_sdk::vendor::input::asset::{Amount, Asset, Token, Use, exec_uses};

const FORMS: &str = "use CODE:DECIMAL, CODE:ISSUER:DECIMAL, CONTRACT:DECIMAL or INTEGER base units";

pub use allowit_sdk::vendor::input::asset::decimal_units;

/// Parses an amount for a rail with the given family and token table.
pub fn parse(family: Family, tokens: &[Token], text: &str, usage: Use) -> Result<Amount> {
    let integer = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    if usage == Use::Shares && !integer {
        return Err(Error::usage(format!(
            "{} is not a share amount; vault shares are INTEGER base units",
            quoted(text)
        )));
    }
    if integer {
        if usage == Use::Minimum {
            return Err(Error::usage(
                "--token-out needs ASSET:MIN, for example USDC:9.5",
            ));
        }
        let units: u128 = text
            .parse()
            .map_err(|_| Error::usage(format!("{} is too large", quoted(text))))?;
        if units == 0 {
            return Err(Error::usage("the amount must be positive"));
        }
        return Ok(Amount {
            asset: Asset::Installed,
            quantity: units,
            decimals: None,
            text: text.into(),
        });
    }
    let parts: Vec<&str> = text.split(':').collect();
    let (asset, decimals, quantity) = match parts.as_slice() {
        [asset, quantity] => {
            let (asset, decimals) = named_asset(family, tokens, asset, None)?;
            (asset, decimals, *quantity)
        }
        [code, issuer, quantity] => {
            let (asset, decimals) = named_asset(family, tokens, code, Some(issuer))?;
            (asset, decimals, *quantity)
        }
        _ => {
            return Err(Error::usage(format!(
                "{} is not an amount; {FORMS}",
                quoted(text)
            )));
        }
    };
    let quantity = decimal_units(quantity, decimals)?;
    if quantity == 0 && usage != Use::Minimum {
        return Err(Error::usage("the amount must be positive"));
    }
    Ok(Amount {
        asset,
        quantity,
        decimals: Some(decimals),
        text: text.into(),
    })
}

fn named_asset(
    family: Family,
    tokens: &[Token],
    code: &str,
    issuer: Option<&str>,
) -> Result<(Asset, u8)> {
    let formats = crate::input::formats(family)?;
    if issuer.is_none()
        && let Some(decimals) = (formats.asset_contract)(code)
    {
        return Ok((Asset::Contract(code.into()), decimals));
    }
    let native = tokens.iter().find(|t| t.native);
    if code.eq_ignore_ascii_case("native") || native.is_some_and(|t| t.code == code) {
        let token = native.ok_or_else(|| Error::usage("this rail has no native asset"))?;
        if issuer.is_some() {
            return Err(Error::usage(format!("{} takes no issuer", token.code)));
        }
        return Ok((
            Asset::Native {
                code: token.code.into(),
            },
            token.decimals,
        ));
    }
    if !(formats.asset_code)(code) {
        return Err(Error::usage(format!(
            "{} is not an asset code; {FORMS}",
            quoted(code)
        )));
    }
    let known = tokens.iter().find(|t| !t.native && t.code == code);
    match issuer {
        None => {
            let token = known.ok_or_else(|| unknown_code(code, tokens))?;
            Ok((
                Asset::Token {
                    code: code.into(),
                    issuer: token.issuer.map(str::to_owned),
                },
                token.decimals,
            ))
        }
        Some(issuer) => {
            crate::input::address::check_token(family, issuer)?;
            let decimals = match known {
                Some(t) if t.issuer.is_none_or(|i| i == issuer) => t.decimals,
                _ => formats.issuer_decimals.ok_or_else(|| {
                    Error::usage(format!(
                        "the decimals of token {issuer} are unknown; give the amount in base units"
                    ))
                })?,
            };
            Ok((
                Asset::Token {
                    code: code.into(),
                    issuer: Some(issuer.into()),
                },
                decimals,
            ))
        }
    }
}

fn unknown_code(code: &str, tokens: &[Token]) -> Error {
    let known: Vec<&str> = tokens.iter().map(|t| t.code).collect();
    Error::usage(format!(
        "unknown asset code {}; known codes are {} and native, or use CODE:ISSUER:DECIMAL",
        quoted(code),
        known.join(", ")
    ))
}

pub use allowit_sdk::vendor::input::asset::ResolvedAsset;
/// Resolve an explicit asset identity. Known defaults apply only when requested.
pub fn resolve(
    network: &str,
    symbol: &str,
    issuer: Option<&str>,
    known_default: bool,
) -> Result<ResolvedAsset> {
    let network = crate::routing::normalize(network)?;
    let formats = super::formats(crate::routing::family(network)?)?;
    (formats.resolve_asset)(network, symbol, issuer, known_default)
}
