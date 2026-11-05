//! Owner-bound generation through the selected compiled backend capability.
use crate::{
    error::{Error, Result},
    wallet::LocalSigner,
};

pub use allowit_sdk::vendor::generation::{GenerateRequest, GeneratedDraft, Target};

pub fn generate(
    backend: &str,
    origin: &str,
    signer: &LocalSigner,
    request: &GenerateRequest,
    mut clock: impl FnMut() -> Result<u64>,
) -> Result<GeneratedDraft> {
    if (5..=3000).contains(&request.prompt.len()) {
        crate::routing::info(&request.network)?;
    }
    request.validate()?;
    if signer.network() != crate::routing::normalize(&request.network)? {
        return Err(Error::config(
            "Owner signer belongs to another selected rail",
        ));
    }
    crate::vendors::generate(backend, origin, signer, request, &mut clock)
}
