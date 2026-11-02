//! Registration shims for Stellar-owned key and signature semantics.
use super::{KeyProvider, PrivateSigner};
use crate::{
    error::{Error, Result},
    wallet::LocalSigner,
};
use allowit_stellar_wallet as native;
use zeroize::Zeroizing;

pub(super) static KEYS: Keys = Keys;
pub(super) struct Keys;
struct Key(native::signing::LocalSigner);

impl KeyProvider for Keys {
    fn entropy_bytes(&self) -> usize {
        native::signing::KEY_ENTROPY_BYTES
    }
    fn derive(&self, entropy: &[u8]) -> Result<Box<dyn PrivateSigner>> {
        native::signing::LocalSigner::from_entropy(entropy)
            .map(|key| Box::new(Key(key)) as Box<dyn PrivateSigner>)
            .map_err(Error::config)
    }
    fn parse(&self, secret: &str) -> Result<Box<dyn PrivateSigner>> {
        native::signing::LocalSigner::from_encoded(secret)
            .map(|key| Box::new(Key(key)) as Box<dyn PrivateSigner>)
            .map_err(Error::config)
    }
    fn validate_identity(&self, address: &str, public_key: &str, key_type: &str) -> Result<()> {
        native::signing::validate_key_identity(address, public_key, key_type).map_err(Error::usage)
    }
}

impl PrivateSigner for Key {
    fn address(&self) -> &str {
        self.0.account()
    }
    fn public_key(&self) -> &str {
        self.0.account()
    }
    fn key_type(&self) -> &str {
        native::signing::KEY_TYPE
    }
    fn encoded_secret(&self) -> Zeroizing<String> {
        self.0.encoded_secret()
    }
    fn sign_hash(&self, digest: &[u8]) -> Result<Vec<u8>> {
        self.0.sign_digest(digest).map_err(Error::config)
    }
}

fn signature(signer: &LocalSigner, digest: &[u8; 32]) -> native::Result<[u8; 64]> {
    if crate::routing::family(signer.network()).map_err(|_| "Unknown signing network")?
        != crate::routing::Family::Stellar
    {
        return Err("Signer does not serve the selected native protocol".into());
    }
    signer
        .sign_hash(digest)
        .map_err(|_| "Local signer failed")?
        .try_into()
        .map_err(|_| "Invalid native signature size".into())
}

impl crate::stellar::owner_client::OwnerSigner for LocalSigner {
    fn account(&self) -> &str {
        self.address()
    }
    fn sign_hash(&self, digest: &[u8; 32]) -> native::Result<[u8; 64]> {
        signature(self, digest)
    }
}
impl crate::stellar::client::AgentSigner for LocalSigner {
    fn account(&self) -> &str {
        self.address()
    }
    fn sign_hash(&self, digest: &[u8; 32]) -> native::Result<[u8; 64]> {
        signature(self, digest)
    }
}

pub(super) fn generate(
    backend: &str,
    origin: &str,
    signer: &LocalSigner,
    request: &crate::generation::GenerateRequest,
    clock: &mut dyn FnMut() -> Result<u64>,
) -> Result<crate::generation::GeneratedDraft> {
    let mut client =
        crate::stellar::owner_client::OwnerClient::new(backend, origin, signer.address())?;
    client.authenticate(signer, clock()?)?;
    let policy = client.generate(request, clock()?)?;
    let unresolved = policy.runtime.instance_binding.portable.unresolved.clone();
    let document = serde_json::to_value(policy)
        .map_err(|_| Error::config("Cannot encode generated policy"))?;
    Ok(crate::generation::GeneratedDraft {
        document,
        unresolved,
    })
}
