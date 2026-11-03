//! Native error classification. Event hints never establish contract authority.
use crate::runtime::Problem;
pub use allowit_stellar_wallet::rpc::{ContractFault, simulation_fault};
use serde_json::json;
fn fail(message: impl Into<String>) -> Problem {
    Problem {
        status: 409,
        code: "",
        message: message.into(),
        details: None,
    }
}
fn coded(status: u16, code: &'static str, message: impl Into<String>) -> Problem {
    Problem {
        status,
        code,
        message: message.into(),
        details: None,
    }
}
trait Details {
    fn with_details(self, value: serde_json::Value) -> Self;
}
impl Details for Problem {
    fn with_details(mut self, value: serde_json::Value) -> Self {
        self.details = Some(value);
        self
    }
}
/// `AllowitError` names, their codes and the canonical rule each one reports.
const RULE_ERRORS: &[(&str, u32, Option<&str>)] = &[
    ("MissingParam", 101, None),
    ("PaymentLimitExceeded", 102, Some("payment_limit")),
    (
        "LifetimeSpendingLimitExceeded",
        103,
        Some("lifetime_spending_limit"),
    ),
    (
        "DailySpendingLimitExceeded",
        104,
        Some("daily_spending_limit"),
    ),
    ("DailyVolumeLimitExceeded", 105, Some("daily_volume_limit")),
    ("CallLimitExceeded", 106, Some("call_limit")),
    ("DestinationNotAllowed", 107, Some("destinations")),
    ("TokenNotAllowed", 108, Some("tokens")),
    ("Expired", 109, Some("expires_at")),
    ("Unpriced", 110, None),
    ("InvalidParam", 111, None),
    ("InvalidState", 112, None),
    ("Overflow", 113, None),
    ("Unsupported", 114, None),
    ("OperationNotAllowed", 115, Some("operations")),
    ("Panicked", 116, None),
];
/// `AllowitWalletError` codes below 100. Rule denials collapse to `PolicyDenied` onchain.
const WALLET_ERRORS: &[(u32, &str)] = &[
    (1, "InvalidState"),
    (2, "InvalidParam"),
    (3, "UnsupportedAsset"),
    (4, "InvalidActor"),
    (5, "StaleRequest"),
    (6, "Paused"),
    (7, "Expired"),
    (8, "InvalidAmount"),
    (9, "InvalidRecipient"),
    (11, "Overflow"),
    (14, "InsufficientFunds"),
    (15, "InvalidEffects"),
    (16, "UnsupportedOperation"),
    (17, "InvalidInvocation"),
    (18, "PolicyDenied"),
    (19, "Replay"),
];
/// The rail core's structured host evaluation denials. Each is followed by an `AllowitError`
/// name, which is the only text that names a rule.
const RULE_MARKERS: &[&str] = &[
    "Policy prefix denied: ",
    "Authored policy denied this exact operation: ",
];
/// The client-visible codes of operational wallet contract errors.
fn wallet_error_code(name: &str) -> &'static str {
    match name {
        "Paused" => "wallet_paused",
        "InsufficientFunds" => "insufficient_funds",
        "Replay" | "StaleRequest" => "stale_request",
        _ => "wallet_rejected",
    }
}
fn policy_denied(name: &str, rule: Option<&str>, contract: Option<u32>) -> Problem {
    let text = match (rule, contract) {
        (Some(rule), _) => format!("The policy denied this operation: `{rule}` ({name})."),
        (None, Some(code)) => format!(
            "The policy wallet denied this operation in simulation: {name} (contract error {code})."
        ),
        (None, None) => format!("The policy denied this operation: {name}."),
    };
    coded(409, "policy_denied", text)
        .with_details(json!({"rule":rule,"error":name,"contractError":contract}))
}
/// A host evaluator's `AllowitError`, by its code, as `policy_denied`.
pub fn rule_denial(code: u32) -> Problem {
    match RULE_ERRORS.iter().find(|(_, c, _)| *c == code) {
        Some((name, _, rule)) => policy_denied(name, *rule, None),
        None => fail(format!("The policy evaluator failed with error {code}.")),
    }
}
/// A client-visible fault for a rejected operation. Only a structured rule denial of the rail
/// core's host evaluation, or a rule error code or `PolicyDenied` that the installed `wallet`
/// itself raised in simulation, becomes `policy_denied`. Other wallet contract errors keep their
/// own codes. An error that another contract raised, such as the asset contract or a vault, or
/// whose origin is unknown, is a plain wallet rejection with the raw code, and anything else
/// stays a plain conflict.
pub fn denial(message: &str, contract: Option<&ContractFault>, wallet: &str) -> Problem {
    let named = RULE_MARKERS.iter().find_map(|marker| {
        let start = message.find(marker)? + marker.len();
        let name: String = message[start..]
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect();
        RULE_ERRORS.iter().find(|(n, _, _)| *n == name)
    });
    if let Some((name, _, rule)) = named {
        return policy_denied(name, *rule, contract.map(|c| c.code));
    }
    let Some(fault) = contract else {
        return fail(message.to_string());
    };
    let code = fault.code;
    if fault.contract.as_deref() != Some(wallet) {
        let origin = fault
            .contract
            .as_deref()
            .unwrap_or("an unidentified contract");
        return coded(
            409,
            "wallet_rejected",
            format!("The operation failed in simulation: {origin} raised contract error {code}."),
        )
        .with_details(json!({"contractError":code,"contract":fault.contract}));
    }
    if let Some((name, _, rule)) = RULE_ERRORS.iter().find(|(_, c, _)| *c == code) {
        return policy_denied(name, *rule, Some(code));
    }
    match WALLET_ERRORS.iter().find(|(c, _)| *c == code) {
        Some((_, "PolicyDenied")) => policy_denied("PolicyDenied", None, Some(code)),
        Some((_, name)) => coded(
            409,
            wallet_error_code(name),
            format!(
                "The policy wallet rejected this operation in simulation: {name} (contract error {code})."
            ),
        )
        .with_details(json!({"error":name,"contractError":code})),
        None => fail(format!(
            "The policy wallet rejected this operation in simulation with contract error {code}."
        ))
        .with_details(json!({"contractError":code})),
    }
}
