//! Shared address and amount input. The selected native adapter supplies format checks.
pub mod address;
pub mod asset;
use crate::routing::Family;
pub(crate) use allowit_sdk::vendor::input::{Check, Formats};
pub(crate) fn formats(family: Family) -> crate::error::Result<&'static Formats> {
    crate::vendors::formats(family)
}
