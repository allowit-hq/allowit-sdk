//! Local settings under the config directory (`$XDG_CONFIG_HOME/allowit`,
//! default `~/.config/allowit`, or `--config-dir`):
//!
//! - `config.toml`: the current rail and, per rail, the default wallet, the
//!   agent key, known wallets, the engine URL, an RPC override and the
//!   paid-service asset.
//! - `keys/NAME.toml`: public key metadata. `secrets/NAME`: the private key,
//!   mode 0600 in a 0700 directory.
//! - `requests/`: the local request journal that `status` and `history` read.
pub(crate) mod journal;
pub(crate) mod keys;

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Settings for one rail, keyed by its canonical CAIP-2 identifier.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RailSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wallet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wallets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpc_url: Option<String>,
    /// The asset that `setup` chose for paid-service wallets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_asset: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rail: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rails: BTreeMap<String, RailSettings>,
}

/// The config directory and its parsed `config.toml`.
pub(crate) struct Settings {
    pub dir: PathBuf,
    pub config: Config,
}

/// The default config directory, following XDG.
pub(crate) fn default_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(dir).join("allowit"));
    }
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(|home| PathBuf::from(home).join(".config").join("allowit"))
        .ok_or_else(|| Error::config("HOME is not set; pass --config-dir DIR"))
}

impl Settings {
    pub(crate) fn path(dir: &Path) -> PathBuf {
        dir.join("config.toml")
    }
    /// Reads `config.toml`. A missing file is an empty configuration.
    pub(crate) fn load(dir: PathBuf) -> Result<Self> {
        let path = Self::path(&dir);
        let config = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).map_err(|e| {
                Error::config(format!("{} is invalid: {}", path.display(), e.message()))
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(_) => return Err(Error::config(format!("cannot read {}", path.display()))),
        };
        Ok(Self { dir, config })
    }
    /// Writes `config.toml` atomically.
    pub(crate) fn save(&self) -> Result<()> {
        let text = toml::to_string(&self.config)
            .map_err(|_| Error::config("cannot encode config.toml"))?;
        write_atomic(&Self::path(&self.dir), text.as_bytes(), 0o644)
    }
    pub(crate) fn rail(&self, id: &str) -> RailSettings {
        self.config.rails.get(id).cloned().unwrap_or_default()
    }
    pub(crate) fn rail_mut(&mut self, id: &str) -> &mut RailSettings {
        self.config.rails.entry(id.to_owned()).or_default()
    }
}

/// Creates a private directory (mode 0700) and its parents.
pub(crate) fn private_dir(path: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|_| Error::config(format!("cannot create {}", path.display())))
}

/// Writes a file through a temporary file and a rename.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| Error::config("invalid settings path"))?;
    private_dir(parent)?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        std::process::id()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let result = (|| {
        let mut file = options
            .open(&temporary)
            .map_err(|_| Error::config(format!("cannot write {}", path.display())))?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| Error::config(format!("cannot write {}", path.display())))?;
        std::fs::rename(&temporary, path)
            .map_err(|_| Error::config(format!("cannot write {}", path.display())))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
pub(crate) fn temp_dir(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().canonicalize().unwrap().join(format!(
        "allowit-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    private_dir(&dir).unwrap();
    dir
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_round_trips_and_missing_file_is_empty() {
        let dir = temp_dir("settings");
        let mut s = Settings::load(dir.clone()).unwrap();
        assert_eq!(s.config, Config::default());
        s.config.rail = Some("stellar:testnet".into());
        s.rail_mut("stellar:testnet").wallet = Some("CWALLET".into());
        s.save().unwrap();
        let again = Settings::load(dir.clone()).unwrap();
        assert_eq!(again.config, s.config);
        let text = std::fs::read_to_string(Settings::path(&dir)).unwrap();
        assert!(text.contains("[rails.\"stellar:testnet\"]"), "{text}");
        std::fs::write(Settings::path(&dir), "rail = 3").unwrap();
        assert_eq!(Settings::load(dir.clone()).err().unwrap().code, 3);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
