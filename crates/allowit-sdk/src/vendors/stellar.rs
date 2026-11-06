//! Stellar provider registration and public compatibility reexports.
pub use allowit_stellar_wallet::provider::*;
pub mod runtime {
    pub use allowit_stellar_wallet::provider::runtime::*;
    pub mod owner_client {
        pub use crate::generation::{GenerateRequest, Target};
        pub use allowit_stellar_wallet::provider::runtime::owner_client::*;
    }
}
