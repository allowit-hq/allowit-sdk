//! Owner-bound generation through the selected compiled backend capability.
use crate::{
    error::{Error, Result},
    wallet::LocalSigner,
};

pub use allowit_core::vendor::generation::{GeneratedDraft, Target, wallet_kind_of};
pub use allowit_core::vendor::kinds::WalletKind;
#[derive(Clone, Debug)]
pub struct GenerateRequest {
    pub prompt: String,
    pub network: String,
    pub profile: String,
    pub target: Target,
    /// The wallet kind that the owner chose. The rail sends it to the backend as the top-level
    /// `walletKind`; the backend binds it or refuses with a typed error whose `details` the
    /// error keeps. `None` lets the backend propose a kind.
    pub wallet_kind: Option<WalletKind>,
}
impl GenerateRequest {
    pub fn validate(&self) -> Result<()> {
        allowit_core::vendor::generation::validate_fields(
            &self.prompt,
            &self.network,
            &self.profile,
            &self.target,
            |network| crate::routing::info(network).map(|_| ()),
        )?;
        let Some(kind) = self.wallet_kind else {
            return Ok(());
        };
        // A rail that does not offer the kind, or a profile of the rail that installs another
        // kind, cannot serve the choice: stop before any request.
        let network = crate::routing::normalize(&self.network)?;
        let rail = crate::plugins::for_network(network)?;
        if !rail.wallet_kinds(network).contains(&kind) {
            return Err(Error::usage(format!(
                "The {} wallet kind is not offered on {network}",
                kind.id()
            )));
        }
        let installs = rail
            .server()
            .profiles()
            .iter()
            .find(|profile| profile.id == self.profile)
            .and_then(|profile| profile.wallet_kind());
        if installs.is_some_and(|installed| installed != kind) {
            return Err(Error::usage(format!(
                "The selected wallet profile does not install the {} wallet kind",
                kind.id()
            )));
        }
        Ok(())
    }
}
impl From<&GenerateRequest> for allowit_core::vendor::generation::GenerateRequest {
    fn from(request: &GenerateRequest) -> Self {
        Self {
            prompt: request.prompt.clone(),
            network: request.network.clone(),
            profile: request.profile.clone(),
            target: request.target.clone(),
            wallet_kind: request.wallet_kind,
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
    clock: impl FnMut() -> Result<u64>,
) -> Result<GeneratedDraft> {
    request.validate()?;
    if signer.network() != crate::routing::normalize(&request.network)? {
        return Err(Error::config(
            "Owner signer belongs to another selected rail",
        ));
    }
    let clock = std::cell::RefCell::new(clock);
    let cx = allowit_core::vendor::plugin::GenerateContext {
        backend,
        origin,
        signer,
        clock: &|| (clock.borrow_mut())(),
    };
    crate::plugins::for_network(&request.network)?.generate(&cx, &request.into())
}
