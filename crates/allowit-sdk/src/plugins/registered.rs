//! The only Rust file that names vendors: one entry line per registered rail plugin and one
//! import line per registered policy package.
use allowit_core::{policy_package::PolicyPackage, vendor::plugin::RailPlugin};
use etherfuse::PACKAGE as PACKAGE_1;

#[rustfmt::skip]
pub(super) static RAILS: &[&dyn RailPlugin] = &[
    &allowit_stellar::PLUGIN,
];

/// Each imported package, under its import alias.
pub(super) static PACKAGES: &[&PolicyPackage] = &[&PACKAGE_1];
