//! Stellar adapter registration and native workflow bindings.
pub(super) mod execution;
pub(super) mod formats;
#[cfg(test)]
mod input_tests;
pub(super) mod owner;
pub(super) mod profiles;
pub(super) mod request;
pub mod runtime;
pub(super) mod server;
pub(crate) mod server_owner;
mod typed;
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
    if crate::routing::family(signer.network()).map_err(|_| "Unknown signing network")? != FAMILY {
        return Err("Signer does not serve the selected native protocol".into());
    }
    signer
        .sign_hash(digest)
        .map_err(|_| "Local signer failed")?
        .try_into()
        .map_err(|_| "Invalid native signature size".into())
}

impl crate::vendors::stellar::runtime::owner_client::OwnerSigner for LocalSigner {
    fn account(&self) -> &str {
        self.address()
    }
    fn sign_hash(&self, digest: &[u8; 32]) -> native::Result<[u8; 64]> {
        signature(self, digest)
    }
}
impl crate::vendors::stellar::runtime::client::AgentSigner for LocalSigner {
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
    let mut client = crate::vendors::stellar::runtime::owner_client::OwnerClient::new(
        backend,
        origin,
        signer.address(),
    )?;
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

use crate::input::asset::Token;
const TESTNET: &[Token] = &[
    Token {
        code: "XLM",
        decimals: 7,
        native: true,
        issuer: None,
    },
    Token {
        code: "USDC",
        decimals: 7,
        native: false,
        issuer: Some("GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5"),
    },
];
const PUBNET: &[Token] = &[
    Token {
        code: "XLM",
        decimals: 7,
        native: true,
        issuer: None,
    },
    Token {
        code: "USDC",
        decimals: 7,
        native: false,
        issuer: Some("GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN"),
    },
];

pub(crate) fn tokens_for(rail: &str) -> &'static [Token] {
    if rail == "stellar:testnet" {
        TESTNET
    } else {
        PUBNET
    }
}

pub(super) const FAMILY: crate::routing::Family = crate::routing::Family("stellar");
pub(super) fn networks() -> Vec<crate::routing::RailInfo> {
    native::networks::NETWORKS
        .iter()
        .map(|n| crate::routing::RailInfo {
            id: n.id,
            label: if n.testnet {
                "Stellar Testnet"
            } else {
                "Stellar Mainnet"
            },
            aliases: if n.testnet {
                &["stellar-testnet"]
            } else {
                &["stellar:mainnet", "stellar-mainnet"]
            },
            family: FAMILY,
            testnet: n.testnet,
        })
        .collect()
}
pub(super) fn capabilities(network: &str) -> crate::execution::Capabilities {
    crate::execution::Capabilities {
        curve: "ed25519",
        address_is_key: true,
        operations: &["transfer", "deposit", "redeem"],
        build_only: true,
        verbs: if crate::routing::info(network).is_ok_and(|v| v.testnet) {
            &[
                "keys fund",
                "wallet setup",
                "wallet import",
                "policy deploy",
                "policy fund",
                "policy activate",
                "policy export",
                "policy owner-status",
                "wallet show",
                "policy show",
                "policy generate",
                "exec",
                "status",
            ]
        } else {
            &[
                "wallet import",
                "policy deploy",
                "policy fund",
                "policy activate",
                "policy export",
                "policy owner-status",
                "wallet show",
                "policy show",
                "policy generate",
                "exec",
                "status",
            ]
        },
    }
}
pub(super) fn fund_key(network: &str, address: &str, endpoint: Option<&str>) -> Result<String> {
    if !crate::routing::info(network)?.testnet {
        return Err(Error::unsupported(
            "The selected network has no test faucet",
        ));
    }
    native::owner::fund_testnet_account(address, endpoint).map_err(|message| {
        Error::uncertain(format!("{message}; check the balance before retrying"))
    })
}
