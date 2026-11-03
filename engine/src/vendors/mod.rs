//! Mechanical compiled registrations. Native semantics remain in vendor libraries.
#[cfg(feature = "stellar")]
mod stellar;
use crate::{
    error::{Error, Result},
    routing,
};
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

pub(crate) trait OwnerProvider {
    fn deploy(
        &self,
        cx: &crate::owner::Context<'_>,
        r: crate::owner::Deploy<'_>,
    ) -> Result<serde_json::Value>;
    fn fund(
        &self,
        cx: &crate::owner::Context<'_>,
        document: &[u8],
        id: &str,
        amount: &str,
        fee: &str,
    ) -> Result<serde_json::Value>;
    fn activate(
        &self,
        cx: &crate::owner::Context<'_>,
        r: crate::owner::Activate<'_>,
    ) -> Result<serde_json::Value>;
    fn export(
        &self,
        cx: &crate::owner::Context<'_>,
        document: &[u8],
        id: &str,
    ) -> Result<crate::profiles::Profile>;
    fn setup(
        &self,
        cx: &crate::owner::Context<'_>,
        r: crate::owner::Setup<'_>,
    ) -> Result<serde_json::Value>;
    fn status(
        &self,
        root: &std::path::Path,
        network: &str,
        rpc: &str,
        id: &str,
    ) -> Result<serde_json::Value>;
}

fn unavailable() -> Error {
    Error::unsupported("The selected build has no matching vendor capability")
}
pub(crate) fn networks() -> Vec<routing::RailInfo> {
    #[cfg(feature = "stellar")]
    {
        stellar::networks()
    }
    #[cfg(not(feature = "stellar"))]
    {
        Vec::new()
    }
}
pub(crate) fn keys(network: &str) -> Result<&'static dyn KeyProvider> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => Ok(&stellar::KEYS),
        _ => Err(unavailable()),
    }
}
pub(crate) fn owner(network: &str) -> Result<&'static dyn OwnerProvider> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => Ok(&stellar::owner::OWNER),
        _ => Err(unavailable()),
    }
}
pub(crate) fn generate(
    _backend: &str,
    _origin: &str,
    _signer: &crate::wallet::LocalSigner,
    request: &crate::generation::GenerateRequest,
    _clock: &mut dyn FnMut() -> Result<u64>,
) -> Result<crate::generation::GeneratedDraft> {
    match routing::family(&request.network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::generate(_backend, _origin, _signer, request, _clock),
        _ => Err(unavailable()),
    }
}
pub(crate) fn formats(family: routing::Family) -> Result<&'static crate::input::Formats> {
    match family {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => Ok(&stellar::formats::FORMATS),
        _ => Err(unavailable()),
    }
}
pub(crate) enum ProfileBinding {
    #[cfg(feature = "stellar")]
    Stellar(crate::stellar::client::Bundle),
}
pub(crate) fn decode_profile(
    network: &str,
    _bytes: &[u8],
) -> Result<(crate::profiles::Description, ProfileBinding)> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::profiles::decode(network, _bytes),
        _ => Err(unavailable()),
    }
}
pub(crate) fn execution_tokens(network: &str) -> Result<&'static [crate::input::asset::Token]> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => Ok(stellar::tokens_for(network)),
        _ => Err(unavailable()),
    }
}
pub(crate) fn check_execution(
    network: &str,
    _op: &str,
    _to: &str,
    _minimum: Option<&crate::input::asset::Amount>,
    _args: bool,
) -> Result<()> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::request::check(_op, _to, _minimum, _args),
        _ => Err(unavailable()),
    }
}
pub(crate) fn build_execution(
    p: &crate::profiles::Profile,
    _root: &std::path::Path,
    _rpc: Option<&str>,
    _r: &crate::execution::Request,
) -> Result<serde_json::Value> {
    match routing::family(&p.description().network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::execution::build(p, _root, _rpc, _r),
        _ => Err(unavailable()),
    }
}
pub(crate) fn execute(
    p: crate::profiles::Profile,
    _s: crate::wallet::LocalSigner,
    _root: &std::path::Path,
    _rpc: Option<String>,
    _r: &crate::execution::Request,
) -> Result<crate::execution::Outcome> {
    match routing::family(&p.description().network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::execution::execute(p, _s, _root, _rpc, _r),
        _ => Err(unavailable()),
    }
}
pub(crate) fn execution_status(
    p: crate::profiles::Profile,
    _s: crate::wallet::LocalSigner,
    _root: &std::path::Path,
    _rpc: Option<String>,
    _id: &str,
    _wait: std::time::Duration,
) -> Result<crate::execution::Outcome> {
    match routing::family(&p.description().network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::execution::status(p, _s, _root, _rpc, _id, _wait),
        _ => Err(unavailable()),
    }
}
pub(crate) fn capabilities(network: &str) -> Result<crate::execution::Capabilities> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => Ok(stellar::capabilities(network)),
        _ => Err(unavailable()),
    }
}
pub(crate) fn fund_key(network: &str, _address: &str, _endpoint: Option<&str>) -> Result<String> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::fund_key(network, _address, _endpoint),
        _ => Err(unavailable()),
    }
}

pub(crate) fn verify_profile_import(_binding: &ProfileBinding, _endpoint: &str) -> Result<()> {
    #[cfg(feature = "stellar")]
    {
        let ProfileBinding::Stellar(b) = _binding;
        b.verify_import(_endpoint)
    }
    #[cfg(not(feature = "stellar"))]
    {
        Err(unavailable())
    }
}

pub(crate) fn verify_server_role(_binding: &ProfileBinding, _server: &str) -> Result<()> {
    #[cfg(feature = "stellar")]
    {
        let ProfileBinding::Stellar(b) = _binding;
        b.verify_server_role(_server)
    }
    #[cfg(not(feature = "stellar"))]
    {
        Err(unavailable())
    }
}

pub(crate) enum RecoveryBinding {
    #[cfg(feature = "stellar")]
    Stellar(crate::stellar::client::Retained),
}
pub(crate) fn recovery_binding(binding: &RecoveryBinding) -> crate::execution::Binding {
    match *binding {
        #[cfg(feature = "stellar")]
        RecoveryBinding::Stellar(ref r) => crate::execution::Binding {
            wallet: r.config().wallet.clone(),
            policy: format!(
                "{}@{}",
                r.policy_id(),
                allowit_stellar_wallet::hex(&r.config().module_digest)
            ),
        },
    }
}
pub(crate) fn recovery_signed(binding: &RecoveryBinding) -> bool {
    match *binding {
        #[cfg(feature = "stellar")]
        RecoveryBinding::Stellar(ref r) => r.signed(),
    }
}
pub(crate) fn retained(
    _root: &std::path::Path,
    network: &str,
    _wallet: Option<&str>,
    _id: &str,
) -> Result<Option<crate::execution::Recovery>> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::execution::retained(_root, network, _wallet, _id),
        _ => Err(unavailable()),
    }
}
pub(crate) fn recover(
    original: crate::execution::Recovery,
    _signer: crate::wallet::LocalSigner,
    _backend: &str,
    _root: &std::path::Path,
    _id: &str,
    _wait: std::time::Duration,
    _request: Option<&crate::execution::Request>,
) -> Result<crate::execution::Outcome> {
    match original.binding {
        #[cfg(feature = "stellar")]
        RecoveryBinding::Stellar(r) => {
            stellar::execution::recover(r, _signer, _backend, _root, _id, _wait, _request)
        }
    }
}

pub(crate) trait RuntimeProvider {
    fn import(
        &self,
        a: &crate::config::ServerAuthority,
        b: &[u8],
    ) -> Result<crate::runtime::Installation>;
    fn request(
        &self,
        i: &crate::runtime::Installation,
        id: [u8; 32],
        a: crate::runtime::Action,
        accounting: Option<&crate::runtime::Accounting>,
    ) -> Result<crate::runtime::RequestPacket>;
    fn read_request(
        &self,
        i: &crate::runtime::Installation,
        b: &[u8],
    ) -> Result<crate::runtime::RequestPacket>;
    fn snapshot(
        &self,
        i: &crate::runtime::Installation,
        a: Option<&crate::runtime::Accounting>,
    ) -> Result<crate::runtime::Snapshot>;
    fn action(
        &self,
        i: &crate::runtime::Installation,
        r: &crate::runtime::RequestPacket,
    ) -> Result<crate::runtime::Action>;
    fn resources(&self, i: &crate::runtime::Installation) -> Result<crate::runtime::Resources>;
    fn redeemable(&self, i: &crate::runtime::Installation, v: &str, shares: i128) -> Result<i128>;
    fn finish<'a>(
        &self,
        i: &crate::runtime::Installation,
        p: &crate::runtime::Prepared,
        retain: Box<dyn FnOnce(&crate::runtime::Prepared) -> Result<()> + 'a>,
    ) -> Result<crate::runtime::Prepared>;
    fn prepare(
        &self,
        i: &crate::runtime::Installation,
        r: &crate::runtime::RequestPacket,
        t: &crate::runtime::Terms,
        a: Option<&crate::runtime::Accounting>,
        v: crate::runtime::Validity,
    ) -> Result<crate::runtime::Prepared>;
    fn finalize<'a>(
        &self,
        i: &crate::runtime::Installation,
        p: &crate::runtime::Prepared,
        s: &dyn crate::runtime::Signer,
        e: Option<crate::runtime::Evaluator<'_>>,
        now: u64,
        retain: Box<dyn FnOnce(&crate::runtime::Prepared) -> Result<()> + 'a>,
    ) -> Result<crate::runtime::Prepared>;
    fn revalidate(
        &self,
        i: &crate::runtime::Installation,
        p: &crate::runtime::Prepared,
        now: u64,
    ) -> Result<crate::runtime::Prepared>;
    fn sign(
        &self,
        i: &crate::runtime::Installation,
        p: &crate::runtime::Prepared,
        s: &dyn crate::runtime::Signer,
        now: u64,
    ) -> Result<crate::runtime::Signature>;
    fn authorize(
        &self,
        i: &crate::runtime::Installation,
        p: &crate::runtime::Prepared,
        s: &[crate::runtime::Signature],
        now: u64,
    ) -> Result<crate::runtime::Signed>;
    fn submit(
        &self,
        s: &mut dyn crate::runtime::OperationStore,
        p: &crate::runtime::Signed,
        now: u64,
    ) -> Result<crate::runtime::Status>;
    fn recover(
        &self,
        s: &mut dyn crate::runtime::OperationStore,
        id: [u8; 32],
    ) -> Result<crate::runtime::Status>;
    fn observe(&self, p: &crate::runtime::Signed) -> Result<Option<crate::runtime::Receipt>>;
    fn observe_authorizations(
        &self,
        p: &[crate::runtime::Prepared],
    ) -> Result<Vec<crate::runtime::ObservedAuthorization>>;
    fn prepare_owner(
        &self,
        i: &crate::runtime::Installation,
        a: crate::runtime::OwnerAction,
        fee: u64,
        now: u64,
    ) -> Result<crate::runtime::OwnerPacket>;
    fn read_owner_signed(
        &self,
        p: &crate::runtime::OwnerPacket,
        signed_payload: &[u8],
    ) -> Result<crate::runtime::OwnerSigned>;
    fn submit_owner<'a>(
        &self,
        p: &crate::runtime::OwnerSigned,
        retain: Box<dyn FnOnce(&crate::runtime::OwnerSigned) -> Result<()> + 'a>,
        poll: crate::runtime::Poll,
        now: u64,
    ) -> Result<crate::runtime::Receipt>;
    fn observe_owner(
        &self,
        p: &crate::runtime::OwnerSigned,
    ) -> Result<Option<crate::runtime::Receipt>>;
}
pub(crate) fn runtime(network: &str, _endpoint: &str) -> Result<Box<dyn RuntimeProvider>> {
    match routing::family(network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => Ok(Box::new(stellar::server::Provider::new(
            network, _endpoint,
        )?)),
        _ => Err(unavailable()),
    }
}
pub(crate) fn runtime_policy(
    i: crate::runtime::PolicyInstallation,
) -> Result<crate::runtime::Installation> {
    match routing::family(&i.description.network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::server::policy(i),
        _ => Err(unavailable()),
    }
}
pub(crate) fn runtime_restore(
    k: crate::runtime::Kind,
    v: Box<serde_json::value::RawValue>,
) -> Result<crate::runtime::Packet> {
    #[cfg(feature = "stellar")]
    {
        stellar::server::restore(k, v)
    }
    #[cfg(not(feature = "stellar"))]
    {
        let _ = (k, v);
        Err(unavailable())
    }
}
pub(crate) fn runtime_info(
    p: &crate::runtime::Packet,
    _k: crate::runtime::Kind,
) -> Result<crate::runtime::PacketInfo> {
    match routing::family(&p.network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::server::info(p, _k),
        _ => Err(unavailable()),
    }
}

pub(crate) fn runtime_same_snapshot(
    a: &crate::runtime::Snapshot,
    b: &crate::runtime::Snapshot,
) -> Result<bool> {
    if a.evidence.network != b.evidence.network {
        return Ok(false);
    }
    match routing::family(&a.evidence.network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::server::same_snapshot(a, b),
        _ => Err(unavailable()),
    }
}
pub(crate) fn runtime_receipt_info(
    p: &crate::runtime::Packet,
) -> Result<crate::runtime::ReceiptInfo> {
    match routing::family(&p.network)? {
        #[cfg(feature = "stellar")]
        stellar::FAMILY => stellar::server::receipt_info(p),
        _ => Err(unavailable()),
    }
}
