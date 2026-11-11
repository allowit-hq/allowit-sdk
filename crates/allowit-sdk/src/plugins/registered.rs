//! The only Rust file that names vendors: one entry line per registered rail plugin and one
//! import line per registered policy package.
use allowit_core::{policy_package::PolicyPackage, vendor::plugin::RailPlugin};

#[rustfmt::skip]
pub(super) static RAILS: &[&dyn RailPlugin] = &[
    &allowit_stellar::PLUGIN,
];

/// Each imported package, under its import alias. This build registers none.
pub(super) static PACKAGES: &[&PolicyPackage] = &[];
