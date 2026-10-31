//! Owner operations (fund, withdraw, revoke, close, tune, tune-action) and
//! executor payments (execute) submitted through the native lifecycle.
//! Owner operations sign with the `ALLOWIT_OWNER_KEYPAIR` key file; execute
//! signs with `ALLOWIT_EXECUTOR_KEYPAIR` and needs a request ID. The adapter
//! supplies the owner key from `--sign-with-key` and the request ID from
//! `--request-id`; the hidden verbs of earlier releases read their settings.
use super::*;

pub(super) fn run(session: &Session, name: &str, pos: &[String], out: &mut Out) -> Result<i32> {
    let Session {
        local,
        policy,
        owner,
        signer,
        client,
        audit,
        journal,
        lifecycle,
        options: run_options,
    } = *session;
    let saved_request_id = run_options.request_id.as_deref();
    let request_id = saved_request_id.unwrap_or_default().to_owned();
    let options = match name {
        "deploy" => Options {
            amount: Some(pos[0].clone()),
            ..Options::default()
        },
        "fund" | "withdraw" => Options {
            amount: Some(pos[0].clone()),
            additional_owner_operation: run_options.additional,
            ..Options::default()
        },
        "tune" => Options {
            amount: Some(pos[0].clone()),
            ..Options::default()
        },
        "tune-action" => Options {
            amount: Some(pos[0].clone()),
            ..Options::default()
        },
        "close" => native(lifecycle.close_options(policy, owner, saved_request_id))?,
        _ => Options::default(),
    };
    let method = if name == "tune-action" {
        "tune_action"
    } else {
        name
    };
    let mut result = if name == "execute" {
        if request_id.is_empty() {
            return Err(Error::config(
                "Set one fresh ALLOWIT_REQUEST_ID for this intended payment; keep it unchanged for retries",
            ));
        }
        let reporter = audit.as_ref().ok_or_else(|| {
            Error::config(
                "Import an owner-issued native executor bundle with its scoped approval capability",
            )
        })?;
        // Wallet recipients pay their associated token account, as the engine binds it.
        let requested_recipient = native(Key::parse(&pos[0]))?;
        let action = configured("ALLOWIT_EXECUTION_ACTION", "transfer");
        let merchant = env_value("ALLOWIT_EXECUTION_MERCHANT");
        let context = execution_context()?;
        let identity = native(execution_identity(
            client,
            journal,
            requested_recipient,
            |recipient| {
                ExecutionRequestIdentity::new(
                    policy,
                    &request_id,
                    &pos[1],
                    recipient,
                    &action,
                    &merchant,
                    &context,
                )
            },
        ))?;
        let executor = load_signer("ALLOWIT_EXECUTOR_KEYPAIR")?;
        native(lifecycle.submit_authorized(
            policy,
            owner,
            &identity,
            || reporter.authorize(&identity, requested_recipient, &context),
            |transaction| {
                if transaction.payer != executor.public_key() {
                    return Err(allowit_native::error::Error::config(
                        "Configured executor does not match the prepared fee payer",
                    ));
                }
                Ok(executor.sign(&transaction.message))
            },
        ))?
    } else {
        native(lifecycle.submit(
            policy,
            owner,
            method,
            &options,
            saved_request_id,
            |tx, role| {
                if role != "owner" {
                    return Err(allowit_native::error::Error::config(
                        "Owner operation requested an unexpected signer",
                    ));
                }
                let signer = signer.as_ref().ok_or_else(|| {
                    allowit_native::error::Error::config("Owner signing is unavailable")
                })?;
                if tx.payer != signer.public_key() {
                    return Err(allowit_native::error::Error::config(
                        "Configured signer does not match the prepared fee payer",
                    ));
                }
                Ok(signer.sign(&tx.message))
            },
        ))?
    };
    let replayed = result.extra.get("replayed") == Some(&serde_json::json!(true));
    let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !result.final_status()
        && !result.expired()
        && std::time::Instant::now() < until
        && !audit
            .as_ref()
            .is_some_and(|a| a.blocked.load(std::sync::atomic::Ordering::Relaxed))
    {
        std::thread::sleep(std::time::Duration::from_secs(1));
        match lifecycle.recover(&result.id, policy, owner) {
            Ok(observed) => result = observed,
            Err(_) => {
                // The signed proof is already durable and may have landed.
                // A failed observation cannot turn it into a config-only exit.
                let name = format!("request-{}", result.id);
                match journal.locked(|| {
                    let mut current = journal.read::<Record>(&name)?.ok_or_else(|| {
                        allowit_native::error::Error::config("Operation journal disappeared")
                    })?;
                    if current.signature != result.signature
                        || current.signed_bytes != result.signed_bytes
                        || current.intent != result.intent
                    {
                        return Err(allowit_native::error::Error::config(
                            "Operation journal changed",
                        ));
                    }
                    if !current.final_status() && !current.expired() {
                        current.status = "uncertain".into();
                        current.extra.remove("replayed");
                        current.extra.insert(
                            "error".into(),
                            serde_json::json!("recovery observation failed"),
                        );
                        journal.write(&name, &current)?;
                    }
                    Ok(current)
                }) {
                    Ok(current) => result = current,
                    Err(_) => {
                        result.status = "uncertain".into();
                        if run_options.legacy {
                            out.progress("Recovery observation could not be saved; retain the original journal and request ID.");
                        }
                    }
                }
                break;
            }
        }
    }
    if replayed {
        result
            .extra
            .insert("replayed".into(), serde_json::json!(true));
    }
    let mut output = result.public();
    let (broadcast_withheld, audit_pending) = if name == "execute" {
        audit
            .as_ref()
            .map(|a| a.finish(journal, &result.id))
            .unwrap_or((false, false))
    } else {
        (false, false)
    };
    if audit.is_some() && name == "execute" {
        if broadcast_withheld {
            output["broadcastWithheld"] = serde_json::json!(true);
        }
        output["auditStatus"] = serde_json::json!(if audit_pending {
            "pending"
        } else {
            "confirmed"
        });
    }
    if result.status == "settled" && name == "deploy" {
        let state = native(client.state(policy, owner, true, None))?
            .ok_or_else(|| Error::config("Deployed vault is unavailable"))?;
        let skill = native(client.skill(policy, &state))?;
        save_text(&local.directory.join("SKILL.md"), &skill)?;
        output["skill"] = serde_json::json!(skill);
    }
    // The flags of the next step after an unfinished vault operation, for
    // the current commands; the hidden verbs keep their recovery hints.
    let retry = retry_flags(
        &result.id,
        result.expired(),
        result.extra.get("decisionCode") == Some(&serde_json::json!("EXPIRED_UNEXECUTED")),
        audit_pending,
    )
    .filter(|_| !run_options.legacy);
    if let Some(flags) = &retry {
        output["retry"] = serde_json::json!(flags);
    }
    if out.json {
        out.json_value(&output);
    } else {
        let mut view = serde_json::json!({
            "policyId": policy.id,
            "operation": name,
            "status": result.status,
            "requestId": result.id,
            "transactionUrl": result.transaction_url,
        });
        for key in [
            "replayed",
            "broadcastWithheld",
            "auditStatus",
            "decisionCode",
            "blockhashExpired",
            "error",
            "skill",
            "retry",
        ] {
            if let Some(value) = output.get(key) {
                view[key] = value.clone();
            }
        }
        out.value(&view);
    }
    // The hidden verbs of earlier releases keep their recovery hints; the
    // `retry` field carries the same step for the current commands.
    if run_options.legacy {
        if result.expired() {
            out.progress("The original operation remains uncertain and cannot be broadcast again. For an explicitly additional fund/withdraw, set both a fresh ALLOWIT_REQUEST_ID and ALLOWIT_ADDITIONAL_OWNER_OPERATION=1; retain the old proof.");
        }
        if result.extra.get("decisionCode") == Some(&serde_json::json!("EXPIRED_UNEXECUTED")) {
            out.progress("This request expired without execution. For a deliberate retry, set a new ALLOWIT_REQUEST_ID; keep the original journal.");
        }
        if audit_pending && broadcast_withheld {
            out.progress("Broadcast was withheld. Retry the identical execute command with this request ID to report and broadcast the saved proof; policy status only observes and reports.");
        }
        if audit_pending {
            out.progress("Native SQL audit is pending. Retain the original journal and request ID; retry the identical command or run allowit wallet show.");
        }
    }
    if audit_pending {
        return Ok(5);
    }
    Ok(if result.status == "failed" {
        20
    } else if result.status != "settled" {
        5
    } else if replayed {
        6
    } else {
        0
    })
}

/// The flags that repeat or replace an unfinished vault operation in the
/// current commands: a new request ID (and `--additional` for a second
/// operation after an uncertain one), or the same request ID.
fn retry_flags(id: &str, expired: bool, unexecuted: bool, audit_pending: bool) -> Option<String> {
    if expired {
        Some("--request-id NEW_ID --additional".into())
    } else if unexecuted {
        Some("--request-id NEW_ID".into())
    } else if audit_pending {
        Some(format!("--request-id {id}"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn retry_flags_name_the_current_flags() {
        assert_eq!(
            super::retry_flags("fund-0001", true, false, false).unwrap(),
            "--request-id NEW_ID --additional"
        );
        assert_eq!(
            super::retry_flags("fund-0001", false, true, false).unwrap(),
            "--request-id NEW_ID"
        );
        assert_eq!(
            super::retry_flags("fund-0001", false, false, true).unwrap(),
            "--request-id fund-0001"
        );
        assert!(super::retry_flags("fund-0001", false, false, false).is_none());
    }
}
