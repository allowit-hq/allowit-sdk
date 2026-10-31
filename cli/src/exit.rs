//! Process exit codes. Every command, including the paid-service commands,
//! uses this one table.
use crate::error::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum Exit {
    Ok = 0,
    Internal = 1,
    Usage = 2,
    /// Configuration, authentication, or not supported on this rail.
    Config = 3,
    /// Rejected by the contract, for example a paused wallet or a wrong asset.
    Rejected = 4,
    /// Uncertain or stale: retry with the same request ID.
    Uncertain = 5,
    Replayed = 6,
    OwnerSignature = 10,
    OwnerInput = 11,
    Pending = 12,
    Denied = 20,
    Interrupted = 130,
}

pub(crate) const TABLE: &[(Exit, &str)] = &[
    (Exit::Ok, "ok"),
    (Exit::Internal, "internal error"),
    (Exit::Usage, "usage error or invalid input"),
    (
        Exit::Config,
        "configuration, authentication or not supported on this rail",
    ),
    (
        Exit::Rejected,
        "rejected by the contract (paused, wrong asset)",
    ),
    (
        Exit::Uncertain,
        "uncertain or stale: retry with the same ID",
    ),
    (Exit::Replayed, "replayed"),
    (Exit::OwnerSignature, "owner signature needed"),
    (Exit::OwnerInput, "waiting for owner input"),
    (Exit::Pending, "pending"),
    (Exit::Denied, "denied by the policy"),
    (Exit::Interrupted, "interrupted"),
];

impl Exit {
    pub(crate) fn code(self) -> i32 {
        self as i32
    }
    /// The symbolic name used in JSON errors.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Exit::Ok => "ok",
            Exit::Internal => "internal",
            Exit::Usage => "usage",
            Exit::Config => "config",
            Exit::Rejected => "rejected",
            Exit::Uncertain => "uncertain",
            Exit::Replayed => "replayed",
            Exit::OwnerSignature => "owner_signature",
            Exit::OwnerInput => "owner_input",
            Exit::Pending => "pending",
            Exit::Denied => "denied",
            Exit::Interrupted => "interrupted",
        }
    }
    pub(crate) fn from_code(code: i32) -> Exit {
        TABLE
            .iter()
            .map(|(e, _)| *e)
            .find(|e| e.code() == code)
            .unwrap_or(Exit::Internal)
    }
}

/// The exit code of a contract rejection that an error message names: a
/// paused wallet, an unapproved vault or a wrong asset exits 4, a stale
/// request exits 5. The forms are the Soroban contract error of the Stellar
/// policy wallet and the custom program error of the Solana vault.
pub(crate) fn contract_exit(message: &str) -> Option<Exit> {
    // Stellar policy wallet: UnsupportedAsset 3, StaleRequest 5, Paused 6.
    const STELLAR: &[(u32, Exit)] = &[
        (3, Exit::Rejected),
        (5, Exit::Uncertain),
        (6, Exit::Rejected),
    ];
    // Solana vault: StaleRequest 101, NotApproved 104.
    const SOLANA: &[(u32, Exit)] = &[(101, Exit::Uncertain), (104, Exit::Rejected)];
    let compact: String = message.chars().filter(|c| !c.is_whitespace()).collect();
    STELLAR
        .iter()
        .find(|(n, _)| compact.contains(&format!("Error(Contract,#{n})")))
        .or_else(|| {
            SOLANA.iter().find(|(n, _)| {
                token(&compact, &format!("customprogramerror:{n:#x}"))
                    || compact.contains(&format!("\"Custom\":{n}}}"))
                    || compact.contains(&format!("Custom({n})"))
            })
        })
        .map(|(_, exit)| *exit)
}

/// True when `needle` occurs in `hay` and no letter or digit follows it.
fn token(hay: &str, needle: &str) -> bool {
    hay.match_indices(needle).any(|(at, _)| {
        !hay[at + needle.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
    })
}

/// The error for a verb whose backend is not ready on this rail.
pub(crate) fn not_available(verb: &str, rail: &str) -> Error {
    Error::unsupported(format!("{verb} is not available on {rail} yet"))
}

/// The error for a command that needs a rail when none is selected.
pub(crate) fn no_rail() -> Error {
    Error::config("no rail selected; run allowit rail use RAIL (for example stellar:testnet)")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn table_codes_are_unique_and_round_trip() {
        for (i, (exit, _)) in TABLE.iter().enumerate() {
            assert_eq!(Exit::from_code(exit.code()), *exit);
            assert!(TABLE[..i].iter().all(|(e, _)| e.code() != exit.code()));
        }
        assert_eq!(Exit::from_code(99), Exit::Internal);
        assert_eq!(not_available("wallet close", "stellar:testnet").code, 3);
        assert_eq!(no_rail().code, 3);
    }
    #[test]
    fn contract_rejections_map_to_the_table() {
        for (message, exit) in [
            ("HostError: Error(Contract, #6)", Some(Exit::Rejected)),
            (
                "simulation failed: Error(Contract, #5)",
                Some(Exit::Uncertain),
            ),
            ("Error(Contract, #3)", Some(Exit::Rejected)),
            ("Error(Contract, #13)", None),
            ("Error(Contract, #60)", None),
            ("custom program error: 0x68", Some(Exit::Rejected)),
            ("custom program error: 0x65", Some(Exit::Uncertain)),
            ("custom program error: 0x650", None),
            (
                r#"{"InstructionError":[0,{"Custom":104}]}"#,
                Some(Exit::Rejected),
            ),
            (r#"{"InstructionError":[0,{"Custom":1040}]}"#, None),
            ("InstructionError(0, Custom(101))", Some(Exit::Uncertain)),
            ("the vault is paused", None),
        ] {
            assert_eq!(contract_exit(message), exit, "{message}");
        }
    }
}
