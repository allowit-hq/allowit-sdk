//! Allowit backend requests and service composition, with no terminal I/O.
//! Callers pass arguments and readers in and receive values or `String` output.
pub use allowit_sdk::client::error;
pub mod config;
pub mod output;
pub mod request;
pub mod routing;
pub mod typed_request;
pub mod wallet;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
