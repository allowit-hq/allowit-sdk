//! Rail plugin registry. Each vendor registers one [`RailPlugin`] in `registered.rs`; every
//! dispatch below looks the plugin up by network, so no function in this module names a vendor.
mod registered;
#[cfg(feature = "testkit")]
pub mod testkit;

use crate::{
    error::{Error, Result},
    routing::{Family, RailInfo},
};
pub use allowit_core::vendor::plugin::*;
pub(crate) use allowit_core::vendor::{PrivateSigner, RuntimeProvider};
use std::path::Path;

/// Every registered rail plugin, in registration order.
pub fn rails() -> &'static [&'static dyn RailPlugin] {
    registered::RAILS
}

/// Every network of every registered rail, in registration order.
pub fn networks() -> impl Iterator<Item = &'static RailInfo> {
    rails().iter().flat_map(|rail| rail.networks().iter())
}

/// The plugin that serves a CAIP-2 network id or alias. An unknown network is a usage error.
pub fn for_network(network: &str) -> Result<&'static dyn RailPlugin> {
    rails()
        .iter()
        .copied()
        .find(|rail| {
            rail.networks()
                .iter()
                .any(|n| n.id == network || n.aliases.contains(&network))
        })
        .ok_or_else(|| Error::usage(format!("unknown rail {}", crate::error::quoted(network))))
}

/// The plugin with this [`RailPlugin::id`].
pub fn by_id(id: &str) -> Result<&'static dyn RailPlugin> {
    rails()
        .iter()
        .copied()
        .find(|rail| rail.id() == id)
        .ok_or_else(|| Error::config(format!("unknown rail plugin {}", crate::error::quoted(id))))
}

/// The vendor policy function `namespace::name`, where `namespace` is a rail plugin id.
/// The `allowit` namespace belongs to Core and is never dispatched here.
pub fn policy_function(namespace: &str, name: &str) -> Option<PolicyFunction> {
    by_id(namespace)
        .ok()?
        .policy_functions()
        .iter()
        .find(|(function, _)| *function == name)
        .map(|(_, f)| *f)
}

/// Invoke a vendor policy function with guest arguments and host-supplied facts.
pub fn invoke_policy_function(
    namespace: &str,
    name: &str,
    args: &serde_json::Value,
    facts: &serde_json::Value,
) -> Result<serde_json::Value> {
    let f = policy_function(namespace, name).ok_or_else(|| {
        Error::usage(format!(
            "unregistered policy function {}",
            crate::error::quoted(&format!("{namespace}::{name}"))
        ))
    })?;
    f(args, facts).map_err(Error::config)
}

pub(crate) fn for_family(family: Family) -> Result<&'static dyn RailPlugin> {
    rails()
        .iter()
        .copied()
        .find(|rail| rail.networks().iter().any(|n| n.family == family))
        .ok_or_else(|| Error::unsupported("The selected build has no matching rail plugin"))
}

/// Store namespaces that the SDK assigns to a plugin under the caller's root.
pub(crate) fn execution_store(root: &Path, rail: &dyn RailPlugin) -> Result<crate::Store> {
    crate::Store::open(&root.join(format!("{}-journal", rail.id())))
}
pub(crate) fn owner_store(root: &Path, network: &str) -> Result<crate::Store> {
    crate::Store::open(&root.join(format!("{}-owner-workflows", for_network(network)?.id())))
}

pub(crate) fn source_consent<P: serde::Serialize>(
    input: &crate::runtime::owner::SourceConsent<'_, P>,
) -> Result<(serde_json::Value, String)> {
    let params = serde_json::to_value(input.params)
        .map_err(|_| Error::config("Cannot encode policy parameters"))?;
    for_network(&input.configuration.network)?
        .owner_runtime()
        .source_consent(&allowit_core::vendor::runtime::owner::SourceConsent {
            configuration: &input.configuration.0,
            policy_id: input.policy_id,
            revision: input.revision,
            source: input.source,
            source_hash: input.source_hash,
            ir_hash: input.ir_hash,
            module_digest: input.module_digest,
            language: input.language,
            tier: input.tier,
            rules: input.rules,
            params: &params,
            params_hash: input.params_hash,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn identifier(value: &str) -> bool {
        let mut bytes = value.bytes();
        value.len() <= 64
            && bytes.next().is_some_and(|b| b.is_ascii_lowercase())
            && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    }
    #[test]
    fn registrations_are_unique_and_namespaced() {
        assert!(!rails().is_empty(), "no registered rail");
        let mut ids = std::collections::BTreeSet::new();
        let mut networks = std::collections::BTreeSet::new();
        for rail in rails() {
            assert!(identifier(rail.id()) && rail.id() != "allowit", "{}", rail.id());
            assert!(ids.insert(rail.id()), "duplicate rail {}", rail.id());
            let mut names = std::collections::BTreeSet::new();
            for (name, _) in rail.policy_functions() {
                assert!(identifier(name), "{}::{name}", rail.id());
                assert!(names.insert(*name), "duplicate {}::{name}", rail.id());
                assert!(policy_function(rail.id(), name).is_some());
            }
            for network in rail.networks() {
                for name in std::iter::once(&network.id).chain(network.aliases) {
                    assert!(networks.insert(*name), "{name} registered twice");
                }
            }
        }
        assert!(policy_function("allowit", "price").is_none());
        assert_eq!(
            invoke_policy_function("unknown", "f", &serde_json::Value::Null, &serde_json::Value::Null)
                .unwrap_err()
                .code,
            2
        );
    }
}
