//! Verified public installations. Keys and retained signing records use separate stores.
use crate::{
    config::parse_origin,
    error::{Error, Result},
    routing, vendors,
};
use allowit_core::{client::store::Store, policy::PolicyValue};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Description {
    pub network: String,
    pub wallet: String,
    pub agent: String,
    pub policy_id: String,
    pub owner: String,
    pub server: String,
    pub asset: String,
    pub asset_symbol: String,
    pub source: String,
    pub source_hash: String,
    pub module_digest: String,
    pub params: BTreeMap<String, PolicyValue>,
}
/// Select one exact public installation. Multiple versions require explicit selection.
#[derive(Clone, Debug, Default)]
pub struct Selector {
    pub network: String,
    pub wallet: Option<String>,
    pub agent: Option<String>,
    pub policy_id: Option<String>,
    pub version: Option<String>,
}
pub struct Profile {
    description: Description,
    pub version: String,
    #[cfg_attr(not(feature = "stellar"), allow(dead_code))]
    pub(crate) backend: String,
    #[cfg_attr(not(feature = "stellar"), allow(dead_code))]
    pub(crate) binding: vendors::ProfileBinding,
    descriptor: Box<RawValue>,
    authority: Option<crate::config::ServerAuthority>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    schema: u8,
    network: String,
    backend: String,
    descriptor: Box<RawValue>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    version: String,
}
pub struct Cache {
    versions: Store,
    selections: Store,
}
fn canonical(value: &impl Serialize) -> Result<Vec<u8>> {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| serde_json_canonicalizer::to_vec(&v).ok())
        .ok_or(())
        .map_err(|_| Error::config("Cannot encode public profile"))
}
impl Profile {
    pub fn with_authority(mut self, authority: &crate::config::ServerAuthority) -> Result<Self> {
        if self.description.network != authority.network() || self.backend != authority.origin() {
            return Err(Error::config(
                "Configured server authority belongs to another backend or rail",
            ));
        }
        vendors::verify_server_role(&self.binding, authority.public_key())?;
        self.authority = Some(authority.clone());
        Ok(self)
    }
    pub fn server_origin(&self) -> &str {
        &self.backend
    }
    pub(crate) fn require_authority(&self) -> Result<&crate::config::ServerAuthority> {
        self.authority.as_ref().ok_or_else(|| {
            Error::config("Fresh execution requires independently configured server authority")
        })
    }
    pub fn description(&self) -> &Description {
        &self.description
    }

    fn decode(envelope: Envelope) -> Result<Self> {
        if envelope.schema != 1
            || routing::normalize(&envelope.network)? != envelope.network
            || parse_origin(&envelope.backend)? != envelope.backend
        {
            return Err(Error::config("Invalid public profile envelope"));
        }
        // Decode the strict native DTO before canonicalization can collapse duplicate fields.
        let (description, binding) =
            vendors::decode_profile(&envelope.network, envelope.descriptor.get().as_bytes())?;
        let version = allowit_core::digest(&canonical(&envelope)?);
        Ok(Self {
            description,
            binding,
            version,
            backend: envelope.backend,
            descriptor: envelope.descriptor,
            authority: None,
        })
    }
    pub fn document(&self) -> Result<serde_json::Value> {
        serde_json::from_str(self.descriptor.get())
            .map_err(|_| Error::config("Cannot encode public profile"))
    }
    pub fn inspect(&self) -> Result<serde_json::Value> {
        let mut value = serde_json::to_value(&self.description)
            .map_err(|_| Error::config("Cannot encode profile description"))?;
        value
            .as_object_mut()
            .expect("description object")
            .remove("source");
        value["rail"] = self.description.network.clone().into();
        value["version"] = self.version.clone().into();
        Ok(value)
    }
    #[cfg_attr(not(feature = "stellar"), allow(dead_code))]
    fn envelope(&self) -> Result<Envelope> {
        Ok(Envelope {
            schema: 1,
            network: self.description.network.clone(),
            backend: self.backend.clone(),
            descriptor: RawValue::from_string(self.descriptor.get().into())
                .map_err(|_| Error::config("Invalid public descriptor"))?,
        })
    }
}
impl Cache {
    pub fn open(root: &Path) -> Result<Self> {
        Ok(Self {
            versions: Store::open(&root.join("public-profiles/versions"))?,
            selections: Store::open(&root.join("public-profiles/selections"))?,
        })
    }
    /// Install only a descriptor returned by the checked owner export capability.
    #[cfg_attr(not(feature = "stellar"), allow(dead_code))]
    pub(crate) fn install(
        &self,
        network: &str,
        backend: &str,
        descriptor: Box<RawValue>,
    ) -> Result<Profile> {
        let profile = Profile::decode(Envelope {
            schema: 1,
            network: routing::normalize(network)?.into(),
            backend: parse_origin(backend)?,
            descriptor,
        })?;
        let envelope = profile.envelope()?;
        if let Some(old) = self.versions.load::<Envelope>(&profile.version)? {
            if canonical(&old)? != canonical(&envelope)? {
                return Err(Error::config(
                    "Public profile version already binds another descriptor",
                ));
            }
        } else {
            self.versions.save(&profile.version, &envelope)?;
        }
        Ok(profile)
    }
    /// Import a strict public export after native source and current authority checks.
    pub fn import(
        &self,
        authority: &crate::config::ServerAuthority,
        endpoint: &str,
        bytes: &[u8],
    ) -> Result<Profile> {
        let network = authority.network();
        let (_, binding) = vendors::decode_profile(network, bytes)?;
        vendors::verify_server_role(&binding, authority.public_key())?;
        vendors::verify_profile_import(&binding, endpoint)?;
        let raw = RawValue::from_string(
            std::str::from_utf8(bytes)
                .map_err(|_| Error::usage("Public profile must use UTF-8"))?
                .into(),
        )
        .map_err(|_| Error::usage("Invalid public profile JSON"))?;
        self.install(network, authority.origin(), raw)?
            .with_authority(authority)
    }
    pub fn resolve(&self, selector: &Selector) -> Result<Profile> {
        let network = routing::normalize(&selector.network)?;
        let mut found = Vec::new();
        for envelope in self.versions.entries::<Envelope>()? {
            let p = Profile::decode(envelope)?;
            let d = &p.description;
            if d.network == network
                && selector.wallet.as_ref().is_none_or(|v| v == &d.wallet)
                && selector.agent.as_ref().is_none_or(|v| v == &d.agent)
                && selector
                    .policy_id
                    .as_ref()
                    .is_none_or(|v| v == &d.policy_id)
                && selector.version.as_ref().is_none_or(|v| v == &p.version)
            {
                found.push(p);
            }
        }
        if found.len() > 1 && selector.version.is_none() {
            let key = selection_key(selector)?;
            if let Some(selected) = self.selections.load::<Selection>(&key)? {
                found.retain(|p| p.version == selected.version);
            }
        }
        match found.len() {
            1 => Ok(found.pop().expect("one profile")),
            0 => Err(Error::config(
                "No verified public profile matches this selection. Complete policy activation and export for this wallet and agent.",
            )),
            _ => Err(Error::config(
                "Several public profiles match. Select an exact policy and profile version.",
            )),
        }
    }
    pub fn select(&self, selector: &Selector) -> Result<Profile> {
        if selector.version.is_none() {
            return Err(Error::usage("Select an explicit public profile version"));
        }
        let profile = self.resolve(selector)?;
        let mut selection = selector.clone();
        selection.version = None;
        self.selections.save(
            &selection_key(&selection)?,
            &Selection {
                version: profile.version.clone(),
            },
        )?;
        Ok(profile)
    }
}
fn selection_key(s: &Selector) -> Result<String> {
    Ok(allowit_core::digest(&canonical(&(
        routing::normalize(&s.network)?,
        &s.wallet,
        &s.agent,
        &s.policy_id,
    ))?))
}
