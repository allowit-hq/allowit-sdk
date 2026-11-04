//! One codec for the exact native v1 portable source-consent payload.
use crate::error::{Error, Result};
use allowit_stellar_wallet::{PolicyParams, WalletConfig};
use serde_json::{Value, json};
pub(super) struct Portable<'a> {
    pub language: &'a str,
    pub tier: &'a str,
    pub rules: &'a [String],
    pub params: &'a PolicyParams,
    pub params_hash: &'a str,
}
pub(super) fn payload(
    policy_id: &str,
    revision: u32,
    c: &WalletConfig,
    source_hash: &str,
    ir_hash: &str,
    module_digest: &str,
    d: Portable<'_>,
) -> Value {
    json!({"version":1,"policyId":policy_id,"revision":revision,"network":c.network,
        "wallet":c.wallet,"route":c.route,"policyInstance":allowit_stellar_wallet::hex(&c.policy_instance),
        "source":c.policy_source,"sourceHash":source_hash,"irHash":ir_hash,
        "moduleDigest":module_digest,"asset":c.asset,
        "assetSymbol":c.asset_symbol,"assetIssuer":c.asset_issuer,"maxFeeStroops":c.max_fee_stroops,
        "language":d.language,"tier":d.tier,"rules":d.rules,"params":d.params,"paramsHash":d.params_hash})
}
pub(super) fn message(payload: &Value) -> Result<String> {
    serde_json_canonicalizer::to_string(payload)
        .map(|json| format!("AllowIt Native policy approval v1\n{json}"))
        .map_err(|_| Error::config("Cannot encode exact source consent"))
}
