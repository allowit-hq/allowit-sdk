//! Allowit backend requests and service composition, with no terminal I/O.
//! Callers pass arguments and readers in and receive values or `String` output.
#[cfg(test)]
extern crate self as allowit_sdk_engine;
pub use allowit_sdk::client::error;
pub mod authoring;
pub mod config;
pub mod execution;
pub mod generation;
pub mod input;
pub mod output;
pub mod owner;
pub mod parameter_reads;
pub mod parameters;
pub mod profiles;
pub mod request;
pub mod routing;
pub mod runtime;
pub mod typed_request;
pub mod vendors;
pub mod wallet;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
