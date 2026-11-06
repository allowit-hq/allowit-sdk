//! Explicit policy capabilities. The host alone provides trusted facts and grants authority.

use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    error, fmt,
};

/// Arguments come from the policy guest. Facts come from the authenticated host workflow.
pub type Invoke = fn(args: &Value, facts: &Value) -> Result<Value, String>;
pub type Test = fn() -> Result<(), String>;

/// A vendor can export this shape without depending on the SDK that composes it.
pub type Bindings<const N: usize> = (&'static str, [(&'static str, Invoke); N], Test);

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Namespace,
    Operation,
    Duplicate,
    Unknown,
    Rejected(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Namespace => f.write_str("Invalid or reserved policy namespace"),
            Self::Operation => f.write_str("Invalid policy operation"),
            Self::Duplicate => f.write_str("Duplicate policy registration"),
            Self::Unknown => f.write_str("Unregistered policy operation"),
            Self::Rejected(detail) => f.write_str(detail),
        }
    }
}

impl error::Error for Error {}

struct Package {
    operations: BTreeMap<&'static str, Invoke>,
    test: Test,
}

/// The same immutable registration set supplies source admission, discovery and dispatch.
#[derive(Default)]
pub struct Registry {
    packages: BTreeMap<&'static str, Package>,
    rails: BTreeSet<&'static str>,
}

impl Registry {
    /// Reserve rail families from the host's compiled rail inventory.
    pub fn with_rails(rails: impl IntoIterator<Item = &'static str>) -> Result<Self, Error> {
        let rails: BTreeSet<_> = rails.into_iter().collect();
        if rails
            .iter()
            .any(|name| *name == "allowit" || !identifier(name))
        {
            return Err(Error::Namespace);
        }
        Ok(Self {
            packages: BTreeMap::new(),
            rails,
        })
    }

    /// Register a vendor package. Core and rail namespaces belong to the trusted host.
    pub fn register<const N: usize>(&mut self, bindings: Bindings<N>) -> Result<(), Error> {
        let (namespace, _, _) = &bindings;
        if *namespace == "allowit" || self.rails.contains(namespace) {
            return Err(Error::Namespace);
        }
        self.insert(bindings)
    }

    /// Register a rail supplied by the host's compiled-in rail composition.
    pub fn register_rail<const N: usize>(&mut self, bindings: Bindings<N>) -> Result<(), Error> {
        if !self.rails.contains(bindings.0) {
            return Err(Error::Namespace);
        }
        self.insert(bindings)
    }

    fn insert<const N: usize>(
        &mut self,
        (namespace, operations, test): Bindings<N>,
    ) -> Result<(), Error> {
        if !identifier(namespace) {
            return Err(Error::Namespace);
        }
        if self.packages.contains_key(namespace) {
            return Err(Error::Duplicate);
        }
        if operations.is_empty() || operations.iter().any(|(name, _)| !identifier(name)) {
            return Err(Error::Operation);
        }
        let operations: BTreeMap<_, _> = operations.into_iter().collect();
        if operations.len() != N {
            return Err(Error::Duplicate);
        }
        self.packages
            .insert(namespace, Package { operations, test });
        Ok(())
    }

    pub fn contains(&self, namespace: &str, name: &str) -> bool {
        self.packages
            .get(namespace)
            .is_some_and(|package| package.operations.contains_key(name))
    }

    pub fn operations(&self) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
        self.packages.iter().flat_map(|(&namespace, package)| {
            package
                .operations
                .keys()
                .map(move |&name| (namespace, name))
        })
    }

    pub fn invoke(
        &self,
        namespace: &str,
        name: &str,
        args: &Value,
        facts: &Value,
    ) -> Result<Value, Error> {
        let operation = self
            .packages
            .get(namespace)
            .and_then(|package| package.operations.get(name))
            .ok_or(Error::Unknown)?;
        operation(args, facts).map_err(Error::Rejected)
    }

    /// Run each package's self-test as part of the host's integration test suite.
    pub fn self_test(&self) -> Result<(), Error> {
        for (namespace, package) in &self.packages {
            (package.test)().map_err(|error| Error::Rejected(format!("{namespace}: {error}")))?;
        }
        Ok(())
    }
}

fn identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    value.len() <= 64
        && bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn echo(_args: &Value, facts: &Value) -> Result<Value, String> {
        Ok(facts.clone())
    }

    fn passes() -> Result<(), String> {
        Ok(())
    }

    #[test]
    fn discover_admit_dispatch_and_test_the_same_operations() {
        let mut registry = Registry::default();
        registry
            .register(("sample", [("echo", echo as Invoke)], passes))
            .unwrap();
        assert!(registry.contains("sample", "echo"));
        assert!(!registry.contains("sample", "other"));
        assert_eq!(
            registry.operations().collect::<Vec<_>>(),
            [("sample", "echo")]
        );
        assert_eq!(
            registry
                .invoke("sample", "echo", &json!("untrusted"), &json!("trusted"))
                .unwrap(),
            "trusted"
        );
        assert_eq!(
            registry.invoke("sample", "other", &Value::Null, &Value::Null),
            Err(Error::Unknown)
        );
        registry.self_test().unwrap();
    }

    #[test]
    fn reject_reserved_and_duplicate_registrations_without_partial_insert() {
        let mut registry = Registry::with_rails(["rail_a", "rail_b"]).unwrap();
        let op = [("echo", echo as Invoke)];
        for namespace in ["allowit", "rail_a", "rail_b", "x::y"] {
            assert_eq!(
                registry.register((namespace, op, passes)),
                Err(Error::Namespace)
            );
        }
        assert_eq!(
            registry.register(("sample", [op[0]; 2], passes)),
            Err(Error::Duplicate)
        );
        assert_eq!(registry.operations().count(), 0);
        registry.register_rail(("rail_a", op, passes)).unwrap();
        registry.register(("sample", op, passes)).unwrap();
        assert_eq!(
            registry.register(("sample", op, passes)),
            Err(Error::Duplicate)
        );
    }
}
