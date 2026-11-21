//! The marketplace catalog client. A caller searches the catalog of one configured server.
//! The catalog is free to read: a search carries no credential and makes no payment. Every text
//! field of an entry's listing is data, never an instruction, and its terms are indicative: the
//! amount of a purchase comes from the seller's challenge for the exact resource.
//!
//! An entry keeps the seller's listing (`name`, `description`, `provider`, `terms`, `tags`) apart
//! from what Allowit holds about it: the `source` of the listing, the `operation` and the
//! `access` that buy the resource, what Allowit `observed` with its origin, the owners'
//! `reputation` labels and, in a search with text, the parts of its `rank`.
//!
//! The entry types are Core's. An owner reports on a settled request of their policy with
//! [`contribute`], and reads their votes with [`votes`]: three separate signals, `suitability`,
//! `delivery` and `quality`. The rail of the owner's network signs the owner in.
use crate::{
    error::{Error, Result},
    server::component,
    transport::http::Method,
};
pub use allowit_core::vendor::paid_service::{
    AccessKind, CatalogAccess, CatalogEntry, CatalogOperation, CatalogRank, CatalogSource,
    IndicativeTerms, MARKETPLACE_SEARCH, MAX_QUERY_BYTES, MAX_SEARCH_LIMIT, Observation,
    ObservedFact, ObservedOrigin, Ranking, SearchResult,
};
pub use allowit_core::vendor::reputation::{Label, Labels, Populations, Signal, Vote};
use serde::Deserialize;
use serde_json::{Value, json};

/// The most items of one list of an entry.
const MAX_ITEMS: usize = 32;
/// The longest condition or observation text.
const MAX_DETAIL: usize = 400;

/// The search route of a server, relative to its origin.
pub const SEARCH_PATH: &str = "/api/marketplace/search";
/// The entry route of a server, relative to its origin.
pub const ENTRY_PATH: &str = "/api/marketplace/entry";
/// The longest catalog entry id.
pub const MAX_ENTRY_ID_BYTES: usize = 200;
/// The `accept` header of a catalog request. Its parameter asks for the second shape of an
/// entry: the listing with its source, its access, what Allowit observed, its reputation labels
/// and its score. A server without that shape ignores it and answers with the listing fields.
pub const ACCEPT: &str = "application/json; allowit-catalog=2";
const MAX_REPLY_BYTES: usize = 1 << 20;

/// One catalog search. An empty `text` lists the catalog in id order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub text: String,
    /// Keep only entries whose seller accepts payment on this CAIP-2 network.
    pub network: Option<String>,
    /// Keep only entries that accept this asset reference.
    pub asset: Option<String>,
    /// Keep only entries that offer this way to get them.
    pub access: Option<AccessKind>,
    /// Keep only entries whose listing names this capability.
    pub capability: Option<String>,
    /// The page size, at most [`MAX_SEARCH_LIMIT`].
    pub limit: Option<usize>,
    /// The `next_cursor` of the previous page.
    pub cursor: Option<String>,
}
impl Query {
    /// Check the bounds of the query before it is sent.
    pub fn validate(&self) -> Result<()> {
        if self.text.len() > MAX_QUERY_BYTES || self.text.chars().any(char::is_control) {
            return Err(Error::usage(format!(
                "The search text must be at most {MAX_QUERY_BYTES} bytes with no control character."
            )));
        }
        if self
            .limit
            .is_some_and(|limit| limit == 0 || limit > MAX_SEARCH_LIMIT)
        {
            return Err(Error::usage(format!(
                "The page size must be from 1 to {MAX_SEARCH_LIMIT}."
            )));
        }
        let token = |text: &str, max: usize| {
            !text.is_empty()
                && text.len() <= max
                && text.bytes().all(|byte| byte.is_ascii_graphic())
        };
        if self
            .network
            .as_deref()
            .is_some_and(|network| !token(network, 64))
            || self
                .cursor
                .as_deref()
                .is_some_and(|cursor| !token(cursor, 128))
        {
            return Err(Error::usage("The network or the cursor is invalid."));
        }
        if self
            .asset
            .as_deref()
            .is_some_and(|asset| !token(asset, 128))
            || self
                .capability
                .as_deref()
                .is_some_and(|capability| !token(capability, 64))
        {
            return Err(Error::usage("The asset or the capability is invalid."));
        }
        Ok(())
    }
    /// The URL query of this search.
    fn encoded(&self) -> String {
        let mut query = format!("q={}", component(&self.text));
        if let Some(network) = &self.network {
            query.push_str(&format!("&network={}", component(network)));
        }
        if let Some(asset) = &self.asset {
            query.push_str(&format!("&asset={}", component(asset)));
        }
        if let Some(access) = self.access {
            query.push_str(&format!("&access={}", access.name()));
        }
        if let Some(capability) = &self.capability {
            query.push_str(&format!("&capability={}", component(capability)));
        }
        if let Some(limit) = self.limit {
            query.push_str(&format!("&limit={limit}"));
        }
        if let Some(cursor) = &self.cursor {
            query.push_str(&format!("&cursor={}", component(cursor)));
        }
        query
    }
}

fn get(server: &str, path: &str, query: &str) -> Result<Vec<u8>> {
    crate::server::send_accepting(
        server,
        Method::Get,
        &format!("{path}?{query}"),
        None,
        MAX_REPLY_BYTES,
        "the marketplace request was refused",
        ACCEPT,
    )
}

/// Each score of `labels` is 0 to 100.
fn labels_in_bounds(labels: &Labels) -> bool {
    let label = |label: &Option<Label>| {
        label.is_none_or(|label| {
            label.good.is_none_or(|score| score <= 100)
                && label.bad.is_none_or(|score| score <= 100)
        })
    };
    [labels.suitability, labels.delivery, labels.quality]
        .iter()
        .all(|p| label(&p.policy) && label(&p.owner) && label(&Some(p.others)))
}

/// Each part of `rank` is in its range: relevance and terms 0 to 1, reputation -1 to 1, and the
/// score their weighted sum, -0.2 to 1.
fn rank_in_bounds(rank: &CatalogRank) -> bool {
    let within = |value: f64, low: f64, high: f64| {
        value.is_finite() && value >= low - 1e-9 && value <= high + 1e-9
    };
    within(rank.relevance, 0.0, 1.0)
        && within(rank.terms, 0.0, 1.0)
        && within(rank.reputation, -1.0, 1.0)
        && within(rank.score, -0.2, 1.0)
}

/// Check an entry that a server returned. A server is configured, not trusted with the shape
/// of what a host prints or requests: the resource must be a bounded `http` or `https` URL and
/// every text is bounded and holds no control character.
pub fn validate_entry(entry: &CatalogEntry) -> Result<()> {
    let invalid = || Error::uncertain("The server returned an invalid catalog entry");
    let text = |value: &str, max: usize| value.len() <= max && !value.chars().any(char::is_control);
    let list = |values: &[String], max: usize| {
        values.len() <= 32
            && values
                .iter()
                .all(|value| !value.is_empty() && text(value, max))
    };
    let link =
        |value: &str| crate::paid_service::resource_url(value).is_ok() && !value.contains('#');
    // The seller transport requests no URL with a fragment, so no entry offers one.
    if !link(&entry.resource) {
        return Err(invalid());
    }
    let token = |value: &str, max: usize| {
        !value.is_empty() && value.len() <= max && value.bytes().all(|b| b.is_ascii_graphic())
    };
    let optional =
        |value: &Option<String>, check: &dyn Fn(&str) -> bool| value.as_deref().is_none_or(check);
    let source = entry.source.as_ref().is_none_or(|source| {
        optional(&source.listing, &link) && optional(&source.captured, &|v| token(v, 300))
    });
    let access = entry.access.len() <= MAX_ITEMS
        && entry.access.iter().all(|access| {
            optional(&access.protocol, &|v| token(v, 32))
                && optional(&access.scheme, &|v| token(v, 32))
                && optional(&access.network, &|v| token(v, 64))
                && optional(&access.asset, &|v| token(v, 128))
                && optional(&access.detail, &|v| text(v, MAX_DETAIL))
        });
    let observed = entry.observed.len() <= MAX_ITEMS
        && entry.observed.iter().all(|fact| {
            token(&fact.at, 40)
                && optional(&fact.scheme, &|v| token(v, 32))
                && optional(&fact.network, &|v| token(v, 64))
                && optional(&fact.asset, &|v| token(v, 128))
                && optional(&fact.amount, &|v| {
                    token(v, 40) && v.bytes().all(|b| b.is_ascii_digit())
                })
                && optional(&fact.pay_to, &|v| token(v, 128))
                && optional(&fact.detail, &|v| text(v, MAX_DETAIL))
        });
    let reputation = entry.reputation.as_ref().is_none_or(labels_in_bounds);
    let rank = entry.rank.as_ref().is_none_or(rank_in_bounds);
    if !(source && access && observed && reputation && rank) {
        return Err(invalid());
    }
    if entry.id.is_empty()
        || entry.id.len() > MAX_ENTRY_ID_BYTES
        || !entry.id.bytes().all(|byte| byte.is_ascii_graphic())
        || entry.name.is_empty()
        || !text(&entry.name, 200)
        || !text(&entry.description, 2000)
        || !text(&entry.provider, 200)
        || !list(&entry.methods, 16)
        || !list(&entry.networks, 64)
        || !list(&entry.assets, 128)
        || !list(&entry.tags, 64)
        || entry
            .terms
            .as_ref()
            .is_some_and(|terms| !text(&terms.price, 40) || !text(&terms.unit, 32))
    {
        return Err(invalid());
    }
    Ok(())
}

/// Search the catalog of `server`.
pub fn search(server: &str, query: &Query) -> Result<SearchResult> {
    query.validate()?;
    let body = get(server, SEARCH_PATH, &query.encoded())?;
    let result: SearchResult = serde_json::from_slice(&body)
        .map_err(|_| Error::uncertain("The server returned an invalid search result"))?;
    if result.entries.len() > MAX_SEARCH_LIMIT {
        return Err(Error::uncertain(
            "The server returned more entries than a page holds",
        ));
    }
    for entry in &result.entries {
        validate_entry(entry)?;
    }
    Ok(result)
}

/// The reply of the entry route.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryReply {
    entry: CatalogEntry,
    #[allow(dead_code)]
    catalog: String,
}

/// Read one catalog entry of `server` by its id.
pub fn entry(server: &str, id: &str) -> Result<CatalogEntry> {
    if id.is_empty() || id.len() > MAX_ENTRY_ID_BYTES || !id.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Error::usage("The catalog entry id is invalid."));
    }
    let body = get(server, ENTRY_PATH, &format!("id={}", component(id)))?;
    let reply: EntryReply = serde_json::from_slice(&body)
        .map_err(|_| Error::uncertain("The server returned an invalid catalog entry"))?;
    if reply.entry.id != id {
        return Err(Error::uncertain(
            "The server returned another catalog entry",
        ));
    }
    validate_entry(&reply.entry)?;
    Ok(reply.entry)
}

/// The owner's vote route of a server, relative to its origin.
pub const REPUTATION_PATH: &str = "/api/native/reputation";
/// The route of the owner's votes, relative to its origin.
pub const VOTES_PATH: &str = "/api/native/reputation/votes";
/// The most request IDs of one read of votes.
pub const MAX_VOTES_READ: usize = 100;

fn token(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && value.bytes().all(|b| b.is_ascii_graphic())
}

/// The owner's `vote` on `signal` for the settled request `request_id` of `policy_id`. The server
/// reads the request and its subject from the wallet's journal. The reply is the server's
/// `{outcome, requestId, subject, votes, labels}`: `outcome` is `added`, `unchanged` or `edited`.
pub fn contribute(
    cx: &crate::owner::Context<'_>,
    policy_id: &str,
    request_id: &str,
    signal: Signal,
    vote: Vote,
) -> Result<Value> {
    if !token(policy_id, 128) || !token(request_id, 128) {
        return Err(Error::usage("The policy ID or the request ID is invalid."));
    }
    let reply = crate::owner::call(
        cx,
        &format!("{REPUTATION_PATH}?network={}", component(cx.network)),
        &json!({"policyId": policy_id, "requestId": request_id, "signal": signal, "vote": vote}),
    )?;
    checked_votes(&reply)?;
    Ok(reply)
}

/// The owner's votes on the requests `request_ids` of `policy_id`, at most
/// [`MAX_VOTES_READ`], with the labels of each request's subject. The reply is the server's
/// `{requests: {<id>: {settled, subject, votes, labels}}}`.
pub fn votes(
    cx: &crate::owner::Context<'_>,
    policy_id: &str,
    request_ids: &[String],
) -> Result<Value> {
    if !token(policy_id, 128)
        || request_ids.is_empty()
        || request_ids.len() > MAX_VOTES_READ
        || !request_ids.iter().all(|id| token(id, 128))
    {
        return Err(Error::usage(
            "Name the policy and 1 to 100 valid request IDs.",
        ));
    }
    let reply = crate::owner::call(
        cx,
        &format!("{VOTES_PATH}?network={}", component(cx.network)),
        &json!({"policyId": policy_id, "requestIds": request_ids}),
    )?;
    checked_reads(&reply)?;
    Ok(reply)
}

/// A read of votes: `requests` is an object, and each of its entries reads as Core's types.
fn checked_reads(reply: &Value) -> Result<()> {
    let requests = reply["requests"]
        .as_object()
        .ok_or_else(|| Error::uncertain("The server returned invalid reputation labels"))?;
    requests.values().try_for_each(checked_votes)
}

/// The votes and labels of one reply, which must read as Core's types.
fn checked_votes(reply: &Value) -> Result<()> {
    let invalid = || Error::uncertain("The server returned invalid reputation labels");
    serde_json::from_value::<std::collections::BTreeMap<Signal, Vote>>(reply["votes"].clone())
        .map_err(|_| invalid())?;
    let labels =
        serde_json::from_value::<Labels>(reply["labels"].clone()).map_err(|_| invalid())?;
    if !labels_in_bounds(&labels) {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    fn sample() -> serde_json::Value {
        json!({"id":"sample/report","name":"Report","description":"A sample report.",
            "provider":"Sample","resource":"https://seller.example/report","methods":["GET"],
            "terms":{"price":"0.01","unit":"USD"},"networks":["example:testnet"],
            "assets":["XYZ"],"tags":["sample"]})
    }
    /// A local server that answers one request and returns its request line.
    fn server(status: &str, body: serde_json::Value) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let status = status.to_owned();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut received = Vec::new();
            let mut buffer = [0u8; 4096];
            while !received.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                received.extend_from_slice(&buffer[..count]);
            }
            let body = body.to_string();
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            let text = String::from_utf8_lossy(&received).into_owned();
            text.lines().next().unwrap_or_default().to_owned()
        });
        (origin, handle)
    }

    #[test]
    #[cfg_attr(not(feature = "e2e"), ignore = "end-to-end: run with --features e2e")]
    fn a_search_sends_its_bounded_query_and_reads_the_page() {
        let (origin, handle) = server(
            "200 OK",
            json!({"entries":[sample()],"ranking":"hybrid","nextCursor":"20","catalog":"ab"}),
        );
        let result = search(
            &origin,
            &Query {
                text: "vault report & more".into(),
                network: Some("example:testnet".into()),
                limit: Some(20),
                cursor: None,
                ..Query::default()
            },
        )
        .unwrap();
        assert_eq!(result.ranking, Ranking::Hybrid);
        assert_eq!(result.entries[0].id, "sample/report");
        assert_eq!(result.next_cursor.as_deref(), Some("20"));
        assert_eq!(
            handle.join().unwrap(),
            "GET /api/marketplace/search?q=vault%20report%20%26%20more&network=example%3Atestnet&limit=20 HTTP/1.1"
        );
    }

    /// An entry as the server returns it from a search with text.
    fn full() -> serde_json::Value {
        let mut entry = sample();
        entry["source"] = json!({"listing":"https://seller.example/openapi.json",
            "captured":"wiki/providers/seller/report/README.md"});
        entry["operation"] = json!("transfer");
        entry["access"] = json!([{"kind":"payment","protocol":"x402","scheme":"exact",
            "network":"example:testnet","asset":"XYZ"},
            {"kind":"proof_of_work","detail":"A 16-bit SHA-256 solution within 300 seconds."}]);
        entry["observed"] = json!([{"fact":"response","origin":"registry",
            "at":"2026-10-28T20:44:59Z","status":402,"scheme":"exact","network":"example:testnet",
            "asset":"XYZ","amount":"100000","payTo":"GSELLER","payable":true},
            {"fact":"settlement","origin":"server","at":"2026-11-18","count":3,"amount":"100000"}]);
        entry["reputation"] = json!({"suitability":{"others":{"support":0}},
            "delivery":{"policy":{"support":1,"good":33,"bad":0},"owner":{"support":0},"others":{"support":5,"good":55,"bad":10}},
            "quality":{"others":{"support":0}}});
        entry["rank"] = json!({"score":0.695,"relevance":0.7143,"reputation":0.6,"terms":0.75});
        entry
    }

    #[test]
    fn a_full_entry_keeps_its_listing_apart_from_its_source_access_and_observations() {
        let entry: CatalogEntry = serde_json::from_value(full()).unwrap();
        validate_entry(&entry).unwrap();
        assert_eq!(entry.operation, Some(CatalogOperation::Transfer));
        assert!(entry.offers(AccessKind::ProofOfWork));
        assert!(!entry.offers(AccessKind::Free));
        assert_eq!(entry.observed[0].origin, ObservedOrigin::Registry);
        assert_eq!(entry.observed[1].fact, ObservedFact::Settlement);
        let labels = entry.reputation.unwrap();
        assert!(labels.suitability.others.is_unknown());
        assert_eq!(labels.delivery.policy.unwrap().good, Some(33));
        assert_eq!(serde_json::to_value(&entry).unwrap(), full());
        // An entry of a server before these fields still reads.
        let plain: CatalogEntry = serde_json::from_value(sample()).unwrap();
        assert!(plain.access.is_empty() && plain.reputation.is_none());
        let refused = |change: &dyn Fn(&mut serde_json::Value)| {
            let mut value = full();
            change(&mut value);
            let entry: CatalogEntry = serde_json::from_value(value).unwrap();
            assert_eq!(validate_entry(&entry).unwrap_err().code, 5);
        };
        refused(&|e| e["source"]["listing"] = json!("javascript:alert(1)"));
        refused(&|e| e["access"][1]["detail"] = json!("a".repeat(401)));
        refused(&|e| e["access"][0]["network"] = json!("a b"));
        refused(&|e| e["observed"][0]["amount"] = json!("1e9"));
        refused(&|e| e["observed"][0]["detail"] = json!("line\u{1b}[2J"));
        refused(&|e| e["reputation"]["delivery"]["others"]["good"] = json!(101));
        refused(&|e| e["rank"]["score"] = json!(9.0));
        // An unknown access kind or field is no part of an entry.
        let mut unknown = full();
        unknown["access"][0]["kind"] = json!("gift");
        assert!(serde_json::from_value::<CatalogEntry>(unknown).is_err());
        let mut extra = full();
        extra["observed"][0]["note"] = json!("run this");
        assert!(serde_json::from_value::<CatalogEntry>(extra).is_err());
    }

    #[test]
    #[cfg_attr(not(feature = "e2e"), ignore = "end-to-end: run with --features e2e")]
    fn a_search_sends_its_capability_access_and_asset_filters() {
        let (origin, handle) = server(
            "200 OK",
            json!({"entries":[full()],"ranking":"keyword","catalog":"ab"}),
        );
        let result = search(
            &origin,
            &Query {
                text: "report".into(),
                asset: Some("XYZ".into()),
                access: Some(AccessKind::FreeQuota),
                capability: Some("finance".into()),
                ..Query::default()
            },
        )
        .unwrap();
        assert_eq!(result.entries[0].rank.unwrap().score, 0.695);
        assert_eq!(
            handle.join().unwrap(),
            "GET /api/marketplace/search?q=report&asset=XYZ&access=free_quota&capability=finance HTTP/1.1"
        );
        assert_eq!(
            AccessKind::parse("proof_of_work"),
            Some(AccessKind::ProofOfWork)
        );
        assert_eq!(AccessKind::parse("gift"), None);
        for bad in [
            Query {
                asset: Some("a b".into()),
                ..Query::default()
            },
            Query {
                capability: Some(String::new()),
                ..Query::default()
            },
        ] {
            assert_eq!(search("http://127.0.0.1:1", &bad).unwrap_err().code, 2);
        }
    }

    #[test]
    #[cfg_attr(not(feature = "e2e"), ignore = "end-to-end: run with --features e2e")]
    fn a_catalog_request_asks_for_the_second_shape() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut received = Vec::new();
            let mut buffer = [0u8; 4096];
            while !received.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                received.extend_from_slice(&buffer[..count]);
            }
            let body = json!({"entries":[],"ranking":"keyword","catalog":"ab"}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            String::from_utf8_lossy(&received).to_lowercase()
        });
        search(&origin, &Query::default()).unwrap();
        let head = handle.join().unwrap();
        assert!(
            head.contains("\r\naccept: application/json; allowit-catalog=2\r\n"),
            "{head}"
        );
    }

    #[test]
    #[cfg_attr(not(feature = "e2e"), ignore = "end-to-end: run with --features e2e")]
    fn an_entry_reads_by_its_id_and_another_entry_is_refused() {
        let (origin, handle) = server("200 OK", json!({"entry":sample(),"catalog":"ab"}));
        assert_eq!(entry(&origin, "sample/report").unwrap().name, "Report");
        assert_eq!(
            handle.join().unwrap(),
            "GET /api/marketplace/entry?id=sample%2Freport HTTP/1.1"
        );
        let (origin, handle) = server("200 OK", json!({"entry":sample(),"catalog":"ab"}));
        assert_eq!(entry(&origin, "sample/other").unwrap_err().code, 5);
        handle.join().unwrap();
    }

    #[test]
    #[cfg_attr(not(feature = "e2e"), ignore = "end-to-end: run with --features e2e")]
    fn a_typed_refusal_keeps_its_code_and_bounded_text() {
        let (origin, handle) = server(
            "503 Service Unavailable",
            json!({"error":"The marketplace catalog is not configured.\u{1b}[31m","code":"marketplace_unavailable"}),
        );
        let error = search(&origin, &Query::default()).unwrap_err();
        assert_eq!(error.status, Some(503));
        assert_eq!(error.api_code.as_deref(), Some("marketplace_unavailable"));
        assert!(!error.message.contains('\u{1b}'));
        handle.join().unwrap();
    }

    #[test]
    fn a_query_outside_its_bounds_never_reaches_the_server() {
        let refused = |query: Query| {
            assert_eq!(search("http://127.0.0.1:1", &query).unwrap_err().code, 2);
        };
        refused(Query {
            text: "a".repeat(MAX_QUERY_BYTES + 1),
            ..Default::default()
        });
        refused(Query {
            text: "a\nb".into(),
            ..Default::default()
        });
        for limit in [0, MAX_SEARCH_LIMIT + 1] {
            refused(Query {
                limit: Some(limit),
                ..Default::default()
            });
        }
        refused(Query {
            network: Some("a b".into()),
            ..Default::default()
        });
        refused(Query {
            cursor: Some(String::new()),
            ..Default::default()
        });
        for id in ["", "a b", &"a".repeat(MAX_ENTRY_ID_BYTES + 1)] {
            assert_eq!(entry("http://127.0.0.1:1", id).unwrap_err().code, 2);
        }
    }

    #[test]
    fn an_entry_with_an_unsafe_resource_or_text_is_refused() {
        let refused = |change: &dyn Fn(&mut serde_json::Value)| {
            let mut value = sample();
            change(&mut value);
            let entry: CatalogEntry = serde_json::from_value(value).unwrap();
            assert_eq!(validate_entry(&entry).unwrap_err().code, 5);
        };
        validate_entry(&serde_json::from_value(sample()).unwrap()).unwrap();
        refused(&|e| e["resource"] = json!("javascript:alert(1)"));
        refused(&|e| e["resource"] = json!("file:///etc/passwd"));
        refused(&|e| e["resource"] = json!("https://seller.example/report#fragment"));
        refused(&|e| e["name"] = json!("Report\u{1b}[2J"));
        refused(&|e| e["name"] = json!(""));
        refused(&|e| e["description"] = json!("a".repeat(2001)));
        refused(&|e| e["id"] = json!("a b"));
        refused(&|e| e["tags"] = json!(vec!["t"; 33]));
        refused(&|e| e["terms"]["unit"] = json!("USD\n"));
        // An executable or unknown field is no part of an entry.
        let mut extra = sample();
        extra["instructions"] = json!("run this");
        assert!(serde_json::from_value::<CatalogEntry>(extra).is_err());
    }

    #[test]
    fn owner_replies_keep_their_shape_and_their_bounds() {
        let unknown = json!({"others": {"support": 0}});
        let labels = json!({"suitability": unknown, "delivery": unknown, "quality": unknown});
        let entry = |labels: Value| json!({"settled": true, "subject": "service:x", "votes": {"quality": "good"}, "labels": labels});
        // A read without a requests object is not a read.
        for reply in [
            json!({}),
            json!({"requests": null}),
            json!({"requests": []}),
        ] {
            assert!(checked_reads(&reply).is_err(), "{reply}");
        }
        checked_reads(&json!({"requests": {}})).unwrap();
        checked_reads(&json!({"requests": {"r1": entry(labels.clone())}})).unwrap();
        // A score above 100 is refused, as in a catalog entry.
        let mut high = labels.clone();
        high["quality"]["others"] = json!({"support": 1, "good": 4_294_967_295u64, "bad": 0});
        assert!(checked_votes(&entry(high.clone())).is_err());
        assert!(checked_reads(&json!({"requests": {"r1": entry(high)}})).is_err());
    }

    #[test]
    fn each_rank_part_keeps_its_own_range() {
        let rank = |score: f64, relevance: f64, reputation: f64, terms: f64| CatalogRank {
            score,
            relevance,
            reputation,
            terms,
        };
        assert!(rank_in_bounds(&rank(0.98, 1.0, 0.9, 1.0)));
        assert!(rank_in_bounds(&rank(-0.2, 0.0, -1.0, 0.0)));
        for bad in [
            rank(0.5, -2.0, 0.0, 0.5),
            rank(0.5, 0.5, 2.0, 0.5),
            rank(0.5, 0.5, 0.0, -2.0),
            rank(1.5, 1.0, 1.0, 1.0),
            rank(f64::NAN, 0.5, 0.0, 0.5),
        ] {
            assert!(!rank_in_bounds(&bad), "{bad:?}");
        }
    }
}
