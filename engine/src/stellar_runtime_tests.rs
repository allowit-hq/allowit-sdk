//! Persistence and recovery contracts of the shared Stellar workflow.
use crate::error::{Error, Result};
use crate::runtime::{OperationStore, Runtime, Signed, Status};
use crate::vendors::stellar::runtime::Runtime as NativeRuntime;
use allowit_stellar_wallet::{self as native, PreparedOperation, SignedOperation, rpc};
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
    struct Transport(ScriptedRpc);
    impl crate::runtime::Transport for Transport {
        fn call(&self, method: &str, params: Value) -> Result<Value> {
            rpc::Rpc::call(&self.0, method, params).map_err(Error::config)
        }
    }
    Runtime::with_transport(
        "stellar:testnet",
        Arc::new(Transport(ScriptedRpc {
            events: events.clone(),
            responses: Mutex::new(responses.into()),
        })),
    )
    .unwrap()
}
fn packet(s: &SignedOperation) -> Signed {
    serde_json::from_value(serde_json::to_value(s).unwrap()).unwrap()
}
struct Store {
    events: Events,
    value: Option<(Signed, Status)>,
    fail: bool,
}
impl OperationStore for Store {
    fn load(&self, _: [u8; 32]) -> Result<Option<(Signed, Status)>> {
        Ok(self.value.clone())
    }
    fn record(&mut self, _: [u8; 32], signed: &Signed, status: Status) -> Result<()> {
        self.events
            .lock()
            .unwrap()
            .push(format!("retain:{status:?}"));
        if self.fail {
            return Err(Error::config("durable retention failed"));
        }
        if self
            .value
            .as_ref()
            .is_some_and(|(old, _)| old.bytes() != signed.bytes())
        {
            return Err(Error::config("rebound original packet"));
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
    let native = native_signed();
    let signed = packet(&native);
    let events = Events::default();
    let runtime = runtime(
        &events,
        vec![
            Err("lost response".into()),
            Ok(json!({"status":"NOT_FOUND"})),
            Ok(json!({"hash":native::hex(&native.transaction_hash),"status":"PENDING"})),
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
        Status::SubmissionUnknown
    );
    assert_eq!(
        *events.lock().unwrap(),
        vec!["retain:SubmissionUnknown", "rpc:sendTransaction"]
    );
    assert_eq!(
        runtime.submit(&mut store, &signed, 86500).unwrap(),
        Status::Submitted
    );
    assert_eq!(
        runtime.submit(&mut store, &signed, 86700).unwrap(),
        Status::SubmissionUnknown
    );
    assert_eq!(store.value.as_ref().unwrap().0.bytes(), signed.bytes());
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
    let native = native_signed();
    let signed = packet(&native);
    let events = Events::default();
    let runtime = runtime(&events, vec![Ok(tx_status(&native, true)); 3]);
    let mut store = Store {
        events: events.clone(),
        value: Some((signed.clone(), Status::SubmissionUnknown)),
        fail: false,
    };
    assert_eq!(
        runtime
            .recover(&mut store, native.prepared.request.operation.nonce)
            .unwrap(),
        Status::Confirmed
    );
    let mut changed_native = native.clone();
    changed_native.transaction_hash[0] ^= 1;
    let changed = Signed(
        crate::runtime::Packet::new("stellar:testnet", &changed_native, signed.0.provider())
            .unwrap(),
    );
    assert!(runtime.submit(&mut store, &changed, 90000).is_err());
    assert_eq!(
        runtime.submit(&mut store, &signed, 90000).unwrap(),
        Status::Confirmed
    );
    assert_eq!(
        runtime
            .recover(&mut store, native.prepared.request.operation.nonce)
            .unwrap(),
        Status::Confirmed
    );
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "rpc:getTransaction",
            "retain:Confirmed",
            "rpc:getTransaction",
            "retain:Confirmed",
            "rpc:getTransaction",
            "retain:Confirmed"
        ]
    );
    assert_eq!(store.value.as_ref().unwrap().0.bytes(), signed.bytes());
}

#[test]
fn failed_policy_envelope_never_proves_the_address_authorization_cannot_execute() {
    let native = policy_signed();
    let signed = packet(&native);
    let events = Events::default();
    let runtime = runtime(
        &events,
        vec![Ok(tx_status(&native, false)), Ok(tx_status(&native, false))],
    );
    let mut store = Store {
        events: events.clone(),
        value: Some((signed.clone(), Status::Submitted)),
        fail: false,
    };
    assert_eq!(
        runtime
            .recover(&mut store, native.prepared.request.operation.nonce)
            .unwrap(),
        Status::SubmissionUnknown
    );
    assert_eq!(store.value.as_ref().unwrap().1, Status::SubmissionUnknown);
    let prepared = serde_json::from_value(serde_json::to_value(&native.prepared).unwrap()).unwrap();
    let observed = runtime.observe_authorizations(&[prepared]).unwrap();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].receipt.info().unwrap().status, Status::Failed);
    assert_eq!(observed[0].status, Status::SubmissionUnknown);
    assert_eq!(store.value.as_ref().unwrap().0.bytes(), signed.bytes());
}

#[test]
fn authorization_retention_precedes_second_rpc_and_simulation_failure_keeps_it() {
    let prepared = policy_signed().prepared;
    let events = Events::default();
    let runtime = NativeRuntime::from_rpc(Arc::new(ScriptedRpc {
        events: events.clone(),
        responses: Mutex::new(vec![Err("post-sign simulation unavailable".into())].into()),
    }));
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
