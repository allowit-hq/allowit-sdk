//! Amount and asset input, the same on every rail. Each rail supplies its
//! token table and decimals.
//!
//! | Form | Meaning |
//! |---|---|
//! | `CODE:DECIMAL` | display units of a token from the rail's table; `native` names the native asset |
//! | `CODE:ISSUER:DECIMAL` | ISSUER is a Stellar G issuer, a Solana mint or a Tempo 0x token |
//! | `C…:DECIMAL` | a Stellar asset contract (Stellar only) |
//! | `INTEGER` | base units of the wallet's installed asset |
//! | `CODE:max` | the whole balance; `wallet withdraw` only |
//!
//! `--token-out` takes `ASSET:MIN` in the same forms; MIN may be zero.
use crate::{
    error::{Error, Result, quoted},
    routing::Family,
};

/// One token a rail knows by code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    pub code: &'static str,
    pub decimals: u8,
    pub native: bool,
    /// The issuer, mint or token address on this rail, when it has one.
    pub issuer: Option<&'static str>,
}

/// The asset an amount names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Asset {
    /// The wallet's installed asset (a plain INTEGER amount).
    Installed,
    Native {
        code: String,
    },
    Token {
        code: String,
        issuer: Option<String>,
    },
    /// A Stellar asset contract.
    Contract(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Quantity {
    Units(u128),
    Max,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Amount {
    pub asset: Asset,
    pub quantity: Quantity,
    /// Decimals of the asset; `None` for base units of the installed asset.
    pub decimals: Option<u8>,
    /// The original input, for the request journal.
    pub text: String,
}

/// Where an amount appears, which decides the forms it may take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Use {
    /// `--amount`, `wallet fund`: a positive amount.
    Spend,
    /// `wallet withdraw`: a positive amount or `CODE:max`.
    Withdraw,
    /// `--token-out ASSET:MIN`: an asset is required and MIN may be zero.
    Minimum,
    /// A policy parameter value: a nonnegative amount.
    Limit,
}

impl Amount {
    pub(crate) fn units(&self) -> Option<u128> {
        match self.quantity {
            Quantity::Units(u) => Some(u),
            Quantity::Max => None,
        }
    }
}

const FORMS: &str = "use CODE:DECIMAL, CODE:ISSUER:DECIMAL, C…:DECIMAL or INTEGER base units";

/// Formats base units as a decimal without trailing zeros.
pub(crate) fn format_units(units: u128, decimals: u8) -> String {
    let scale = 10u128.pow(u32::from(decimals));
    let whole = units / scale;
    let fraction = units % scale;
    if fraction == 0 {
        return whole.to_string();
    }
    let digits = format!("{fraction:0width$}", width = usize::from(decimals));
    format!("{whole}.{}", digits.trim_end_matches('0'))
}

/// Converts a display decimal to base units, refusing extra precision.
pub(crate) fn decimal_units(text: &str, decimals: u8) -> Result<u128> {
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

fn code_valid(code: &str) -> bool {
    !code.is_empty() && code.len() <= 12 && code.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Parses an amount for a rail with the given family and token table.
pub(crate) fn parse(family: Family, tokens: &[Token], text: &str, usage: Use) -> Result<Amount> {
    if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
        if usage == Use::Minimum {
            return Err(Error::usage(
                "--token-out needs ASSET:MIN, for example USDC:9.5",
            ));
        }
        let units: u128 = text
            .parse()
            .map_err(|_| Error::usage(format!("{} is too large", quoted(text))))?;
        if units == 0 && usage != Use::Limit {
            return Err(Error::usage("the amount must be positive"));
        }
        return Ok(Amount {
            asset: Asset::Installed,
            quantity: Quantity::Units(units),
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
    let quantity = if quantity == "max" {
        if usage != Use::Withdraw {
            return Err(Error::usage("CODE:max is accepted only by wallet withdraw"));
        }
        Quantity::Max
    } else {
        let units = decimal_units(quantity, decimals)?;
        if units == 0 && !matches!(usage, Use::Minimum | Use::Limit) {
            return Err(Error::usage("the amount must be positive"));
        }
        Quantity::Units(units)
    };
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
    let formats = super::formats(family);
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
    if !code_valid(code) {
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
            super::address::check_token(family, issuer)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    const STELLAR: &[Token] = &[
        Token {
            code: "XLM",
            decimals: 7,
            native: true,
            issuer: None,
        },
        Token {
            code: "USDC",
            decimals: 7,
            native: false,
            issuer: Some("GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5"),
        },
    ];
    const SOLANA: &[Token] = &[
        Token {
            code: "SOL",
            decimals: 9,
            native: true,
            issuer: None,
        },
        Token {
            code: "USDC",
            decimals: 6,
            native: false,
            issuer: Some("4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU"),
        },
    ];
    fn units(family: Family, tokens: &[Token], s: &str) -> u128 {
        parse(family, tokens, s, Use::Spend)
            .unwrap()
            .units()
            .unwrap()
    }
    #[test]
    fn units_parse_and_format_with_the_rail_decimals() {
        assert_eq!(units(Family::Stellar, STELLAR, "XLM:5"), 50_000_000);
        assert_eq!(units(Family::Stellar, STELLAR, "USDC:0.25"), 2_500_000);
        assert_eq!(units(Family::Solana, SOLANA, "USDC:0.25"), 250_000);
        assert_eq!(units(Family::Solana, SOLANA, "SOL:1"), 1_000_000_000);
        assert_eq!(units(Family::Solana, SOLANA, "1500"), 1500);
        assert_eq!(format_units(2_500_000, 7), "0.25");
        assert_eq!(format_units(50_000_000, 7), "5");
        assert_eq!(format_units(1, 6), "0.000001");
        assert_eq!(decimal_units("0.000001", 6).unwrap(), 1);
    }
    #[test]
    fn native_and_xlm_name_the_same_asset() {
        let a = parse(Family::Stellar, STELLAR, "native:1", Use::Spend).unwrap();
        let b = parse(Family::Stellar, STELLAR, "XLM:1", Use::Spend).unwrap();
        assert_eq!(a.asset, b.asset);
        assert_eq!(a.units(), b.units());
        assert_eq!(a.asset, Asset::Native { code: "XLM".into() });
    }
    #[test]
    fn issuer_contract_and_installed_forms() {
        let issued = parse(
            Family::Stellar,
            STELLAR,
            "EURC:GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5:1.5",
            Use::Spend,
        )
        .unwrap();
        assert_eq!(issued.units(), Some(15_000_000));
        assert!(matches!(issued.asset, Asset::Token { ref issuer, .. } if issuer.is_some()));
        let native = allowit_stellar_wallet::asset_from_identity("XLM", None).unwrap();
        let sac = allowit_stellar_wallet::sac_address("stellar:testnet", &native).unwrap();
        let contract = parse(Family::Stellar, STELLAR, &format!("{sac}:2"), Use::Spend).unwrap();
        assert_eq!(contract.asset, Asset::Contract(sac.clone()));
        assert_eq!(contract.units(), Some(20_000_000));
        assert!(parse(Family::Solana, SOLANA, &format!("{sac}:2"), Use::Spend).is_err());
        let installed = parse(Family::Stellar, STELLAR, "100", Use::Spend).unwrap();
        assert_eq!(installed.asset, Asset::Installed);
        assert_eq!(installed.decimals, None);
        assert!(
            parse(
                Family::Solana,
                SOLANA,
                "BONK:DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263:1",
                Use::Spend
            )
            .is_err()
        );
    }
    #[test]
    fn max_minimum_and_bad_input() {
        let huge = decimal_units(&u128::MAX.to_string(), 7).unwrap_err();
        assert_eq!(huge.code, 2);
        assert!(huge.message.contains("is too large"), "{}", huge.message);
        let max = parse(Family::Stellar, STELLAR, "XLM:max", Use::Withdraw).unwrap();
        assert_eq!(max.quantity, Quantity::Max);
        assert!(parse(Family::Stellar, STELLAR, "XLM:max", Use::Spend).is_err());
        assert_eq!(
            parse(Family::Stellar, STELLAR, "XLM:0", Use::Limit)
                .unwrap()
                .units(),
            Some(0)
        );
        assert_eq!(
            parse(Family::Stellar, STELLAR, "0", Use::Limit)
                .unwrap()
                .units(),
            Some(0)
        );
        assert!(parse(Family::Stellar, STELLAR, "0", Use::Spend).is_err());
        let min = parse(Family::Stellar, STELLAR, "USDC:0", Use::Minimum).unwrap();
        assert_eq!(min.units(), Some(0));
        assert!(parse(Family::Stellar, STELLAR, "10", Use::Minimum).is_err());
        for bad in [
            "",
            "0",
            "XLM:0",
            "XLM:-1",
            "XLM:1e3",
            "XLM:01",
            "XLM:1.",
            "XLM:.5",
            "XLM:0.00000001",
            "DOGE:1",
            "a:b:c:d",
            "XLM:G:1",
            "USDC:1,5",
        ] {
            assert_eq!(
                parse(Family::Stellar, STELLAR, bad, Use::Spend)
                    .unwrap_err()
                    .code,
                2,
                "{bad}"
            );
        }
    }
}
