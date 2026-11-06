//! Owner-bound generation through the selected compiled backend capability.
use crate::{
    error::{Error, Result},
    wallet::LocalSigner,
};
use serde_json::Value;

#[derive(Clone, Debug)]
pub enum Target {
    NewWallet,
    Installation(String),
}
#[derive(Clone, Debug)]
pub struct GenerateRequest {
    pub prompt: String,
    pub network: String,
    pub profile: String,
    pub target: Target,
}
impl GenerateRequest {
    pub fn validate(&self) -> Result<()> {
        if !(5..=3000).contains(&self.prompt.len()) {
            return Err(Error::usage("Describe the policy in 5–3000 UTF-8 bytes"));
        }
        crate::routing::info(&self.network)?;
        if self.profile.is_empty()
            || self.profile.len() > 128
            || !self
                .profile
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
        {
            return Err(Error::usage("Select an advertised wallet profile"));
        }
        if let Target::Installation(id) = &self.target
            && (id.len() != 64
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        {
            return Err(Error::config("Invalid lowercase installation identity"));
        }
        Ok(())
    }
}

/// A fully checked backend document and its remaining required values.
/// The selected capability validates its native binding before returning this document.
pub struct GeneratedDraft {
    pub document: Value,
    pub unresolved: Vec<String>,
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
