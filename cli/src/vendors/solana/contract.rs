//! Solana vault program errors in the exit table: an unapproved vault exits
//! 4 and a stale request exits 5.
use crate::{
    error::{Error, Result},
    exit::Exit,
};

/// Vault custom errors: StaleRequest 101, NotApproved 104.
const ERRORS: &[(u32, Exit)] = &[(101, Exit::Uncertain), (104, Exit::Rejected)];

/// The exit code of a vault custom error that a message names.
pub(crate) fn exit(message: &str) -> Option<Exit> {
    let compact: String = message.chars().filter(|c| !c.is_whitespace()).collect();
    ERRORS
        .iter()
        .find(|(n, _)| {
            token(&compact, &format!("customprogramerror:{n:#x}"))
                || compact.contains(&format!("\"Custom\":{n}}}"))
                || compact.contains(&format!("Custom({n})"))
        })
        .map(|(_, exit)| *exit)
}

/// Gives a failed Solana operation the exit code of the vault error that
/// its message names. A usage error keeps its code.
pub(crate) fn mapped(result: Result<i32>) -> Result<i32> {
    result.map_err(|mut e: Error| {
        if e.code != Exit::Usage.code()
            && let Some(exit) = exit(&e.message)
        {
            e.code = exit.code();
        }
        e
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vault_errors_map_to_the_table() {
        for (message, exit) in [
            ("custom program error: 0x68", Some(Exit::Rejected)),
            ("custom program error: 0x65", Some(Exit::Uncertain)),
            ("custom program error: 0x650", None),
            (
                r#"{"InstructionError":[0,{"Custom":104}]}"#,
                Some(Exit::Rejected),
            ),
            (r#"{"InstructionError":[0,{"Custom":1040}]}"#, None),
            ("InstructionError(0, Custom(101))", Some(Exit::Uncertain)),
            // A Soroban contract error is not a Solana vault error.
            ("HostError: Error(Contract, #6)", None),
            ("the vault is paused", None),
        ] {
            assert_eq!(super::exit(message), exit, "{message}");
        }
    }
    #[test]
    fn a_failed_operation_takes_the_vault_exit_and_usage_keeps_its_code() {
        let failed = |e: Error| mapped(Err(e)).unwrap_err().code;
        assert_eq!(
            failed(Error::config("send failed: custom program error: 0x68")),
            4
        );
        assert_eq!(
            failed(Error::usage("custom program error: 0x68 in the input")),
            2
        );
        assert_eq!(failed(Error::config("RPC unavailable")), 3);
        assert_eq!(mapped(Ok(10)).unwrap(), 10);
    }
}
