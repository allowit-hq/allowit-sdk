//! Shared native execution orchestration. Original journal names and signed records remain unchanged.
use crate::{
    error::{Error, Result},
    execution::{Outcome, Request},
    profiles::Profile,
    vendors::ProfileBinding,
    vendors::stellar::runtime::client::{self, ExecutionOutcome},
    wallet::LocalSigner,
};
use allowit_sdk::client::store::Store;
use allowit_stellar_wallet::OperationStatus;
use std::{path::Path, sync::Arc, time::Duration};
fn journal(root: &Path) -> Result<Store> {
    Store::open(&root.join("stellar-journal"))
}
fn typed(
    profile: &Profile,
    root: &Path,
    rpc: Option<&str>,
    r: &Request,
) -> Result<super::typed::TypedRequest> {
    let ProfileBinding::Stellar(bundle) = &profile.binding;
    let retained = if r.build_only {
        None
    } else {
        client::retained_request(&journal(root)?, &bundle.policy_id, &r.request_id)?
    };
    if let Some(old) = retained {
        super::request::rebuild(&bundle.policy_id, &bundle.config, r, &old)
    } else {
        super::request::build(&bundle.policy_id, &bundle.config, r, || {
            super::request::state(&bundle.config, rpc)
        })
    }
}
pub(in crate::vendors) fn build(
    profile: &Profile,
    root: &Path,
    rpc: Option<&str>,
    r: &Request,
) -> Result<serde_json::Value> {
    typed(profile, root, rpc, r)?.wire()
}
fn client(profile: Profile, signer: LocalSigner, rpc: Option<String>) -> Result<client::Client> {
    if profile.description().network != signer.network() {
        return Err(Error::config(
            "Agent signer belongs to another profile rail",
        ));
    }
    let authority = profile.require_authority()?.clone();
    let ProfileBinding::Stellar(bundle) = profile.binding;
    client::Client::new(bundle, Arc::new(signer), &authority, rpc)
}
pub(in crate::vendors) fn execute(
    profile: Profile,
    signer: LocalSigner,
    root: &Path,
    rpc: Option<String>,
    r: &Request,
) -> Result<Outcome> {
    let typed = typed(&profile, root, rpc.as_deref(), r)?;
    client::exec(
        &client(profile, signer, rpc)?,
        &journal(root)?,
        &r.request_id,
        typed.request,
        r.wait,
    )
    .map(render)
}
pub(in crate::vendors) fn retained(
    root: &Path,
    network: &str,
    wallet: Option<&str>,
    id: &str,
) -> Result<Option<crate::execution::Recovery>> {
    client::retained(&journal(root)?, network, wallet, id).map(|r| {
        r.map(|r| crate::execution::Recovery {
            binding: crate::vendors::RecoveryBinding::Stellar(r),
        })
    })
}
pub(in crate::vendors) fn recover(
    original: client::Retained,
    signer: LocalSigner,
    backend: &str,
    root: &Path,
    id: &str,
    wait: Duration,
    request: Option<&Request>,
) -> Result<Outcome> {
    if let Some(request) = request {
        if !original.signed() {
            return Err(Error::config(
                "Unsigned history requires current verified execution authority",
            ));
        }
        let rebuilt = super::request::rebuild(
            original.policy_id(),
            original.config(),
            request,
            original.request(),
        )?;
        if rebuilt.request != *original.request() {
            return Err(Error::usage(
                "Original request ID already binds different input",
            ));
        }
    }
    client::recover(
        original,
        &journal(root)?,
        Arc::new(signer),
        backend,
        id,
        wait,
    )
    .map(render)
}
pub(in crate::vendors) fn status(
    profile: Profile,
    signer: LocalSigner,
    root: &Path,
    rpc: Option<String>,
    id: &str,
    wait: Duration,
) -> Result<Outcome> {
    client::status(&client(profile, signer, rpc)?, &journal(root)?, id, wait).map(render)
}

fn render(out: ExecutionOutcome) -> Outcome {
    let status = match out.status {
        OperationStatus::Confirmed => crate::runtime::Status::Confirmed,
        OperationStatus::Failed => crate::runtime::Status::Failed,
        OperationStatus::SubmissionUnknown => crate::runtime::Status::SubmissionUnknown,
        OperationStatus::Prepared => crate::runtime::Status::Prepared,
        OperationStatus::Submitted => crate::runtime::Status::Submitted,
    };
    Outcome {
        document: serde_json::json!({"requestId":out.request_id,"status":out.status,"transactionHash":out.transaction_hash}),
        status,
    }
}
