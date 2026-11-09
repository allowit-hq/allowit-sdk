//! The only Rust file that names vendors: one entry per registered rail plugin.
use allowit_core::vendor::plugin::RailPlugin;

pub(super) static RAILS: &[&dyn RailPlugin] = &[
    &allowit_stellar_wallet::PLUGIN,
];
