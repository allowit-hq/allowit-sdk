//! Owner-bound generation through the selected compiled backend capability.
use crate::{
    error::{Error, Result},
    wallet::LocalSigner,
};

pub use allowit_core::vendor::generation::{GeneratedDraft, Target};
#[derive(Clone, Debug)]
pub struct GenerateRequest {
    pub prompt: String,
    pub network: String,
    pub profile: String,
    pub target: Target,
}
impl GenerateRequest {
    pub fn validate(&self) -> Result<()> {
        allowit_core::vendor::generation::validate_fields(
            &self.prompt,
            &self.network,
            &self.profile,
            &self.target,
            |network| crate::routing::info(network).map(|_| ()),
        )
    }
}
impl From<&GenerateRequest> for allowit_core::vendor::generation::GenerateRequest {
    fn from(request: &GenerateRequest) -> Self {
        Self {
            prompt: request.prompt.clone(),
            network: request.network.clone(),
            profile: request.profile.clone(),
            target: request.target.clone(),
        }
    }
}
impl<'a> From<&'a GenerateRequest>
    for std::borrow::Cow<'a, allowit_core::vendor::generation::GenerateRequest>
{
    fn from(request: &'a GenerateRequest) -> Self {
        Self::Owned(request.into())
    }
}

pub fn generate(
    backend: &str,
    origin: &str,
    signer: &LocalSigner,
    request: &GenerateRequest,
    mut clock: impl FnMut() -> Result<u64>,
) -> Result<GeneratedDraft> {
    request.validate()?;
    if signer.network() != crate::routing::normalize(&request.network)? {
        return Err(Error::config(
            "Owner signer belongs to another selected rail",
        ));
    }
    crate::vendors::generate(backend, origin, signer, request, &mut clock)
}
