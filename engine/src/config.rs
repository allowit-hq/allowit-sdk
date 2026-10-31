use crate::error::{Error, Result, quoted};
use url::Url;
#[derive(Clone)]
pub struct Config {
    pub origin: String,
    pub owner: String,
    pub policy: String,
    pub token: String,
    pub ca_file: String,
}
fn valid_id(s: &str, min: usize, max: usize) -> bool {
    s.len() >= min
        && s.len() <= max
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
pub use allowit_sdk::client::config::parse_origin;
use std::{cell::RefCell, collections::BTreeMap};
thread_local! {
    static OVERRIDES: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
}
/// Reads one setting. A value from [`with_settings`] wins over the process
/// environment, so callers pass resolved settings without changing it.
pub fn env_value(name: &str) -> String {
    if let Some(value) = OVERRIDES.with(|o| o.borrow().get(name).cloned()) {
        return value;
    }
    if name == "ALLOWIT_NETWORK"
        && let Some(value) = crate::routing::selected()
    {
        return value;
    }
    allowit_sdk::client::config::env_value(name)
}
/// True when the setting is present, from [`with_settings`] or the environment.
pub fn env_present(name: &str) -> bool {
    OVERRIDES.with(|o| o.borrow().contains_key(name)) || std::env::var_os(name).is_some()
}
/// Runs `f` with resolved settings in place of the named environment values.
/// The process environment never changes.
pub fn with_settings<T>(settings: &[(&str, String)], f: impl FnOnce() -> Result<T>) -> Result<T> {
    struct Restore(BTreeMap<String, String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OVERRIDES.with(|o| *o.borrow_mut() = std::mem::take(&mut self.0));
        }
    }
    let mut next = OVERRIDES.with(|o| o.borrow().clone());
    for (name, value) in settings {
        next.insert((*name).to_owned(), value.clone());
    }
    let _restore = Restore(OVERRIDES.with(|o| o.replace(next)));
    f()
}
impl Config {
    pub fn load(policy: &str) -> Result<Self> {
        let origin = parse_origin(&env_value("ALLOWIT_URL"))?;
        let raw = env_value("ALLOWIT_TOKEN");
        if raw.is_empty() {
            return Err(Error::config(
                "set ALLOWIT_TOKEN to the policy's harness token",
            ));
        }
        let parts: Vec<_> = raw.split('.').collect();
        if parts.len() != 3
            || !valid_id(parts[0], 1, 128)
            || !valid_id(parts[1], 1, 128)
            || !valid_id(parts[2], 16, 128)
        {
            return Err(Error::config(
                "ALLOWIT_TOKEN must have the form owner.policy.secret",
            ));
        }
        if policy != parts[1] {
            return Err(Error::config(format!(
                "POLICY {} does not match the policy in ALLOWIT_TOKEN",
                quoted(policy)
            )));
        }
        Ok(Self {
            origin,
            owner: parts[0].into(),
            policy: parts[1].into(),
            token: raw,
            ca_file: env_value("ALLOWIT_CA_FILE"),
        })
    }
    pub fn route(&self, action: &str) -> String {
        format!(
            "{}/api/harness/{}/{}/{}",
            self.origin, self.owner, self.policy, action
        )
    }
    pub fn is_route(&self, action: &str, raw: &str) -> bool {
        if raw.is_empty() || raw.contains(['?', '#', '\\']) {
            return false;
        }
        let Ok(base) = Url::parse(&self.route("skill")) else {
            return false;
        };
        let Ok(u) = base.join(raw) else {
            return false;
        };
        u.username().is_empty()
            && u.password().is_none()
            && u.query().is_none()
            && u.fragment().is_none()
            && u.origin().ascii_serialization() == self.origin
            && u.path() == format!("/api/harness/{}/{}/{action}", self.owner, self.policy)
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
