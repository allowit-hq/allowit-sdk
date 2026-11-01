//! The vault creation intent behind `allowit wallet create` on Solana: records a
//! browser owner-signing intent. No owner key loads. The adapter supplies
//! `ALLOWIT_OWNER` and the request ID; this prints the signing link and exits
//! 10 until the owner signs.
//!
//! ```text
//! allowit --rail solana:devnet policy new "Spend up to 5 test tokens per day" -o policy.json
//! allowit --rail solana:devnet wallet create --asset USDC --policy policy.json --owner OWNER --agent agent --fund USDC:10
//! # executor device, fresh directory (import refuses an existing policy)
//! allowit --rail solana:devnet keys import executor executor.json
//! ```
use super::*;

pub(super) fn run(
    local: &Local,
    policy: &Policy,
    pos: &[String],
    options: &RunOptions,
    out: &mut Out,
) -> Result<i32> {
    let owner = public_setting("ALLOWIT_OWNER", local.context.as_ref().map(|c| c.owner))?
        .ok_or_else(|| Error::config("Set ALLOWIT_OWNER to the owner public key; browser deployment never loads an owner key"))?;
    let request_id = options.request_id.clone().unwrap_or_default();
    if !crate::request::request_id_valid(&request_id) {
        return Err(Error::usage(
            "the deployment intent needs a valid request ID",
        ));
    }
    let journal = FileJournal::new(local.directory.join("journal"));
    let entry = format!("browser-deploy-{request_id}");
    // The latest preparation of a creation: the one a new request ID replaces.
    let latest = options
        .creation
        .as_ref()
        .map(|c| format!("browser-deploy-latest-{c}"));
    let retry_preparation = match (&options.creation, &latest) {
        (Some(creation), Some(latest)) => {
            let recorded: Option<serde_json::Value> = native(journal.read(&entry))?;
            match recorded.as_ref().and_then(|v| v["signingUrl"].as_str()) {
                // A repeat keeps the predecessor that its intent binds.
                Some(url) => crate::vendors::solana::handoff::retry_preparation_from_url(url)?
                    .unwrap_or_default(),
                // A new ID replaces the latest preparation in this journal.
                None => {
                    let pointer: Option<String> = native(journal.read(latest))?;
                    let prior = match pointer {
                        Some(id) => Some(id),
                        None => native(
                            journal
                                .read::<serde_json::Value>(&format!("browser-deploy-{creation}")),
                        )?
                        .map(|_| creation.clone()),
                    };
                    prior.filter(|p| *p != request_id).unwrap_or_default()
                }
            }
        }
        _ if options.legacy => env_value("ALLOWIT_RETRY_PREPARATION_OF"),
        _ => String::new(),
    };
    let url = crate::vendors::solana::handoff::signing_url_with_retry(
        &env_value("ALLOWIT_URL"),
        &request_id,
        owner,
        &pos[0],
        policy,
        &local.config,
        if retry_preparation.is_empty() {
            None
        } else {
            Some(retry_preparation.as_str())
        },
    )?;
    let fingerprint = crate::vendors::solana::handoff::intent_digest_from_url(&url)?;
    let intent =
        serde_json::json!({"requestId":request_id,"signingUrl":url,"intentDigest":fingerprint});
    native(journal.locked(|| {
        let prior: Option<serde_json::Value> = journal.read(&entry)?;
        match prior {
            Some(v) if v != intent => Err(allowit_native::error::Error::config(
                "Deployment request ID already binds another intent",
            )),
            Some(_) => Ok(()),
            None => {
                journal.write(&entry, &intent)?;
                // Only a new preparation becomes the one to replace next.
                match &latest {
                    Some(latest) => journal.write(latest, &request_id),
                    None => Ok(()),
                }
            }
        }
    }))?;
    out.value(&serde_json::json!({
        "state": "awaiting_owner_signature",
        "executed": false,
        "requestId": request_id,
        "signingUrl": url,
        "intentDigest": fingerprint,
    }));
    Ok(10)
}
