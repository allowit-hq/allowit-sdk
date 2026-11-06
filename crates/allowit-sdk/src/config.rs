use crate::error::Result;
pub use allowit_core::client::config::parse_origin;
use allowit_core::client::config::{override_value, with_overrides};
/// Reads one setting. A value from [`with_settings`] wins over the process
/// environment, so callers pass resolved settings without changing it.
pub fn env_value(name: &str) -> String {
    if let Some(value) = override_value(name) {
        return value;
    }
    if name == "ALLOWIT_NETWORK"
        && let Some(value) = crate::routing::selected()
    {
        return value;
    }
    allowit_core::client::config::env_value(name)
}
/// True when the setting is present, from [`with_settings`] or the environment.
pub fn env_present(name: &str) -> bool {
    override_value(name).is_some() || std::env::var_os(name).is_some()
}
/// Runs `f` with resolved settings in place of the named environment values.
/// The process environment never changes. Rail crates that read settings
/// through `allowit-sdk` see the same values.
pub fn with_settings<T>(settings: &[(&str, String)], f: impl FnOnce() -> Result<T>) -> Result<T> {
    with_overrides(settings, f)
}
/// Independently configured public authority for one backend and selected rail.
#[derive(Clone, Debug)]
pub struct ServerAuthority(pub(crate) allowit_core::vendor::config::ServerAuthority);
impl std::ops::Deref for ServerAuthority {
    type Target = allowit_core::vendor::config::ServerAuthority;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl ServerAuthority {
    pub fn new(network: &str, origin: &str, public_key: &str) -> Result<Self> {
        if origin.is_empty() {
            return Err(crate::error::Error::config(
                "Configure an explicit server origin and public identity",
            ));
        }
        let network = crate::routing::normalize(network)?;
        crate::input::address::check_account(crate::routing::family(network)?, public_key)
            .map_err(|_| {
                crate::error::Error::usage("Invalid public server identity for the selected rail")
            })?;
        allowit_core::vendor::config::ServerAuthority::validated(network, origin, public_key)
            .map(Self)
    }
    pub fn network(&self) -> &str {
        self.0.network()
    }
    pub fn origin(&self) -> &str {
        self.0.origin()
    }
    pub fn public_key(&self) -> &str {
        self.0.public_key()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_are_scoped_and_never_touch_the_environment() {
        let name = "ALLOWIT_TEST_SCOPED_SETTING";
        assert!(!env_present(name));
        with_settings(&[(name, "value".into())], || {
            assert_eq!(env_value(name), "value");
            assert!(env_present(name));
            assert!(std::env::var_os(name).is_none());
            Ok(())
        })
        .unwrap();
        assert_eq!(env_value(name), "");
    }
}
