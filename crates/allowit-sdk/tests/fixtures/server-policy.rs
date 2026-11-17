use allowit::prelude::*;
/// Buy a risk report about my vault, and redeem only when the publisher's report and public sources support it.
pub fn execute(ctx: &AllowitContext) -> allowit::Result {
    allowit::operations(ctx, &ctx.params[allowit::OPERATIONS])?;
    allowit::tokens(ctx, &ctx.params[allowit::TOKENS])?;
    allowit::destinations(ctx, &ctx.params[allowit::DESTINATIONS])?;
    allowit::payment_limit(ctx, &ctx.params[allowit::PAYMENT_LIMIT])?;
    allowit::new_decision_context(ctx, None)?;
    let operation = allowit::operations(ctx, &ctx.params[allowit::OPERATIONS])?;
    let catalog_query: String = allowit::param(ctx, "catalog_query");
    let web_query: String = allowit::param(ctx, "web_query");
    let report_url: String = allowit::param(ctx, "report_url");
    let report_key: String = allowit::param(ctx, "report_key");
    let question: String = allowit::param(ctx, "question");
    match operation {
        "transfer" => {
            allowit::marketplace_search(ctx, &allowit::AllowitMarketplaceSearch {
                query: catalog_query.as_str(),
            })?;
        }
        "redeem" => {
            allowit::fetch(ctx, &allowit::AllowitFetch {
                method: "GET",
                url: report_url.as_str(),
                data: "",
                pubkey: report_key.as_str(),
            })?;
        }
        _ => return Err(AllowitError::OperationNotAllowed),
    }
    allowit::internet_search(ctx, &allowit::AllowitInternetSearch {
        query: web_query.as_str(),
    })?;
    allowit::decision(ctx, &allowit::AllowitDecision {
        id: "review",
        query: question.as_str(),
        deny_below: 3000,
        allow_above: 8000,
    })?;
    Ok(())
}
