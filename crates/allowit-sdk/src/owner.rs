//! Owner orchestration through the selected compiled capability. No terminal I/O or native DTOs.
use crate::{
    error::{Error, Result},
    plugins::{for_network, owner_store},
    routing,
    wallet::LocalSigner,
};
use std::path::Path;

pub struct Context<'a> {
    pub network: &'a str,
    pub backend: &'a str,
    pub authority: &'a crate::config::ServerAuthority,
    pub origin: &'a str,
    pub rpc_url: &'a str,
    pub root: &'a Path,
    pub signer: &'a LocalSigner,
    pub clock: &'a dyn Fn() -> Result<u64>,
}
impl Context<'_> {
    pub(crate) fn provider_context<'a>(
        &'a self,
        store: &'a dyn Fn() -> Result<crate::Store>,
    ) -> allowit_core::vendor::owner::Context<'a> {
        allowit_core::vendor::owner::Context {
            network: self.network,
            backend: self.backend,
            authority: &self.authority.0,
            origin: self.origin,
            rpc_url: self.rpc_url,
            store,
            signer: self.signer,
            clock: self.clock,
        }
    }
    fn check(&self) -> Result<()> {
        if self.authority.origin() != crate::config::parse_origin(self.backend)?
            || self.authority.network() != routing::normalize(self.network)?
        {
            return Err(Error::config(
                "Owner context differs from independently configured server authority",
            ));
        }
        if routing::normalize(self.network)? != self.signer.network() {
            return Err(Error::config(
                "Owner signer belongs to another selected rail",
            ));
        }
        Ok(())
    }
}
pub use allowit_core::vendor::owner::{
    Activate, Comparison, Counters, Deploy, EditParams, HostPlacement, NativePlacement,
    NativeWalletAsset, PairedExport, Revise, Setup, StateReadback, UpdateParams, Verification,
};
pub use allowit_core::vendor::runtime::ParamsUpdate;
pub fn deploy(cx: &Context<'_>, request: Deploy<'_>) -> Result<serde_json::Value> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    for_network(cx.network)?
        .owner()
        .deploy(&cx.provider_context(&store), request)
}
pub fn fund(
    cx: &Context<'_>,
    document: &[u8],
    id: &str,
    amount: &str,
    fee_limit: &str,
) -> Result<serde_json::Value> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    for_network(cx.network)?.owner().fund(
        &cx.provider_context(&store),
        document,
        id,
        amount,
        fee_limit,
    )
}
pub fn activate(cx: &Context<'_>, request: Activate<'_>) -> Result<serde_json::Value> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    for_network(cx.network)?
        .owner()
        .activate(&cx.provider_context(&store), request)
}
pub fn export(
    cx: &Context<'_>,
    document: &[u8],
    activation_id: &str,
) -> Result<crate::profiles::Profile> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    let raw = for_network(cx.network)?.owner().export(
        &cx.provider_context(&store),
        document,
        activation_id,
    )?;
    crate::profiles::Cache::open(cx.root)?
        .install(cx.network, cx.backend, raw)?
        .with_authority(cx.authority)
}
pub fn setup(cx: &Context<'_>, request: Setup<'_>) -> Result<serde_json::Value> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    for_network(cx.network)?
        .owner()
        .setup(&cx.provider_context(&store), request)
}
/// The asset of a new Native wallet on the context's network, read by the network's rail as its
/// [`setup`] reads it and refused where the setup would refuse it. The owner signs in to read it
/// and signs nothing else, so a host shows it before [`setup`] asks for the owner's signature.
pub fn native_wallet_asset(cx: &Context<'_>) -> Result<NativeWalletAsset> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    for_network(cx.network)?
        .owner()
        .native_wallet_asset(&cx.provider_context(&store))
}
pub fn status(root: &Path, network: &str, rpc_url: &str, id: &str) -> Result<serde_json::Value> {
    let store = owner_store(root, network)?;
    for_network(network)?
        .owner()
        .status(&store, network, rpc_url, id)
}
/// Change typed params of an unsigned draft through the backend. The reply is the next
/// revision of the draft. The backend refuses a value outside the param's type or range.
pub fn edit_params(cx: &Context<'_>, request: EditParams<'_>) -> Result<serde_json::Value> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    for_network(cx.network)?
        .owner()
        .edit_params(&cx.provider_context(&store), request)
}
/// Revise an unsigned draft in the owner's words. The reply is the next revision.
pub fn revise(cx: &Context<'_>, request: Revise<'_>) -> Result<serde_json::Value> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    for_network(cx.network)?
        .owner()
        .revise(&cx.provider_context(&store), request)
}
/// Change installed params on an active wallet with one owner signature. The workflow is
/// resume-safe under its request ID, as `deploy` and `fund` are. The outcome selects the
/// updated installation.
pub fn update_params(cx: &Context<'_>, request: UpdateParams<'_>) -> Result<serde_json::Value> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    for_network(cx.network)?
        .owner()
        .update_params(&cx.provider_context(&store), request)
}
/// One owner route that needs no rail step, such as the owner's reputation vote. The SDK signs
/// the owner in ([`crate::owner_session::Session`]): the rail of the network only signs the
/// sign-in message, as its wallets sign a text message. Then the SDK posts `body` to `path` under
/// that session. `path` is an API route of the backend ([`check_route`]). The reply is the
/// server's JSON.
pub(crate) fn call(
    cx: &Context<'_>,
    path: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value> {
    cx.check()?;
    check_route(path)?;
    let signature = for_network(cx.network)?
        .message_signature()
        .ok_or_else(|| {
            Error::unsupported("This rail signs no text message, so the owner cannot sign in here")
        })?;
    let session = crate::owner_session::Session::sign_in(
        cx.authority.origin(),
        cx.origin,
        cx.network,
        cx.signer,
        signature,
        (cx.clock)()?,
    )?;
    // The sign-in took time: the session's expiry is checked against the clock now.
    session.post(path, body, (cx.clock)()?)
}
/// The path of an owner route: an API route of the configured backend, never another origin or
/// another route after URL normalization. The route holds letters, digits, `_`, `-` and single
/// `/` only, so no dot segment, encoded character or backslash can climb out of `/api/`. The
/// query holds letters, digits and `%`, `=`, `&`, `_`, `-`, `.`, `:`.
pub(crate) fn check_route(path: &str) -> Result<()> {
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    let route_ok = route.starts_with("/api/")
        && route.len() > "/api/".len()
        && !route.contains("//")
        && route
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-'));
    let query_ok = query.bytes().all(|b| {
        b.is_ascii_alphanumeric() || matches!(b, b'%' | b'=' | b'&' | b'_' | b'-' | b'.' | b':')
    });
    if path.len() > 512 || !route_ok || !query_ok {
        return Err(Error::usage(
            "An owner call names an API route of the backend",
        ));
    }
    Ok(())
}
/// Compare the owner's installation document with the ledger and read the wallet state back.
/// `Verification::matches` is true only when every comparison matches.
pub fn verify(cx: &Context<'_>, document: &[u8]) -> Result<Verification> {
    cx.check()?;
    let store = || owner_store(cx.root, cx.network);
    for_network(cx.network)?
        .owner()
        .verify(&cx.provider_context(&store), document)
}
/// The host and native placements of a draft or installed policy document: what the server
/// evaluates and what the wallet enforces.
pub fn paired_export(network: &str, document: &[u8]) -> Result<PairedExport> {
    let network = routing::normalize(network)?;
    for_network(network)?
        .owner()
        .paired_export(network, document)
}
/// The typed values of the owner's text edits for a draft, or for the outcome of a deployment,
/// funding or params update, of the rail of `network`, with each change as the owner reads it.
/// An outcome holds its policy and installation under `selection`; its installed params are
/// the ones that are edited. `edits` maps a param key to text in human units, such as `3.5`
/// for an amount or `2026-12-31` for an expiry. The checks are those of
/// [`crate::parameters::set_all`]: type, range, a future expiry and limits that fit each
/// other. The backend checks the values again.
pub fn typed_changes(
    network: &str,
    document: &serde_json::Value,
    edits: &std::collections::BTreeMap<String, String>,
    now: i64,
) -> Result<(
    std::collections::BTreeMap<String, crate::PolicyValue>,
    Vec<crate::parameters::Change>,
)> {
    let network = routing::normalize(network)?;
    let rail = for_network(network)?;
    // The document itself, the selection of an outcome, or the policy of that selection:
    // the first one whose params the rail reads.
    let selection = document.get("selection");
    let policy = selection.and_then(|selection| selection.get("policy"));
    let mut read = rail.draft_params(network, document);
    for candidate in [selection, policy].into_iter().flatten() {
        if read.is_ok() {
            break;
        }
        read = rail.draft_params(network, candidate);
    }
    let read = read?;
    let code = [Some(document), policy, selection]
        .into_iter()
        .flatten()
        .find_map(|part| part.get("code").and_then(serde_json::Value::as_str))
        .ok_or_else(|| Error::usage("The document holds no policy source"))?;
    let known = crate::parameters::known_assets(network)?;
    // The portable binding of the policy declares the kind of each param that is not canonical,
    // such as an amount. Without it such a value would parse as a plain number.
    let kinds = [Some(document), policy]
        .into_iter()
        .flatten()
        .find_map(|part| part.pointer("/runtime/instanceBinding/portable/kinds"))
        .map(|kinds| {
            serde_json::from_value(kinds.clone())
                .map_err(|_| Error::usage("The document declares invalid param kinds"))
        })
        .transpose()?
        .unwrap_or_default();
    let draft = crate::parameters::Parameters {
        params: read.params.iter().cloned().collect(),
        kinds,
        asset: crate::parameters::Asset {
            address: known
                .iter()
                .find(|asset| asset.symbol == read.asset.symbol)
                .map(|asset| asset.address.clone())
                .unwrap_or_default(),
            symbol: read.asset.symbol.clone(),
            decimals: u32::from(read.asset.decimals),
        },
        known,
        unresolved: Vec::new(),
    };
    let (next, changes) =
        crate::parameters::set_all(&draft, code, edits, now).map_err(Error::usage)?;
    let typed = changes
        .iter()
        .map(|change| {
            let value = next
                .params
                .get(&change.key)
                .cloned()
                .ok_or_else(|| Error::config("The edit dropped a param"))?;
            serde_json::from_value(value)
                .map(|value| (change.key.clone(), value))
                .map_err(|_| Error::config("The edit produced an untyped value"))
        })
        .collect::<Result<_>>()?;
    Ok((typed, changes))
}

/// What the ledger shows of an installed wallet: whether its owner closed or paused it, at the
/// observed height. This read, never a stored status, says whether a policy is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalletObservation {
    /// The wallet is gone from its network: its owner closed it.
    pub closed: bool,
    /// The agent cannot pay: the owner paused the wallet, or closed it.
    pub paused: bool,
    pub observed_height: u64,
}
impl WalletObservation {
    /// `active`, `stopped` or `closed`.
    pub fn state(&self) -> &'static str {
        match (self.closed, self.paused) {
            (true, _) => "closed",
            (false, true) => "stopped",
            (false, false) => "active",
        }
    }
}
/// What the ledger shows of the wallet of `configuration` now ([`WalletObservation`]): a closed
/// wallet (`Runtime::wallet_closed`) is closed and paused; another is paused as its snapshot shows
/// it. `accounting` is the server's account of a wallet whose server keeps one.
pub fn observe_wallet(
    runtime: &crate::runtime::Runtime,
    configuration: &crate::runtime::owner::Configuration,
    accounting: Option<&crate::runtime::Accounting>,
) -> Result<WalletObservation> {
    let installation = crate::runtime::Installation::from_configuration(configuration);
    if runtime.wallet_closed(&installation)? {
        return Ok(WalletObservation {
            closed: true,
            paused: true,
            observed_height: runtime.current_height()?,
        });
    }
    let snapshot = runtime.discover_snapshot(configuration, accounting)?;
    Ok(WalletObservation {
        closed: false,
        paused: snapshot.paused,
        observed_height: snapshot.observed_height,
    })
}
/// A step of an owner's stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopStep {
    /// Pause the wallet: its agent can no longer pay.
    Pause,
    /// Return every spendable amount to the owner. The wallet stays.
    Return,
}
/// The step of a stop that the ledger asks for next: the pause of a wallet whose agent can pay,
/// then the return of a paused one. A closed wallet has no step. The rail says whether a paused
/// wallet still holds something to return.
pub fn stop_step(seen: &WalletObservation) -> Option<StopStep> {
    match (seen.closed, seen.paused) {
        (true, _) => None,
        (false, false) => Some(StopStep::Pause),
        (false, true) => Some(StopStep::Return),
    }
}
/// Run the preparation of an owner step, `prepare`, once more when its reads of the ledger
/// crossed a ledger (`STALE_OWNER_OBSERVATION`): nothing was signed or retained. The typed code
/// decides, never the message; a second stale read is the answer.
pub fn retry_stale_observation<T>(mut prepare: impl FnMut() -> Result<T>) -> Result<T> {
    match prepare() {
        Err(error)
            if error.api_code.as_deref() == Some(crate::runtime::STALE_OWNER_OBSERVATION) =>
        {
            prepare()
        }
        result => result,
    }
}

#[cfg(test)]
mod stop_tests {
    use super::*;
    fn seen(closed: bool, paused: bool) -> WalletObservation {
        WalletObservation {
            closed,
            paused,
            observed_height: 5,
        }
    }
    /// The stop walk: an active wallet pauses, a paused one returns, a closed one has no step.
    #[test]
    fn the_stop_walk_pauses_then_returns_and_ends_at_a_closed_wallet() {
        assert_eq!(stop_step(&seen(false, false)), Some(StopStep::Pause));
        assert_eq!(stop_step(&seen(false, true)), Some(StopStep::Return));
        assert_eq!(stop_step(&seen(true, true)), None);
        assert_eq!(stop_step(&seen(true, false)), None);
        assert_eq!(
            [seen(false, false), seen(false, true), seen(true, true)].map(|s| s.state()),
            ["active", "stopped", "closed"]
        );
    }
    /// A stale observation is prepared once more, by its typed code alone; a second one is the
    /// answer, and another refusal is never retried.
    #[test]
    fn a_stale_observation_is_prepared_once_more() {
        let stale = || {
            let mut e = Error::config("Owner observation is stale");
            e.api_code = Some(crate::runtime::STALE_OWNER_OBSERVATION.into());
            e
        };
        let mut calls = 0;
        let prepared = retry_stale_observation(|| {
            calls += 1;
            if calls == 1 {
                Err(stale())
            } else {
                Ok("unsigned")
            }
        });
        assert_eq!((prepared.unwrap(), calls), ("unsigned", 2));
        let mut calls = 0;
        let repeated = retry_stale_observation::<()>(|| {
            calls += 1;
            Err(stale())
        });
        assert_eq!(
            (repeated.unwrap_err().api_code.as_deref(), calls),
            (Some(crate::runtime::STALE_OWNER_OBSERVATION), 2)
        );
        // The message text alone never starts a retry.
        let mut calls = 0;
        let uncoded = retry_stale_observation::<()>(|| {
            calls += 1;
            Err(Error::config("Owner observation is stale"))
        });
        assert!(uncoded.is_err());
        assert_eq!(calls, 1);
    }
}

#[cfg(test)]
mod route_tests {
    use super::check_route;

    #[test]
    fn an_owner_call_reaches_an_api_route_of_the_backend_only() {
        for path in [
            "/api/native/reputation?network=example:testnet",
            "/api/native/reputation?network=example%3Atestnet",
            "/api/native/reputation/votes",
        ] {
            check_route(path).unwrap();
        }
        for path in [
            "",
            "/native/reputation",
            "https://elsewhere.example/api/x",
            "//elsewhere.example/api/x",
            "/api/../auth/verify",
            "/api/./x",
            "/api//x",
            "/api/x y",
            "/api/x#y",
            "/api/\u{7f}",
            "/api/%2e%2e/auth/verify",
            "/api/.%2E/auth/verify",
            "/api/..\\auth/verify",
            "/api/x\\y",
            "/api/",
            "/api/x?next=/auth/verify",
            "/api/x?a=b c",
        ] {
            assert!(check_route(path).is_err(), "{path}");
        }
        assert!(check_route(&format!("/api/{}", "a".repeat(600))).is_err());
    }
}
