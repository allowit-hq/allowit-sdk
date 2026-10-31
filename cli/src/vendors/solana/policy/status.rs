//! The vault observation behind `allowit wallet show` on Solana: reads the
//! vault and recovers journaled operations. Reads the chain; needs no signer.
//!
//! ```text
//! allowit --rail solana:devnet wallet show --json
//! ```
use super::*;

pub(super) fn run(session: &Session, out: &mut Out) -> Result<i32> {
    let Session {
        policy,
        owner,
        client,
        audit,
        journal,
        lifecycle,
        options,
        ..
    } = *session;
    let records = native(journal.entries::<Record>())?;
    let mut operations = Vec::new();
    let mut audit_pending = false;
    for record in records {
        operations.push(match lifecycle.recover(&record.id,policy,owner){Ok(r)=>{
            if r.method=="execute" && audit.as_ref().is_some_and(|a|a.report(&r).is_err()) {audit_pending=true;}
            r.public()
        },Err(_)=>{if record.method=="execute" && audit.is_some(){audit_pending=true;} serde_json::json!({"id":record.id,"status":"uncertain","error":"Saved operation could not be verified against this configuration"})}});
    }
    let last = native(journal.read::<serde_json::Value>("last"))?;
    let operation = last
        .as_ref()
        .and_then(|last| operations.iter().find(|r| r["id"] == last["id"]));
    let state = native(client.state(policy, owner, true, None))?;
    let audit_status = audit.as_ref().map(|_| {
        if audit_pending {
            "pending"
        } else {
            "confirmed"
        }
    });
    if out.json {
        out.json_value(&serde_json::json!({
            "policyId": policy.id,
            "network": policy.network,
            "state": state,
            "operations": operations,
            "operation": operation,
            "auditStatus": audit_status,
        }));
    } else {
        let amount = |raw: &str| -> Result<String> {
            Ok(allowit_native::policy::decimal(native(
                allowit_native::native::number(raw),
            )?))
        };
        let mut view = serde_json::json!({"policyId": policy.id, "network": policy.network});
        match &state {
            Some(s) => {
                view["vault"] = serde_json::json!(s.binding.vault.to_string());
                view["authority"] = serde_json::json!(s.binding.authority.to_string());
                view["approved"] = serde_json::json!(s.approved);
                view["balance"] = serde_json::json!(amount(&s.balance)?);
                view["actionLimit"] = serde_json::json!(amount(&s.action_limit)?);
                view["spentToday"] = serde_json::json!(amount(&s.spent)?);
            }
            None => view["deployed"] = serde_json::json!(false),
        }
        if let Some(op) = operation {
            view["lastOperation"] = op["status"].clone();
            view["transactionUrl"] = op["transactionUrl"].clone();
        }
        if let Some(status) = audit_status {
            view["auditStatus"] = serde_json::json!(status);
        }
        out.value(&view);
    }
    if audit_pending && options.legacy {
        out.progress(
            "Native SQL audit is pending; retain this journal and run allowit wallet show again.",
        );
    }
    Ok(if audit_pending { 5 } else { 0 })
}
