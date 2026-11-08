//! The Stellar rail package that this SDK builds must read the current Stellar
//! RPC reply for an unknown transaction as "no receipt yet". Owner workflows
//! check for a receipt before they submit, so a rejection stops every deploy.
use allowit_stellar_wallet::rpc::{Rpc, receipt_by_hash};
use serde_json::{Value, json};

const NETWORK: &str = "stellar:testnet";
const HASH: [u8; 32] = [0x3a; 32];

struct Reply(Value);
impl Rpc for Reply {
    fn call(&self, method: &str, params: Value) -> allowit_stellar_wallet::Result<Value> {
        assert_eq!(method, "getTransaction");
        assert_eq!(params["hash"], "3a".repeat(32));
        Ok(self.0.clone())
    }
}

/// Stellar RPC 30 reports `"ledger": 0` for a transaction it has not seen.
fn not_found(ledger: Value) -> Reply {
    Reply(json!({
        "latestLedger": 5626700,
        "latestLedgerCloseTime": "1794163000",
        "oldestLedger": 5505741,
        "oldestLedgerCloseTime": "1793558200",
        "status": "NOT_FOUND",
        "txHash": "3a".repeat(32),
        "applicationOrder": 0,
        "feeBump": false,
        "events": {},
        "ledger": ledger,
        "createdAt": "0"
    }))
}

#[test]
fn current_rpc_not_found_reply_is_no_receipt() {
    assert_eq!(
        receipt_by_hash(&not_found(json!(0)), NETWORK, &HASH).unwrap(),
        None
    );
}

#[test]
fn a_not_found_reply_with_a_malformed_ledger_is_still_rejected() {
    assert!(receipt_by_hash(&not_found(json!("0")), NETWORK, &HASH).is_err());
    assert!(receipt_by_hash(&not_found(json!(1)), NETWORK, &HASH).is_err());
}
