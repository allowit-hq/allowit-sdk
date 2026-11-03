//! Shared native execution orchestration. Original journal names and signed records remain unchanged.
use crate::{
    error::{Error, Result},
    execution::{Outcome, Request},
    profiles::Profile,
    stellar::client::{self, ExecutionOutcome},
    vendors::ProfileBinding,
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
    let ProfileBinding::Stellar(bundle) = profile.binding;
    client::Client::new(bundle, Arc::new(signer), &profile.backend, rpc)
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
fn historical(
    profile: &Profile,
    journal: &Store,
    root: &Path,
    id: &str,
) -> Result<Option<Profile>> {
    let ProfileBinding::Stellar(bundle) = &profile.binding;
    let Some(old) = client::retained_config(journal, &bundle.policy_id, id)? else {
        return Ok(None);
    };
    if client::matches_retained(&bundle.config, &old) {
        return Ok(None);
    }
    let mut matches = crate::profiles::Cache::open(root)?
        .historical(
            &profile.description().network,
            &bundle.wallet,
            &bundle.policy_id,
        )?
        .into_iter()
        .filter(|p| {
            let ProfileBinding::Stellar(b) = &p.binding;
            p.backend == profile.backend && client::matches_retained(&b.config, &old)
        })
        .collect::<Vec<_>>();
    // Config-equivalent versions differ only in authoring metadata. Their native authority is identical.
    matches.sort_by(|a, b| a.version.cmp(&b.version));
    matches.into_iter().next().map(Some).ok_or_else(||Error::config("The exact historical public profile is missing. Retain the original journal and import its verified export."))
}
pub(in crate::vendors) fn status_binding(
    profile: &Profile,
    root: &Path,
    id: &str,
) -> Result<crate::execution::Binding> {
    let store = journal(root)?;
    let historical = historical(profile, &store, root, id)?;
    let d = &historical.as_ref().unwrap_or(profile).description();
    Ok(crate::execution::Binding {
        wallet: d.wallet.clone(),
        policy: format!("{}@{}", d.policy_id, d.module_digest),
    })
}
pub(in crate::vendors) fn status(
    profile: Profile,
    signer: LocalSigner,
    root: &Path,
    rpc: Option<String>,
    id: &str,
    wait: Duration,
) -> Result<Outcome> {
    let journal = journal(root)?;
    if let Some(historical) = historical(&profile, &journal, root, id)? {
        let ProfileBinding::Stellar(historical) = historical.binding;
        return client::historical_status(
            client(profile, signer, rpc)?,
            historical,
            &journal,
            id,
            wait,
        )
        .map(render);
    }
    client::status(&client(profile, signer, rpc)?, &journal, id, wait).map(render)
}

fn render(out: ExecutionOutcome) -> Outcome {
    let exit_code = match out.status {
        OperationStatus::Confirmed => 0,
        OperationStatus::Failed => 20,
        OperationStatus::SubmissionUnknown => 5,
        OperationStatus::Prepared | OperationStatus::Submitted => 12,
    };
    Outcome {
        document: serde_json::json!({"requestId":out.request_id,"status":out.status,"transactionHash":out.transaction_hash}),
        exit_code,
    }
}
