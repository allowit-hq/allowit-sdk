//! Local agent keys. PublicWallet is the only serializable registration value.
use crate::{
    error::{Error, Result},
    routing,
};
use allowit_sdk::client::store::Store;
use serde::{Deserialize, Serialize};
use std::path::Path;
use zeroize::{Zeroize, Zeroizing};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicWallet {
    pub network: String,
    pub address: String,
    pub public_key: String,
    pub key_type: String,
}
fn public(network: &str, seed: &[u8; 32]) -> Result<PublicWallet> {
    let network = routing::normalize(network)?;
    let address = allowit_stellar_wallet::account_for_seed(seed);
    Ok(PublicWallet {
        network: network.into(),
        public_key: address.clone(),
        address,
        key_type: "ed25519".into(),
    })
}
fn stellar_address(key: &[u8; 32]) -> String {
    stellar_strkey::ed25519::PublicKey(*key).to_string()
}
fn storage(path: &Path) -> Result<(Store, String)> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| Error::usage("Choose a private wallet file"))?;
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || name.is_empty()
    {
        return Err(Error::usage("Invalid wallet filename"));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok((Store::open(parent)?, name.into()))
}
/// Generates an executor key in the rail's existing private-file format.
pub fn generate_wallet(network: &str, path: &Path) -> Result<PublicWallet> {
    routing::info(network)?;
    let (_lock, _name) = storage(path)?;
    let mut seed = Zeroizing::new([0u8; 32]);
    getrandom::fill(seed.as_mut())
        .map_err(|_| Error::config("Operating-system randomness unavailable"))?;
    let metadata = public(network, &seed)?;
    let mut private = stellar_strkey::ed25519::PrivateKey(*seed);
    let secret = Zeroizing::new(private.to_string());
    private.0.zeroize();
    write_new(path, secret.as_bytes())?;
    Ok(metadata)
}
#[cfg(unix)]
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| {
            Error::config("Cannot create new private signer file; existing keys are never replaced")
        })?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| Error::config("Cannot durably save private signer"))?;
    Ok(())
}
#[cfg(not(unix))]
fn write_new(_: &Path, _: &[u8]) -> Result<()> {
    Err(Error::unsupported(
        "Private wallet generation requires a supported POSIX filesystem",
    ))
}
/// Read a private native signer into a zeroizing buffer under its storage lock.
fn read_signer(path: &Path) -> Result<Zeroizing<String>> {
    let (_lock, _) = storage(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::symlink_metadata(path)
            .map_err(|_| Error::config("Cannot read private signer"))?;
        if !m.is_file()
            || m.mode() & 0o7777 != 0o600
            || m.nlink() != 1
            || m.uid() != unsafe { libc::geteuid() }
        {
            return Err(Error::config("Unsafe private signer file"));
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| Error::config("Cannot open private signer"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = file
            .metadata()
            .map_err(|_| Error::config("Cannot inspect signer"))?;
        if !m.is_file()
            || m.mode() & 0o7777 != 0o600
            || m.nlink() != 1
            || m.uid() != unsafe { libc::geteuid() }
        {
            return Err(Error::config("Unsafe private signer file"));
        }
    }
    use std::io::Read;
    let mut text = Zeroizing::new(String::new());
    file.take(4097)
        .read_to_string(&mut text)
        .map_err(|_| Error::config("Cannot read signer"))?;
    if text.len() > 4096 {
        return Err(Error::config("Signer file too large"));
    }
    Ok(text)
}
pub fn stellar_signing_seed(path: &Path) -> Result<Zeroizing<[u8; 32]>> {
    let text = read_signer(path)?;
    allowit_stellar_wallet::decode_seed(text.trim())
        .map(Zeroizing::new)
        .map_err(|_| Error::config("Invalid local Stellar signer"))
}
/// Inspect a native signer file without returning its secret.
pub fn inspect_wallet(network: &str, path: &Path) -> Result<PublicWallet> {
    routing::info(network)?;
    public(network, &*stellar_signing_seed(path)?)
}
#[cfg(test)]
fn built_networks() -> [&'static str; 2] {
    ["stellar:testnet", "stellar:pubnet"]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_metadata_cannot_serialize_secret() {
        let seed = [7u8; 32];
        for network in built_networks() {
            let p = public(network, &seed).unwrap();
            let v = serde_json::to_value(p).unwrap();
            assert_eq!(v.as_object().unwrap().len(), 4);
            assert!(v.get("seed").is_none());
            assert!(v.get("privateKey").is_none());
        }
    }
    #[test]
    fn known_stellar_addresses() {
        assert_eq!(
            stellar_address(&[0; 32]),
            "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"
        );
        let stellar = public("stellar:testnet", &[0; 32]).unwrap();
        assert_eq!(stellar.public_key, stellar.address);
        assert!(stellar.public_key.starts_with('G'));
        stellar.validate().unwrap();
    }
}

impl PublicWallet {
    pub fn validate(&self) -> Result<()> {
        if routing::normalize(&self.network)? != self.network {
            return Err(Error::usage("Registration requires a canonical network"));
        }
        if self.key_type != "ed25519" {
            return Err(Error::usage("Invalid public key type"));
        }
        let key = allowit_stellar_wallet::account_key(&self.public_key)
            .map_err(|_| Error::usage("Invalid public key"))?;
        ed25519_dalek::VerifyingKey::from_bytes(&key)
            .map_err(|_| Error::usage("Invalid public point"))?;
        let expected = stellar_address(&key);
        if expected != self.address {
            return Err(Error::usage("Address differs from public key"));
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod storage_tests {
    use super::*;
    use std::{
        os::unix::fs::{PermissionsExt, symlink},
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn path() -> std::path::PathBuf {
        std::env::temp_dir()
            .canonicalize()
            .expect("Test temporary directory must exist")
            .join(format!(
                "allowit-local-key-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ))
            .join("agent.key")
    }
    #[test]
    fn generated_files_are_usable_private_and_never_replaced() {
        for n in built_networks() {
            let p = path();
            let public = generate_wallet(n, &p).unwrap();
            public.validate().unwrap();
            assert_eq!(inspect_wallet(n, &p).unwrap(), public);
            assert_eq!(
                std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
                0o600
            );
            let before = std::fs::read(&p).unwrap();
            assert!(generate_wallet(n, &p).is_err());
            assert_eq!(before, std::fs::read(&p).unwrap());
            std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
        }
    }
    #[test]
    fn symlink_hardlink_and_insecure_files_are_refused() {
        let p = path();
        generate_wallet("stellar:testnet", &p).unwrap();
        let alias = p.with_file_name("alias");
        symlink(&p, &alias).unwrap();
        assert!(inspect_wallet("stellar:testnet", &alias).is_err());
        assert!(generate_wallet("stellar:testnet", &alias).is_err());
        std::fs::remove_file(&alias).unwrap();
        std::fs::hard_link(&p, &alias).unwrap();
        assert!(inspect_wallet("stellar:testnet", &p).is_err());
        std::fs::remove_file(&alias).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(inspect_wallet("stellar:testnet", &p).is_err());
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
    #[test]
    fn concurrent_generation_has_exactly_one_winner() {
        let p = path();
        let mut threads = vec![];
        for _ in 0..8 {
            let p = p.clone();
            threads.push(std::thread::spawn(move || {
                generate_wallet("stellar:testnet", &p).is_ok()
            }));
        }
        let wins = threads
            .into_iter()
            .map(|t| usize::from(t.join().unwrap()))
            .sum::<usize>();
        assert_eq!(wins, 1);
        inspect_wallet("stellar:testnet", &p).unwrap();
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
}
