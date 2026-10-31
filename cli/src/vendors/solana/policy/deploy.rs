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
    json: bool,
    stdout: &mut String,
) -> Result<i32> {
    let owner = public_setting("ALLOWIT_OWNER", local.context.as_ref().map(|c| c.owner))?
        .ok_or_else(|| Error::config("Set ALLOWIT_OWNER to the owner public key; browser deployment never loads an owner key"))?;
    let request_id = options.request_id.clone().unwrap_or_default();
    if !crate::request::request_id_valid(&request_id) {
        return Err(Error::usage(
            "the deployment intent needs a valid request ID",
        ));
    }
    let retry_preparation = env_value("ALLOWIT_RETRY_PREPARATION_OF");
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
    let journal = FileJournal::new(local.directory.join("journal"));
    let intent =
        serde_json::json!({"requestId":request_id,"signingUrl":url,"intentDigest":fingerprint});
    native(journal.locked(|| {
        let prior: Option<serde_json::Value> =
            journal.read(&format!("browser-deploy-{request_id}"))?;
        if prior.as_ref().is_some_and(|v| v != &intent) {
            return Err(allowit_native::error::Error::config(
                "Deployment request ID already binds another intent",
            ));
        }
        journal.write(&format!("browser-deploy-{request_id}"), &intent)
    }))?;
    if json {
        *stdout += &crate::output::json(
            &serde_json::json!({"state":"awaiting_owner_signature","executed":false,"requestId":request_id,"signingUrl":url,"intentDigest":fingerprint}),
        );
    } else {
        *stdout += &format!(
            "Intent: {}\nOpen this link to review and sign the deployment in your wallet:\n{url}\n",
            &fingerprint[..16]
        );
    }
    Ok(10)
}
