//! Builds the typed transfer request from the `exec` flags and the installed
//! wallet profile. The policy wallet admits only a transfer of its own asset:
//! XLM moves as a native transfer, an issued asset as a SEP-41 `transfer`.
use crate::{
    error::{Error, Result},
    typed_request::operation_nonce,
    vendors::{
        asset::{Amount, Asset},
        typed::TypedRequest,
    },
};
use allowit_sdk::typed_workflow::{
    AbiValue, AccountId, AccountKind, AssetId, Chain, ExecutionInput, ExecutionRequest,
    NativeInstruction, OperationRef, WorkflowAsset, WorkflowEffectBound, WorkflowFeeBound,
};
use allowit_stellar_wallet::{self as sdk, WalletConfig, WalletRoute};

/// A G account or a C contract as a workflow account.
fn account(address: &str) -> Result<AccountId> {
    let invalid = || Error::usage(format!("{address} is not a canonical Stellar address"));
    let parsed = sdk::address(address).map_err(|_| invalid())?;
    if sdk::address_text(&parsed).map_err(|_| invalid())? != address {
        return Err(invalid());
    }
    let (kind, key) = if address.starts_with('G') {
        (
            AccountKind::Wallet,
            sdk::account_key(address).map_err(|_| invalid())?,
        )
    } else {
        (AccountKind::Contract, contract_key(address)?)
    };
    Ok(AccountId {
        chain: Chain::Stellar,
        kind,
        address: key,
    })
}

/// The 32-byte contract ID of a C strkey.
fn contract_key(address: &str) -> Result<[u8; 32]> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut bits = 0u32;
    let mut count = 0;
    let mut bytes = vec![];
    for c in address.bytes() {
        let value = ALPHABET
            .iter()
            .position(|a| *a == c)
            .ok_or_else(|| Error::usage("invalid contract strkey"))? as u32;
        bits = (bits << 5) | value;
        count += 5;
        if count >= 8 {
            count -= 8;
            bytes.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    // Version byte, 32 key bytes and a two-byte checksum; sdk::address has
    // already checked the checksum.
    if bytes.len() != 35 || bytes[0] != 2 << 3 {
        return Err(Error::usage("invalid contract strkey"));
    }
    Ok(bytes[1..33].try_into().expect("32 bytes"))
}

/// Checks that the amount names the wallet's installed asset and returns its
/// base units.
pub(crate) fn installed_units(config: &WalletConfig, amount: &Amount) -> Result<u64> {
    let matches = match &amount.asset {
        Asset::Installed => true,
        Asset::Native { .. } => config.asset_symbol == "XLM" && config.asset_issuer.is_none(),
        Asset::Token { code, issuer } => {
            *code == config.asset_symbol
                && issuer
                    .as_deref()
                    .is_none_or(|i| Some(i) == config.asset_issuer.as_deref())
        }
        Asset::Contract(contract) => *contract == config.asset,
    };
    if !matches {
        return Err(Error {
            code: 4,
            message: format!(
                "the wallet holds only its installed asset {}; {} names another asset",
                config.asset_symbol, amount.text
            ),
            status: None,
            config: false,
        });
    }
    let units = amount
        .units()
        .ok_or_else(|| Error::usage("a transfer needs an exact amount"))?;
    u64::try_from(units).map_err(|_| Error::usage("the amount is too large"))
}

/// The typed transfer request for one recipient and amount.
pub(crate) fn transfer(
    policy_id: &str,
    request_id: &str,
    config: &WalletConfig,
    to: &str,
    units: u64,
) -> Result<TypedRequest> {
    let network = sdk::network_id(&config.network).map_err(Error::config)?;
    let wallet = account(&config.wallet)?;
    let recipient = account(to)?;
    let native = WorkflowAsset::Native {
        chain: Chain::Stellar,
        network,
    };
    let xlm = config.asset_symbol == "XLM" && config.asset_issuer.is_none();
    let (instruction, asset) = if xlm {
        (
            NativeInstruction::StellarNativeTransfer {
                source: wallet.clone(),
                destination: recipient.clone(),
                stroops: units,
            },
            native.clone(),
        )
    } else {
        let token = account(&config.asset)?;
        (
            NativeInstruction::StellarCall {
                contract: token.clone(),
                schema_digest: sdk::hash(sdk::ARGUMENT_SCHEMA),
                function: "transfer".into(),
                arguments: vec![
                    AbiValue::Account(wallet.clone()),
                    AbiValue::Account(recipient.clone()),
                    AbiValue::Signed128(i128::from(units)),
                ],
            },
            WorkflowAsset::Token(AssetId { network, token }),
        )
    };
    // The classic route pays fees from the wallet account; the policy wallet
    // route pays them from the agent account.
    let payer = if config.route == WalletRoute::Native {
        wallet.clone()
    } else {
        account(&config.agent)?
    };
    let request = ExecutionRequest {
        operation: OperationRef {
            policy_instance: config.policy_instance,
            nonce: operation_nonce(policy_id, request_id),
        },
        input: ExecutionInput::Instructions(vec![instruction]),
        effect_bounds: vec![WorkflowEffectBound {
            asset,
            source: wallet,
            beneficiary: recipient,
            max_debit: units.into(),
            min_credit: units.into(),
            max_burn: 0u64.into(),
        }],
        fee_bounds: Some(vec![WorkflowFeeBound {
            payer,
            asset: native,
            max_fee: u64::from(config.max_fee_stroops).into(),
        }]),
        evidence: vec![],
    };
    // The wallet library decodes exactly what the engine will check.
    sdk::transfer_from_request(config, &super::run::convert(&request)?)
        .map_err(|e| Error::usage(format!("the transfer does not fit this wallet: {e}")))?;
    TypedRequest::new(policy_id, request_id, request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vendors::asset::{Quantity, Use};

    fn config(route: WalletRoute, usdc: bool) -> WalletConfig {
        let owner = sdk::account_for_seed(&[1; 32]);
        let source = "use allowit::v1::prelude::*; async fn _execute(ctx: &Context) -> PolicyResult { Ok(()) }".to_string();
        let mut c = WalletConfig {
            network: "stellar:testnet".into(),
            policy_instance: [7; 32],
            wallet: owner.clone(),
            owner,
            agent: sdk::account_for_seed(&[2; 32]),
            server: sdk::account_for_seed(&[3; 32]),
            route: route.clone(),
            policy_engine: (route == WalletRoute::PolicyWallet)
                .then_some(sdk::PolicyEngine::SimplePolicyEngine),
            asset: String::new(),
            asset_symbol: if usdc { "USDC" } else { "XLM" }.into(),
            asset_issuer: usdc.then(|| sdk::account_for_seed(&[9; 32])),
            source_hash: sdk::hash(&source),
            policy_source: source,
            module_digest: [5; 32],
            max_fee_stroops: 50_000,
        };
        if route == WalletRoute::PolicyWallet {
            let native = sdk::asset_from_identity("XLM", None).unwrap();
            // Any valid contract strkey serves as the wallet contract.
            c.wallet = sdk::sac_address("stellar:mainnet", &native).unwrap();
        }
        c.asset = sdk::sac_address(&c.network, &sdk::asset_xdr(&c).unwrap()).unwrap();
        c
    }

    #[test]
    fn native_and_sep41_transfers_decode_in_the_wallet_library() {
        let to = sdk::account_for_seed(&[6; 32]);
        for (route, usdc) in [
            (WalletRoute::Native, false),
            (WalletRoute::PolicyWallet, false),
            (WalletRoute::PolicyWallet, true),
        ] {
            let c = config(route.clone(), usdc);
            let typed = transfer("policy-1", "request-0001", &c, &to, 1_000_000).unwrap();
            let t = sdk::transfer_from_request(
                &c,
                &super::super::run::convert(&typed.request).unwrap(),
            )
            .unwrap();
            assert_eq!(t.amount, 1_000_000);
            assert_eq!(t.recipient, to);
            assert_eq!(t.native, !usdc, "{route:?}");
            assert_eq!(
                typed.request.operation.nonce,
                operation_nonce("policy-1", "request-0001")
            );
        }
    }

    #[test]
    fn contract_strkeys_decode_to_their_contract_id() {
        let native = sdk::asset_from_identity("XLM", None).unwrap();
        let sac = sdk::sac_address("stellar:testnet", &native).unwrap();
        let id = contract_key(&sac).unwrap();
        let account = account(&sac).unwrap();
        assert_eq!(account.kind, AccountKind::Contract);
        assert_eq!(account.address, id);
    }

    #[test]
    fn only_the_installed_asset_can_move() {
        let c = config(WalletRoute::Native, false);
        let tokens = super::super::tokens_for("stellar:testnet");
        let xlm = crate::vendors::asset::parse(
            crate::routing::Family::Stellar,
            tokens,
            "XLM:5",
            Use::Spend,
        )
        .unwrap();
        assert_eq!(installed_units(&c, &xlm).unwrap(), 50_000_000);
        let usdc = crate::vendors::asset::parse(
            crate::routing::Family::Stellar,
            tokens,
            "USDC:1",
            Use::Spend,
        )
        .unwrap();
        assert_eq!(installed_units(&c, &usdc).unwrap_err().code, 4);
        let max = Amount {
            asset: Asset::Installed,
            quantity: Quantity::Max,
            decimals: None,
            text: "XLM:max".into(),
        };
        assert_eq!(installed_units(&c, &max).unwrap_err().code, 2);
    }
}
