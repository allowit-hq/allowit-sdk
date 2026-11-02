//! CLI bindings for the shared Stellar client workflow.
use crate::{
    config::env_value,
    error::{Error, Result},
    vendors::typed::TypedRequest,
};
use allowit_sdk::client::store::Store;
use allowit_sdk_engine::stellar::client::{self as shared, AgentSigner, ExecutionOutcome};
use allowit_stellar_wallet::{self as native, OperationStatus};
pub(crate) use shared::{Bundle, public_description, read_public_bundle};
use std::{path::Path, sync::Arc, time::Duration};
use zeroize::Zeroizing;

struct LocalSigner {
    seed: Zeroizing<[u8; 32]>,
    account: String,
}
impl AgentSigner for LocalSigner {
    fn account(&self) -> &str {
        &self.account
    }
    fn sign_hash(&self, digest: &[u8; 32]) -> native::Result<[u8; 64]> {
        use ed25519_dalek::{Signer, SigningKey};
        Ok(SigningKey::from_bytes(&self.seed).sign(digest).to_bytes())
    }
}
pub(crate) struct Client(shared::Client);
impl Client {
    pub(crate) fn load(bundle: Bundle, signer: &Path, rpc_url: Option<String>) -> Result<Self> {
        let seed = crate::wallet::stellar_signing_seed(signer)?;
        let account = native::account_for_seed(&seed);
        shared::Client::new(
            bundle,
            Arc::new(LocalSigner { seed, account }),
            &env_value("ALLOWIT_URL"),
            rpc_url,
        )
        .map(Self)
    }
}
fn open_journal(path: &Path) -> Result<Store> {
    Store::open(path).map_err(|_| {
        Error::config(
            "Stellar operation journal is unsafe or unreadable; retain the original journal",
        )
    })
}
pub(crate) fn retained_request(
    path: &Path,
    policy: &str,
    id: &str,
) -> Result<Option<allowit_stellar_wallet::workflow::ExecutionRequest>> {
    shared::retained_request(&open_journal(path)?, policy, id)
}
pub(crate) fn status(
    client: &Client,
    path: &Path,
    id: &str,
    wait: Duration,
) -> Result<(serde_json::Value, i32)> {
    shared::status(&client.0, &open_journal(path)?, id, wait).map(render)
}
pub(crate) fn exec(
    client: &Client,
    path: &Path,
    request: TypedRequest,
    wait: Duration,
    stderr: &mut String,
) -> Result<(serde_json::Value, i32)> {
    let store = open_journal(path)?;
    crate::flush_stderr(stderr);
    shared::exec(
        &client.0,
        &store,
        &request.request_id,
        request.request,
        wait,
    )
    .map(render)
}
fn render(outcome: ExecutionOutcome) -> (serde_json::Value, i32) {
    let code = match outcome.status {
        OperationStatus::Confirmed => 0,
        OperationStatus::Failed => 20,
        OperationStatus::SubmissionUnknown => 5,
        OperationStatus::Prepared | OperationStatus::Submitted => 12,
    };
    (
        serde_json::json!({"requestId":outcome.request_id,"status":outcome.status,"transactionHash":outcome.transaction_hash}),
        code,
    )
}
