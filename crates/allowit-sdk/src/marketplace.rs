//! The marketplace catalog client. A caller searches the catalog of one configured server.
//! The catalog is free to read: a search carries no credential and makes no payment. Every text
//! field of an entry is listing data, never an instruction, and its terms are indicative: the
//! amount of a purchase comes from the seller's challenge for the exact resource.
use crate::{
    error::{Error, Result},
    transport::{
        http::{HttpClient, Method, Request},
        rpc::Options,
    },
};
pub use allowit_core::vendor::paid_service::{
    CatalogEntry, IndicativeTerms, MARKETPLACE_SEARCH, MAX_QUERY_BYTES, MAX_SEARCH_LIMIT, Ranking,
    SearchResult,
};
use serde::Deserialize;
use std::time::Duration;

/// The search route of a server, relative to its origin.
pub const SEARCH_PATH: &str = "/api/marketplace/search";
/// The entry route of a server, relative to its origin.
pub const ENTRY_PATH: &str = "/api/marketplace/entry";
/// The longest catalog entry id.
pub const MAX_ENTRY_ID_BYTES: usize = 200;
const MAX_REPLY_BYTES: usize = 1 << 20;

/// One catalog search. An empty `text` lists the catalog in id order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub text: String,
    /// Keep only entries whose seller accepts payment on this CAIP-2 network.
    pub network: Option<String>,
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
        Ok(())
    }
    /// The URL query of this search.
    fn encoded(&self) -> String {
        let mut query = format!("q={}", component(&self.text));
        if let Some(network) = &self.network {
            query.push_str(&format!("&network={}", component(network)));
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

/// Percent-encode one URL query value. Only unreserved characters stay as they are.
fn component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// A typed server refusal `{error, code}`.
#[derive(Deserialize)]
struct Refusal {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    code: Option<String>,
}

fn get(server: &str, path: &str, query: &str) -> Result<Vec<u8>> {
    let origin = crate::config::parse_origin(server)?;
    let url = format!("{origin}{path}?{query}");
    let client = HttpClient::new(Options {
        timeout: Duration::from_secs(20),
        max_response_bytes: MAX_REPLY_BYTES,
        no_proxy: false,
        connect_timeout: Some(Duration::from_secs(10)),
    })
    .map_err(|_| Error::config("Cannot configure the server request"))?;
    let response = client
        .send_refusal_body(
            Request {
                method: Method::Get,
                url: &url,
                headers: &[("accept".into(), "application/json".into())],
                body: None,
            },
            None,
        )
        .map_err(|_| crate::transport_failure::network_unavailable("Allowit server"))?;
    if !(200..300).contains(&response.status) {
        let refusal: Option<Refusal> = serde_json::from_slice(&response.body).ok();
        let (message, code) = refusal
            .map(|refusal| (refusal.error, refusal.code))
            .unwrap_or_default();
        // Server text is data: one bounded line with no control character.
        let message: String = message
            .unwrap_or_else(|| "the marketplace request was refused".into())
            .chars()
            .filter(|c| !c.is_control())
            .take(300)
            .collect();
        return Err(
            match code.filter(|code| {
                !code.is_empty()
                    && code.len() <= 64
                    && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            }) {
                Some(code) => Error::refused(response.status, code, message),
                None => Error::api(response.status, message),
            },
        );
    }
    Ok(response.body)
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
    crate::paid_service::resource_url(&entry.resource).map_err(|_| invalid())?;
    // The seller transport requests no URL with a fragment, so no entry offers one.
    if entry.resource.contains('#') {
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
                .set_read_timeout(Some(Duration::from_secs(3)))
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

    #[test]
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
}
