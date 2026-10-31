//! Rail identity. Every rail has one canonical CAIP-2 identifier; friendly
//! aliases are accepted as input only. Explicit selection never changes
//! process environment or inferred authority.
use crate::error::{Error, Result};
use std::cell::RefCell;
thread_local! { static SELECTED: RefCell<Option<String>> = const { RefCell::new(None) }; }

/// The chain family of a rail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Stellar,
    Solana,
    Tempo,
}
impl Family {
    pub fn name(self) -> &'static str {
        match self {
            Family::Stellar => "stellar",
            Family::Solana => "solana",
            Family::Tempo => "tempo",
        }
    }
}

/// One admitted rail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RailInfo {
    /// Canonical CAIP-2 identifier. Output always uses it.
    pub id: &'static str,
    /// Friendly input aliases. The first one is the display name.
    pub aliases: &'static [&'static str],
    pub family: Family,
    pub testnet: bool,
}
impl RailInfo {
    /// The friendly name, for example `solana:devnet`.
    pub fn name(&self) -> &'static str {
        self.aliases.first().copied().unwrap_or(self.id)
    }
}

/// Every admitted rail, in display order.
pub const RAILS: &[RailInfo] = &[
    RailInfo {
        id: "stellar:testnet",
        aliases: &["stellar:testnet"],
        family: Family::Stellar,
        testnet: true,
    },
    RailInfo {
        id: "stellar:pubnet",
        aliases: &["stellar:mainnet", "stellar:pubnet"],
        family: Family::Stellar,
        testnet: false,
    },
    RailInfo {
        id: "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
        aliases: &["solana:devnet"],
        family: Family::Solana,
        testnet: true,
    },
    RailInfo {
        id: "solana:4uhcVJyU9pJkvQyS88uRDiswHXSCkY3z",
        aliases: &["solana:testnet"],
        family: Family::Solana,
        testnet: true,
    },
    RailInfo {
        id: "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
        aliases: &["solana:mainnet"],
        family: Family::Solana,
        testnet: false,
    },
    RailInfo {
        id: "eip155:42431",
        aliases: &["tempo:testnet"],
        family: Family::Tempo,
        testnet: true,
    },
    RailInfo {
        id: "eip155:4217",
        aliases: &["tempo:mainnet"],
        family: Family::Tempo,
        testnet: false,
    },
];

/// Looks up a rail by canonical identifier or alias.
pub fn info(network: &str) -> Result<&'static RailInfo> {
    RAILS
        .iter()
        .find(|r| r.id == network || r.aliases.contains(&network))
        .ok_or_else(|| {
            Error::usage(format!(
                "unknown rail {}; run allowit rail ls",
                crate::error::quoted(network)
            ))
        })
}

/// Returns the canonical CAIP-2 identifier for a rail identifier or alias.
pub fn normalize(network: &str) -> Result<&'static str> {
    info(network).map(|r| r.id)
}

/// The chain family of a rail identifier or alias.
pub fn family(network: &str) -> Result<Family> {
    info(network).map(|r| r.family)
}

/// The friendly name that the rail crates use in their own files, for example
/// `stellar:mainnet` for `stellar:pubnet`.
pub fn legacy_name(network: &str) -> Result<&'static str> {
    info(network).map(RailInfo::name)
}

/// The canonical identifier of the rail selected for this command.
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
        with_network(Some("eip155:42431".into()), || {
            assert_eq!(selected().as_deref(), Some("eip155:42431"));
            Ok(())
        })
        .unwrap();
        assert!(selected().is_none());
    }
    #[test]
    fn aliases_normalize_to_canonical_caip2() {
        for (input, canonical) in [
            ("solana:devnet", "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1"),
            (
                "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
                "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
            ),
            ("solana:testnet", "solana:4uhcVJyU9pJkvQyS88uRDiswHXSCkY3z"),
            ("solana:mainnet", "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"),
            ("tempo:testnet", "eip155:42431"),
            ("eip155:42431", "eip155:42431"),
            ("tempo:mainnet", "eip155:4217"),
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
    #[test]
    fn legacy_names_serve_rail_crates() {
        assert_eq!(legacy_name("stellar:pubnet").unwrap(), "stellar:mainnet");
        assert_eq!(legacy_name("eip155:42431").unwrap(), "tempo:testnet");
        assert_eq!(family("eip155:4217").unwrap(), Family::Tempo);
    }
}
