//! Vendor-neutral SDK cases. Every case runs against every compiled vendor through the public SDK
//! API. A case that needs a feature which a vendor does not declare in its capabilities is
//! skipped for that vendor, with the reason. Vendor protocol tests stay in the vendor repositories.
use allowit_sdk::{
    execution::{self, Capabilities},
    input::{
        address,
        asset::{self, Use},
    },
    routing::{self, RailInfo},
    wallet,
};

/// Why a case does not apply to a vendor.
struct Skip(String);
type Outcome = Result<(), Skip>;

struct Vendor {
    rail: &'static RailInfo,
    capabilities: Capabilities,
}

impl Vendor {
    fn operation(&self, op: &str) -> Outcome {
        if self.capabilities.operations.contains(&op) {
            Ok(())
        } else {
            Err(Skip(format!("the vendor declares no {op:?} operation")))
        }
    }
}

/// One vendor-neutral case: it passes, panics, or names why it does not apply.
type Case = fn(&Vendor) -> Outcome;

const CASES: &[(&str, Case)] = &[
    ("network names", network_names),
    ("local keys", local_keys),
    ("asset amounts", asset_amounts),
    ("native amounts", native_amounts),
    ("transfer requests", transfer_requests),
    ("undeclared operations", undeclared_operations),
];

#[test]
fn every_vendor_runs_every_case_it_supports() {
    #[cfg(feature = "default")]
    assert!(
        !routing::all().is_empty(),
        "the default build compiles at least one vendor"
    );
    for rail in routing::all() {
        let vendor = Vendor {
            rail,
            capabilities: execution::capabilities(rail.id).unwrap(),
        };
        for (name, case) in CASES {
            match case(&vendor) {
                Ok(()) => eprintln!("pass {} {name}", rail.id),
                Err(Skip(reason)) => eprintln!("skip {} {name}: {reason}", rail.id),
            }
        }
    }
}

/// A new key file under `dir`. Private storage creates its own 0700 directory and refuses a path
/// through a symbolic link.
fn key_path(dir: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    dir.path().canonicalize().unwrap().join("keys").join(name)
}

/// The canonical identifier and every alias name the same network.
fn network_names(v: &Vendor) -> Outcome {
    assert_eq!(routing::normalize(v.rail.id).unwrap(), v.rail.id);
    for alias in v.rail.aliases {
        assert_eq!(routing::normalize(alias).unwrap(), v.rail.id, "{alias}");
        assert_eq!(routing::family(alias).unwrap(), v.rail.family, "{alias}");
    }
    Ok(())
}

/// A generated agent key reopens with the same identity, and its address is a valid account.
fn local_keys(v: &Vendor) -> Outcome {
    let dir = tempfile::tempdir().unwrap();
    let path = key_path(&dir, "agent.key");
    let generated = wallet::generate_wallet(v.rail.id, &path).unwrap();
    generated.validate().unwrap();
    assert_eq!(generated.network, v.rail.id);
    assert_eq!(wallet::inspect_wallet(v.rail.id, &path).unwrap(), generated);
    let signer = wallet::local_signer(v.rail.id, &path).unwrap();
    assert_eq!(signer.address(), generated.address);
    assert!(!signer.sign_hash(&[7; 32]).unwrap().is_empty());
    address::check_account(v.rail.family, &generated.address).unwrap();
    assert!(wallet::generate_wallet(v.rail.id, &path).is_err());
    Ok(())
}

/// Every token in the vendor's table parses by its code and resolves to an address.
fn asset_amounts(v: &Vendor) -> Outcome {
    let tokens = execution::tokens(v.rail.id).unwrap();
    let named: Vec<_> = tokens.iter().filter(|token| !token.native).collect();
    if named.is_empty() {
        return Err(Skip(
            "the vendor declares no token besides its native asset".into(),
        ));
    }
    for token in named {
        let amount = asset::parse(
            v.rail.family,
            tokens,
            &format!("{}:1", token.code),
            Use::Spend,
        )
        .unwrap();
        assert_eq!(amount.decimals, Some(token.decimals), "{}", token.code);
        assert_eq!(amount.units(), 10u128.pow(token.decimals.into()));
        let resolved = asset::resolve(v.rail.id, token.code, token.issuer, true).unwrap();
        assert_eq!(resolved.symbol, token.code);
        assert!(!resolved.address.is_empty());
    }
    assert!(asset::parse(v.rail.family, tokens, "UNKNOWNCODE:1", Use::Spend).is_err());
    assert!(asset::parse(v.rail.family, tokens, "0", Use::Spend).is_err());
    Ok(())
}

/// `native` names the vendor's native asset.
fn native_amounts(v: &Vendor) -> Outcome {
    let tokens = execution::tokens(v.rail.id).unwrap();
    let native = tokens
        .iter()
        .find(|token| token.native)
        .ok_or_else(|| Skip("the vendor declares no native asset".into()))?;
    let amount = asset::parse(v.rail.family, tokens, "native:2", Use::Spend).unwrap();
    assert_eq!(
        amount.asset,
        asset::Asset::Native {
            code: native.code.into()
        }
    );
    assert_eq!(amount.units(), 2 * 10u128.pow(native.decimals.into()));
    Ok(())
}

/// A transfer to a valid account passes the vendor's request check.
fn transfer_requests(v: &Vendor) -> Outcome {
    v.operation("transfer")?;
    if !v.capabilities.address_is_key {
        return Err(Skip("the vendor's account addresses are not keys".into()));
    }
    let dir = tempfile::tempdir().unwrap();
    let recipient = wallet::generate_wallet(v.rail.id, &key_path(&dir, "recipient.key"))
        .unwrap()
        .address;
    address::check_recipient(v.rail.family, &recipient).unwrap();
    execution::check(v.rail.id, "transfer", &recipient, None, false).unwrap();
    Ok(())
}

/// The vendor refuses an operation that its capabilities do not declare.
fn undeclared_operations(v: &Vendor) -> Outcome {
    let op = "undeclared-conformance-operation";
    assert!(v.operation(op).is_err());
    assert!(execution::check(v.rail.id, op, "", None, false).is_err());
    Ok(())
}
