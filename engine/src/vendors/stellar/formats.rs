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

fn address_candidate(token: &str) -> bool {
    token.len() == 56
        && matches!(token.as_bytes()[0], b'G' | b'C' | b'M')
        && token
            .bytes()
            .all(|b| b.is_ascii_uppercase() || (b'2'..=b'7').contains(&b))
}
fn resolve_asset(
    network: &str,
    symbol: &str,
    issuer: Option<&str>,
    known_default: bool,
) -> crate::error::Result<crate::input::asset::ResolvedAsset> {
    let issuer = issuer.or_else(|| {
        known_default
            .then(|| {
                super::tokens_for(network)
                    .iter()
                    .find(|t| t.code == symbol)
                    .and_then(|t| t.issuer)
            })
            .flatten()
    });
    let asset = allowit_stellar_wallet::asset_from_identity(symbol, issuer)
        .map_err(crate::error::Error::usage)?;
    let address =
        allowit_stellar_wallet::sac_address(network, &asset).map_err(crate::error::Error::usage)?;
    Ok(crate::input::asset::ResolvedAsset {
        symbol: symbol.into(),
        address,
        decimals: 7,
    })
}

pub(crate) const FORMATS: Formats = Formats {
    account: (account, "a G… strkey"),
    wallet: (account_or_contract, "a G… account or C… contract strkey"),
    recipient: (account_or_contract, "a G… or C… strkey"),
    token: (account_or_contract, "a G… issuer or C… contract"),
    account_key: |address| allowit_stellar_wallet::account_key(address).ok(),
    address_candidate,
    resolve_asset,
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
