//! Allowit backend requests and service composition, with no terminal I/O.
//! Callers pass arguments and readers in and receive values or `String` output.
pub use allowit_sdk::client::error;
pub mod authoring;
pub mod config;
pub mod execution;
pub mod generation;
pub mod input;
pub mod output;
pub mod owner;
pub mod profiles;
pub mod request;
pub mod routing;
#[cfg(feature = "stellar")]
pub mod stellar;
pub mod typed_request;
mod vendors;
pub mod wallet;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
