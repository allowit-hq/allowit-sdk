//! Compiled vendor bindings. Native implementations remain in their vendor libraries.
mod stellar;

use crate::{error::Result, routing};
use zeroize::Zeroizing;

pub(crate) trait PrivateSigner: Send + Sync {
    fn address(&self) -> &str;
    fn public_key(&self) -> &str;
    fn key_type(&self) -> &str;
    fn encoded_secret(&self) -> Zeroizing<String>;
    fn sign_hash(&self, digest: &[u8]) -> Result<Vec<u8>>;
}

pub(crate) trait KeyProvider: Sync {
    fn entropy_bytes(&self) -> usize;
    fn derive(&self, entropy: &[u8]) -> Result<Box<dyn PrivateSigner>>;
    fn parse(&self, secret: &str) -> Result<Box<dyn PrivateSigner>>;
    fn validate_identity(&self, address: &str, public_key: &str, key_type: &str) -> Result<()>;
}

pub(crate) fn keys(network: &str) -> Result<&'static dyn KeyProvider> {
    match routing::family(network)? {
        routing::Family::Stellar => Ok(&stellar::KEYS),
    }
}

pub(crate) fn generate(
    backend: &str,
    origin: &str,
    signer: &crate::wallet::LocalSigner,
    request: &crate::generation::GenerateRequest,
    clock: &mut dyn FnMut() -> Result<u64>,
) -> Result<crate::generation::GeneratedDraft> {
    match routing::family(&request.network)? {
        routing::Family::Stellar => stellar::generate(backend, origin, signer, request, clock),
    }
}
