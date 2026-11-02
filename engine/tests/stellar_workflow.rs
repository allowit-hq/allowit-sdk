//! Persistence and recovery contracts of the shared Stellar workflow.
use allowit_sdk_engine::stellar::{OperationStore, Runtime};
use allowit_stellar_wallet::{
    self as native, OperationStatus, PreparedOperation, SignedOperation, rpc,
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use stellar_xdr::{
    Limits, TransactionResult, TransactionResultExt, TransactionResultResult, WriteXdr,
};

type Events = Arc<Mutex<Vec<String>>>;
struct ScriptedRpc {
    events: Events,
    responses: Mutex<VecDeque<native::Result<Value>>>,
}
impl rpc::Rpc for ScriptedRpc {
    fn call(&self, method: &str, _: Value) -> native::Result<Value> {
        self.events.lock().unwrap().push(format!("rpc:{method}"));
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected RPC")
    }
}
fn runtime(events: &Events, responses: Vec<native::Result<Value>>) -> Runtime {
    Runtime::from_rpc(Arc::new(ScriptedRpc {
        events: events.clone(),
        responses: Mutex::new(responses.into()),
    }))
}
struct Store {
    events: Events,
    value: Option<(SignedOperation, OperationStatus)>,
    fail: bool,
}
impl OperationStore for Store {
    fn load(&self, _: [u8; 32]) -> native::Result<Option<(SignedOperation, OperationStatus)>> {
        Ok(self.value.clone())
    }
    fn record(
        &mut self,
        _: [u8; 32],
        signed: &SignedOperation,
        status: OperationStatus,
    ) -> native::Result<()> {
        self.events
            .lock()
            .unwrap()
            .push(format!("retain:{status:?}"));
        if self.fail {
            return Err("durable retention failed".into());
        }
        if self.value.as_ref().is_some_and(|(old, _)| old != signed) {
            return Err("rebound original packet".into());
        }
        self.value = Some((signed.clone(), status));
        Ok(())
    }
}
fn native_signed() -> SignedOperation {
    let old: SignedOperation = serde_json::from_str(include_str!(
        "../../cli/tests/fixtures/canonical-main-9682-native-signed.json"
    ))
    .unwrap();
    let mut config = old.prepared.config;
    config.language = native::PolicyLanguage::Portable;
    config.policy_source = "use allowit::prelude::*; pub fn execute(ctx: &AllowitContext) -> allowit::Result { Ok(()) }".into();
    config.source_hash = native::hash(&config.policy_source);
    let mut state = old.prepared.state;
    state.source_hash = config.source_hash;
    let prepared = native::prepare(&config, &old.prepared.request, &state, 86401, 86700).unwrap();
    native::authorize(
        &prepared,
        &[
            native::sign(&prepared, &[2; 32], 86401).unwrap(),
            native::sign(&prepared, &[3; 32], 86401).unwrap(),
        ],
        86401,
    )
    .unwrap()
}
fn policy_signed() -> SignedOperation {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../cli/tests/fixtures/stellar-policy-wallet-revalidation-rpc.json"
    ))
    .unwrap();
    serde_json::from_value(fixture["signed"].clone()).unwrap()
}
fn tx_status(signed: &SignedOperation, success: bool) -> Value {
    let result = TransactionResult {
        fee_charged: 100,
        result: if success {
            TransactionResultResult::TxSuccess(Default::default())
        } else {
            TransactionResultResult::TxFailed(Default::default())
        },
        ext: TransactionResultExt::V0,
    };
    json!({"status":if success{"SUCCESS"}else{"FAILED"},"ledger":777,"envelopeXdr":signed.transaction_xdr,"resultXdr":result.to_xdr_base64(Limits::none()).unwrap()})
}

#[test]
fn retention_failure_prevents_broadcast_and_ambiguous_retry_keeps_exact_bytes() {
    let signed = native_signed();
    let events = Events::default();
    let runtime = runtime(
        &events,
        vec![
            Err("lost response".into()),
            Ok(json!({"status":"NOT_FOUND"})),
            Ok(json!({"hash":native::hex(&signed.transaction_hash),"status":"PENDING"})),
            Ok(json!({"status":"NOT_FOUND"})),
        ],
    );
    let mut store = Store {
        events: events.clone(),
        value: None,
        fail: true,
    };
    assert!(runtime.submit(&mut store, &signed, 86401).is_err());
    assert_eq!(*events.lock().unwrap(), vec!["retain:SubmissionUnknown"]);
    store.fail = false;
    events.lock().unwrap().clear();
    assert_eq!(
        runtime.submit(&mut store, &signed, 86401).unwrap(),
        OperationStatus::SubmissionUnknown
    );
    assert_eq!(
        *events.lock().unwrap(),
        vec!["retain:SubmissionUnknown", "rpc:sendTransaction"]
    );
    assert_eq!(
        runtime.submit(&mut store, &signed, 86500).unwrap(),
        OperationStatus::Submitted
    );
    assert_eq!(
        runtime.submit(&mut store, &signed, 86700).unwrap(),
        OperationStatus::SubmissionUnknown
    );
    assert_eq!(store.value.as_ref().unwrap().0, signed);
    let calls = events.lock().unwrap();
    assert_eq!(
        calls.iter().filter(|v| *v == "rpc:sendTransaction").count(),
        2
    );
    assert!(calls.windows(3).any(|v| v
        == [
            "rpc:getTransaction",
            "retain:SubmissionUnknown",
            "rpc:sendTransaction"
        ]));
}

#[test]
fn original_identity_and_terminal_state_survive_retries_and_expiry() {
    let signed = native_signed();
    let events = Events::default();
    let runtime = runtime(&events, vec![Ok(tx_status(&signed, true))]);
    let mut store = Store {
        events: events.clone(),
        value: Some((signed.clone(), OperationStatus::SubmissionUnknown)),
        fail: false,
    };
    assert_eq!(
        runtime
            .recover(&mut store, signed.prepared.request.operation.nonce)
            .unwrap(),
        OperationStatus::Confirmed
    );
    let mut changed = signed.clone();
    changed.transaction_hash[0] ^= 1;
    assert!(runtime.submit(&mut store, &changed, 90000).is_err());
    assert_eq!(
        runtime.submit(&mut store, &signed, 90000).unwrap(),
        OperationStatus::Confirmed
    );
    assert_eq!(
        runtime
            .recover(&mut store, signed.prepared.request.operation.nonce)
            .unwrap(),
        OperationStatus::Confirmed
    );
    assert_eq!(
        *events.lock().unwrap(),
        vec!["rpc:getTransaction", "retain:Confirmed"]
    );
    assert_eq!(store.value.as_ref().unwrap().0, signed);
}

#[test]
fn failed_policy_envelope_never_proves_the_address_authorization_cannot_execute() {
    let signed = policy_signed();
    let events = Events::default();
    let runtime = runtime(
        &events,
        vec![Ok(tx_status(&signed, false)), Ok(tx_status(&signed, false))],
    );
    let mut store = Store {
        events: events.clone(),
        value: Some((signed.clone(), OperationStatus::Submitted)),
        fail: false,
    };
    assert_eq!(
        runtime
            .recover(&mut store, signed.prepared.request.operation.nonce)
            .unwrap(),
        OperationStatus::SubmissionUnknown
    );
    assert_eq!(
        store.value.as_ref().unwrap().1,
        OperationStatus::SubmissionUnknown
    );
    let observed = runtime.observe_authorizations([&signed.prepared]);
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].receipt.status, OperationStatus::Failed);
    assert_eq!(observed[0].status(), OperationStatus::SubmissionUnknown);
    assert_eq!(store.value.as_ref().unwrap().0, signed);
}

#[test]
fn authorization_retention_precedes_second_rpc_and_simulation_failure_keeps_it() {
    let prepared = policy_signed().prepared;
    let events = Events::default();
    let runtime = runtime(
        &events,
        vec![Err("post-sign simulation unavailable".into())],
    );
    let retained = Mutex::new(None::<PreparedOperation>);
    let result = runtime.finalize_authorization(
        &prepared,
        |p| {
            events.lock().unwrap().push("authorize".into());
            Ok(p.clone())
        },
        |p| {
            events.lock().unwrap().push("retain".into());
            *retained.lock().unwrap() = Some(p.clone());
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(
        *events.lock().unwrap(),
        vec!["authorize", "retain", "rpc:getNetwork"]
    );
    let original = retained.lock().unwrap().take().unwrap();
    assert_eq!(original, prepared);
    native::validate_retained_prepared(&original).unwrap();
    events.lock().unwrap().clear();
    assert!(
        runtime
            .finalize_authorization(
                &prepared,
                |p| Ok(p.clone()),
                |_| Err("retention failed".into())
            )
            .is_err()
    );
    assert!(events.lock().unwrap().is_empty());
}
