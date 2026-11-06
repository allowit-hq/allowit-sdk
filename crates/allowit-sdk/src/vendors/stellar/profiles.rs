//! Strict native descriptor decoding and common-value projection.
use crate::{
    error::{Error, Result},
    profiles::Description,
    vendors::ProfileBinding,
};
use allowit_core::policy::PolicyValue;
pub(in crate::vendors) fn decode(
    network: &str,
    bytes: &[u8],
) -> Result<(Description, ProfileBinding)> {
    let bundle = crate::vendors::stellar::runtime::client::decode_public_bundle(bytes)?;
    if crate::routing::normalize(&bundle.network)? != network {
        return Err(Error::config(
            "Public profile belongs to another selected rail",
        ));
    }
    let c = &bundle.config;
    let d = Description {
        network: network.into(),
        wallet: bundle.wallet.clone(),
        agent: c.agent.clone(),
        policy_id: bundle.policy_id.clone(),
        owner: c.owner.clone(),
        server: c.server.clone(),
        asset: c.asset.clone(),
        asset_symbol: c.asset_symbol.clone(),
        source: c.policy_source.clone(),
        source_hash: allowit_stellar_wallet::hex(&c.source_hash),
        module_digest: bundle.module_digest.clone(),
        params: c
            .params
            .0
            .iter()
            .map(|(k, v)| (k.clone(), PolicyValue(v.allowit())))
            .collect(),
    };
    Ok((d, ProfileBinding::Stellar(bundle)))
}
