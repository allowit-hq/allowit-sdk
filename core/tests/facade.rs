//! Compiling this target type-checks the documented example policies
//! against the facade; the modules need no test body.
extern crate allowit_sdk as allowit;

#[allow(dead_code)]
mod research {
    include!("../../docs/examples/research.rs");
}
#[allow(dead_code)]
mod approval {
    include!("../../docs/examples/approval.rs");
}
#[allow(dead_code)]
mod green {
    include!("../../docs/examples/green-investments.rs");
}
