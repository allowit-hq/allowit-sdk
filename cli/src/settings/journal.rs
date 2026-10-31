//! The local request journal. `exec` records each request before it leaves
//! the process, so `status ID` finds its rail and wallet and `history` lists
//! it. Records are private files in `requests/`, one per request ID.
use crate::error::{Error, Result};
use allowit_sdk::client::store::Store;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Entry {
    pub request_id: String,
    pub command: String,
    pub rail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wallet: Option<String>,
    pub op: String,
    pub to: String,
    pub amount: String,
    pub created_at: u64,
    /// The last exit code, or `None` before the first result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<i32>,
}

fn open(dir: &Path) -> Result<Store> {
    super::private_dir(dir)?;
    Store::open(&dir.join("requests"))
        .map_err(|_| Error::config("the request journal is unsafe or locked"))
}

pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn load(dir: &Path, id: &str) -> Result<Option<Entry>> {
    if !dir.join("requests").exists() {
        return Ok(None);
    }
    open(dir)?.load(id)
}

/// Records a new request, or checks that an existing record has the same
/// intent. A request ID never names two different requests.
pub(crate) fn record(dir: &Path, entry: &Entry) -> Result<()> {
    let store = open(dir)?;
    if let Some(old) = store.load::<Entry>(&entry.request_id)? {
        if old.command != entry.command
            || old.rail != entry.rail
            || old.wallet != entry.wallet
            || old.op != entry.op
            || old.to != entry.to
            || old.amount != entry.amount
        {
            return Err(Error::usage(format!(
                "request ID {} already names a different request; use a new --request-id",
                entry.request_id
            )));
        }
        return Ok(());
    }
    store.save(&entry.request_id, entry)
}

pub(crate) fn finish(dir: &Path, id: &str, exit: i32) -> Result<()> {
    let store = open(dir)?;
    if let Some(mut entry) = store.load::<Entry>(id)? {
        entry.exit = Some(exit);
        store.save(id, &entry)?;
    }
    Ok(())
}

/// Every record, newest first.
pub(crate) fn entries(dir: &Path) -> Result<Vec<Entry>> {
    if !dir.join("requests").exists() {
        return Ok(vec![]);
    }
    let mut all: Vec<Entry> = open(dir)?.entries()?;
    all.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.request_id.cmp(&b.request_id))
    });
    Ok(all)
}

/// A fresh request ID: `req-` and 16 hex digits of OS randomness.
pub(crate) fn new_id() -> Result<String> {
    let mut bytes = [0u8; 8];
    getrandom(&mut bytes)?;
    Ok(format!(
        "req-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}

fn getrandom(bytes: &mut [u8]) -> Result<()> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(bytes))
        .map_err(|_| Error::config("operating-system randomness is unavailable"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn entry(id: &str, at: u64) -> Entry {
        Entry {
            request_id: id.into(),
            command: "exec".into(),
            rail: "stellar:testnet".into(),
            wallet: Some("CWALLET".into()),
            op: "transfer".into(),
            to: "GDEST".into(),
            amount: "XLM:5".into(),
            created_at: at,
            exit: None,
        }
    }
    #[test]
    fn records_bind_one_intent_and_list_newest_first() {
        let dir = crate::settings::temp_dir("journal");
        assert!(entries(&dir).unwrap().is_empty());
        assert!(load(&dir, "pay-0001").unwrap().is_none());
        record(&dir, &entry("pay-0001", 10)).unwrap();
        record(&dir, &entry("pay-0002", 20)).unwrap();
        record(&dir, &entry("pay-0001", 99)).unwrap();
        let mut other = entry("pay-0001", 10);
        other.amount = "XLM:6".into();
        assert_eq!(record(&dir, &other).unwrap_err().code, 2);
        finish(&dir, "pay-0001", 0).unwrap();
        let all = entries(&dir).unwrap();
        assert_eq!(all[0].request_id, "pay-0002");
        assert_eq!(all[1].exit, Some(0));
        let id = new_id().unwrap();
        assert!(crate::request::request_id_valid(&id), "{id}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
