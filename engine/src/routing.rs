//! Rail identity. Every rail has one canonical CAIP-2 identifier; friendly
//! aliases are accepted as input only. Explicit selection never changes
//! process environment or inferred authority.
use crate::error::{Error, Result};
use std::cell::RefCell;
thread_local! { static SELECTED: RefCell<Option<String>> = const { RefCell::new(None) }; }

pub use allowit_sdk::vendor::routing::{Family, RailInfo};
/// Every network advertised by the compiled vendor registrations, in build order.
pub fn all() -> &'static [RailInfo] {
    static RAILS: std::sync::OnceLock<Vec<RailInfo>> = std::sync::OnceLock::new();
    RAILS.get_or_init(crate::vendors::networks)
}
/// Looks up a rail by canonical identifier or alias.
pub fn info(network: &str) -> Result<&'static RailInfo> {
    all()
        .iter()
        .find(|r| r.id == network || r.aliases.contains(&network))
        .ok_or_else(|| Error::usage(format!("unknown rail {}", crate::error::quoted(network))))
}

/// Returns the canonical CAIP-2 identifier for a rail identifier or alias.
pub fn normalize(network: &str) -> Result<&'static str> {
    info(network).map(|r| r.id)
}

/// The chain family of a rail identifier or alias.
pub fn family(network: &str) -> Result<Family> {
    info(network).map(|r| r.family)
}

/// The canonical identifier of the rail selected for this caller.
pub fn selected() -> Option<String> {
    SELECTED.with(|n| n.borrow().clone())
}
pub fn with_network<T>(network: Option<String>, f: impl FnOnce() -> Result<T>) -> Result<T> {
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SELECTED.with(|n| *n.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(SELECTED.with(|n| n.replace(network)));
    f()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_is_scoped() {
        with_network(Some("stellar:testnet".into()), || {
            assert_eq!(selected().as_deref(), Some("stellar:testnet"));
            Ok(())
        })
        .unwrap();
        assert!(selected().is_none());
    }
    #[cfg(feature = "stellar")]
    #[test]
    fn aliases_normalize_to_canonical_caip2() {
        for (input, canonical) in [
            ("stellar:mainnet", "stellar:pubnet"),
            ("stellar:pubnet", "stellar:pubnet"),
            ("stellar:testnet", "stellar:testnet"),
        ] {
            assert_eq!(normalize(input).unwrap(), canonical, "{input}");
            // Normalization is idempotent: the output is valid input.
            assert_eq!(normalize(canonical).unwrap(), canonical);
        }
        for bad in ["eip155:1", "solana", "", "stellar:futurenet"] {
            assert_eq!(normalize(bad).unwrap_err().code, 2, "{bad}");
        }
    }
}

#[cfg(all(test, not(feature = "stellar")))]
mod empty_build {
    #[test]
    fn no_compiled_vendor_exposes_no_network() {
        assert!(super::all().is_empty());
        assert!(super::info("stellar:testnet").is_err());
        assert!(super::family("stellar:testnet").is_err());
    }
}
