//! Allowit backend requests and service composition, with no terminal I/O.
//! Callers pass arguments and readers in and receive values or `String` output.
#[cfg(test)]
extern crate self as allowit_sdk;
/// Core-owned portable policy values, rules and evaluator used by SDK callers.
pub use allowit as portable;
pub use allowit_core::client::error;
/// Core-backed private proof storage accepted by SDK owner workflows.
pub use allowit_core::client::store::Store;
/// Stable Core digest used for source and caller-owned record identities.
pub use allowit_core::digest;
/// JSON representation of a Core portable policy value.
pub use allowit_core::policy::PolicyValue;
/// Bounded HTTP/RPC transport used by caller-owned service adapters.
pub use allowit_core::transport;
/// Core's canonical function contract for SDK projections and service descriptions.
pub mod policy_metadata {
    pub use allowit_core::policy::{
        CanonicalFunctionDescriptor, CanonicalFunctionRegistry, IntegerRange, ParameterDescriptor,
        ResultDescriptor, canonical_function_metadata_hash, canonical_function_registry,
    };
}
pub mod authoring;
pub mod config;
pub mod execution;
pub mod generation;
pub mod input;
pub mod output;
pub mod owner;
pub mod owner_session;
pub mod parameter_reads;
pub mod parameters;
pub mod profiles;
pub mod request;
mod request_proof;
pub mod routing;
pub mod runtime;
pub mod typed_request;
pub mod vendors;
pub mod wallet;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
