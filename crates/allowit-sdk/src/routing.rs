//! Rail identity. Every rail has one canonical CAIP-2 identifier; friendly
//! aliases are accepted as input only. Explicit selection never changes
//! process environment or inferred authority.
use crate::error::{Error, Result};
use std::cell::RefCell;
thread_local! { static SELECTED: RefCell<Option<String>> = const { RefCell::new(None) }; }

pub use allowit_core::vendor::routing::{Family, RailInfo};
/// Every network advertised by the compiled vendor registrations, in build order.
pub fn all() -> &'static [RailInfo] {
    static RAILS: std::sync::OnceLock<Vec<RailInfo>> = std::sync::OnceLock::new();
    RAILS.get_or_init(|| crate::plugins::networks().copied().collect())
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
        let network = all()[0].id;
        with_network(Some(network.into()), || {
            assert_eq!(selected().as_deref(), Some(network));
            Ok(())
        })
        .unwrap();
        assert!(selected().is_none());
    }
    #[test]
    fn every_alias_normalizes_to_its_canonical_caip2() {
        assert!(!all().is_empty(), "no registered rail");
        for rail in all() {
            assert_eq!(normalize(rail.id).unwrap(), rail.id);
            assert_eq!(family(rail.id).unwrap(), rail.family);
            for alias in rail.aliases {
                assert_eq!(normalize(alias).unwrap(), rail.id, "{alias}");
            }
        }
        for bad in ["", "unknown:none", "eip155"] {
            assert_eq!(normalize(bad).unwrap_err().code, 2, "{bad}");
        }
    }
}
