//! Rail plugin and policy package registry. Each vendor registers one [`RailPlugin`] or one
//! [`PolicyPackage`] in `registered.rs`; every dispatch below looks the registration up by
//! network or namespace, so no function in this module names a vendor.
mod registered;
#[cfg(feature = "testkit")]
pub mod testkit;

use crate::{
    error::{Error, Result},
    routing::{Family, RailInfo},
};
pub use allowit_core::policy_package::{PackageFeature, PackageSample, PolicyPackage};
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

/// Every registered policy package, in registration order.
pub fn packages() -> &'static [&'static PolicyPackage] {
    registered::PACKAGES
}

/// One namespace of evidence functions and the release that declares it: a rail plugin id or
/// a policy package's crate name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyNamespace {
    pub namespace: &'static str,
    /// The package's crate release. A rail's functions release with the rail crate, which the
    /// host pins by revision.
    pub release: Option<&'static str>,
}

/// The composed policy registry of this build: every evidence function that a server policy
/// may call. Hosts call this one function and name no vendor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyRegistry {
    /// Exact `(namespace, function)` host calls: rail evidence functions, then package
    /// functions, each in registration order.
    pub host_calls: Vec<(&'static str, &'static str)>,
    /// Each namespace with its release, for the build identity of a server policy.
    pub namespaces: Vec<PolicyNamespace>,
}

/// Compose and check the policy registry. A rail's namespace is its plugin id. A package's
/// namespace is its crate name; `allowit` and every rail id are reserved. `built_in` are the
/// host's own `allowit` server calls; the complete list must pass the policy-source gate, so a
/// registration that the gate cannot serve fails here and not in a later source check.
pub fn policy_registry(built_in: &[(&'static str, &'static str)]) -> Result<PolicyRegistry> {
    let unusable = |reason: String| Error::config(format!("policy registry: {reason}"));
    let rail_ids: Vec<&str> = rails().iter().map(|rail| rail.id()).collect();
    let mut host_calls = Vec::new();
    let mut namespaces = Vec::new();
    for rail in rails() {
        let functions = rail.policy_functions();
        if !functions.is_empty() {
            namespaces.push(PolicyNamespace {
                namespace: rail.id(),
                release: None,
            });
        }
        host_calls.extend(functions.iter().map(|(name, _)| (rail.id(), *name)));
    }
    host_calls
        .extend(allowit_core::policy_package::host_calls(packages(), &rail_ids).map_err(unusable)?);
    namespaces.extend(packages().iter().map(|package| PolicyNamespace {
        namespace: package.namespace,
        release: Some(package.release),
    }));
    let complete: Vec<_> = built_in.iter().chain(&host_calls).copied().collect();
    allowit_core::policy_source::check_host_call_registry(&complete).map_err(unusable)?;
    Ok(PolicyRegistry {
        host_calls,
        namespaces,
    })
}

/// The evidence function `namespace::name`: a rail plugin's function or a policy package's.
/// The `allowit` namespace belongs to Core and is never dispatched here.
pub fn policy_function(namespace: &str, name: &str) -> Option<PolicyFunction> {
    if let Ok(rail) = by_id(namespace) {
        return rail
            .policy_functions()
            .iter()
            .find(|(function, _)| *function == name)
            .map(|(_, f)| *f);
    }
    packages()
        .iter()
        .find(|package| package.namespace == namespace)?
        .function(name)
}

/// Invoke an evidence function with guest arguments and host-supplied facts.
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
            assert!(
                identifier(rail.id()) && rail.id() != "allowit",
                "{}",
                rail.id()
            );
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
        // The composed registry lists exactly the registered functions, and each dispatches.
        let registry = policy_registry(&[("allowit", "price")]).unwrap();
        let declared: usize = rails()
            .iter()
            .map(|rail| rail.policy_functions().len())
            .chain(packages().iter().map(|package| package.functions.len()))
            .sum();
        assert_eq!(registry.host_calls.len(), declared);
        for (namespace, name) in &registry.host_calls {
            assert!(
                policy_function(namespace, name).is_some(),
                "{namespace}::{name}"
            );
        }
        for package in packages() {
            assert!(!ids.contains(package.namespace), "{}", package.namespace);
            assert!(
                registry.namespaces.contains(&PolicyNamespace {
                    namespace: package.namespace,
                    release: Some(package.release)
                }),
                "{}",
                package.namespace
            );
        }
        // A host cannot register a built-in call that Core does not define.
        assert!(policy_registry(&[("allowit", "unknown")]).is_err());
        assert_eq!(
            invoke_policy_function(
                "unknown",
                "f",
                &serde_json::Value::Null,
                &serde_json::Value::Null
            )
            .unwrap_err()
            .code,
            2
        );
    }
}
