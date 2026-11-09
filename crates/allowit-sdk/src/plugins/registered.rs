//! The only Rust file that names vendors: one entry line per registered rail plugin.
use allowit_core::vendor::plugin::RailPlugin;

#[rustfmt::skip]
pub(super) static RAILS: &[&dyn RailPlugin] = &[
    &allowit_stellar::PLUGIN,
];
