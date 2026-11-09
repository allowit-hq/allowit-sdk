//! Vendor-neutral test harness. Shared tests run each case on every registered rail and skip a
//! rail that declares the feature unsupported. Values come from the rail's [`Fixtures`].
use super::{Feature, Fixtures, RailPlugin, rails};

/// Runs `f` once per registered rail that supports `feature`, with that rail's fixtures.
pub fn each(feature: Feature, mut f: impl FnMut(&'static dyn RailPlugin, &Fixtures)) {
    assert!(!rails().is_empty(), "no registered rail");
    let mut ran = 0;
    for rail in rails() {
        let support = rail.test_support();
        if let Some(why) = support.skip(feature) {
            eprintln!("skip {} {feature:?}: {why}", rail.id());
            continue;
        }
        f(*rail, &(support.fixtures)());
        ran += 1;
    }
    eprintln!("{feature:?}: ran on {ran} rail(s)");
}

/// The test network of the first registered rail, for unit tests that need any valid rail id.
pub fn network() -> &'static str {
    rails()
        .first()
        .expect("no registered rail")
        .test_support()
        .network
}
