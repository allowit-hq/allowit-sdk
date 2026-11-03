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

/// One token a rail knows by code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub code: &'static str,
    pub decimals: u8,
    pub native: bool,
    /// The issuer, mint or token address on this rail, when it has one.
    pub issuer: Option<&'static str>,
}

/// The asset an amount names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asset {
    /// No asset named: a plain INTEGER amount of the wallet's installed asset,
    /// or of vault shares for [`Use::Shares`].
    Installed,
    Native {
        code: String,
    },
    Token {
        code: String,
        issuer: Option<String>,
    },
    /// An asset contract accepted by the selected rail.
    Contract(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Amount {
    pub asset: Asset,
    pub quantity: u128,
    /// Decimals of the asset; `None` for base units of the installed asset.
    pub decimals: Option<u8>,
    /// The original input, for the request journal.
    pub text: String,
}

/// Where an amount appears, which decides the forms it may take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    /// `--amount`: a positive amount.
    Spend,
    /// `--token-out ASSET:MIN`: an asset is required and MIN may be zero.
    Minimum,
    /// Vault shares: a positive INTEGER of base units.
    Shares,
}

/// The uses of an operation's `--amount` and `--token-out`: a redeem spends
/// vault shares and a deposit receives them.
pub fn exec_uses(op: &str) -> (Use, Use) {
    match op {
        "redeem" => (Use::Shares, Use::Minimum),
        "deposit" => (Use::Spend, Use::Shares),
        _ => (Use::Spend, Use::Minimum),
    }
}

impl Amount {
    pub fn units(&self) -> u128 {
        self.quantity
    }
}

const FORMS: &str = "use CODE:DECIMAL, CODE:ISSUER:DECIMAL, CONTRACT:DECIMAL or INTEGER base units";

/// Converts a display decimal to base units, refusing extra precision.
pub fn decimal_units(text: &str, decimals: u8) -> Result<u128> {
    let invalid = || {
        Error::usage(format!(
            "{} is not a decimal amount such as 5 or 0.25",
            quoted(text)
        ))
    };
    if text.is_empty() || text.len() > 40 {
        return Err(invalid());
    }
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || (text.contains('.') && fraction.is_empty())
        || (whole.len() > 1 && whole.starts_with('0'))
    {
        return Err(invalid());
    }
    if fraction.len() > usize::from(decimals) {
        return Err(Error::usage(format!(
            "{} has more than {decimals} decimal places",
            quoted(text)
        )));
    }
    let scale = 10u128.pow(u32::from(decimals));
    let whole: u128 = whole.parse().map_err(|_| invalid())?;
    let fraction: u128 = if fraction.is_empty() {
        0
    } else {
        format!("{fraction:0<width$}", width = usize::from(decimals))
            .parse()
            .map_err(|_| invalid())?
    };
    whole
        .checked_mul(scale)
        .and_then(|w| w.checked_add(fraction))
        .ok_or_else(|| Error::usage(format!("{} is too large", quoted(text))))
}

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
