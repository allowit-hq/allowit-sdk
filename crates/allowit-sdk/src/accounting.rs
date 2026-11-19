//! The counters that a server keeps for a wallet whose figures the wallet does not keep itself,
//! updated by Core's one definition of the standard post-flight transition.
use crate::error::{Error, Result};
use crate::runtime::Usage;
pub use allowit_core::AllowitStandardState as Counters;

/// The counters after one approved operation at `now` (Unix seconds) with `usage`, the rail's
/// usage of that operation: Core's standard post-flight (`allowit_core::post_flight`, Standard
/// tier), as a wallet and the server's policy evaluation count it. It rolls the UTC day over,
/// counts the call, adds the spent units to the day and the lifetime and the gross amount with
/// its gas to the day's volume, and marks an unpriced operation. Counters of a later day than
/// `now`'s, or that leave no room for another operation, are refused as Core refuses them.
pub fn post_flight(counters: &Counters, now: u64, usage: &Usage) -> Result<Counters> {
    let refused = |error: allowit_core::AllowitError| {
        Error::config(format!(
            "Core refused the counters' post-flight update: {error:?}"
        ))
    };
    let mut state = allowit_core::AllowitMap::new();
    counters.write(&mut state).map_err(refused)?;
    // Post-flight reads the state and the request's time, spent units, amount, gas and
    // unpriced mark; the other request facts are neutral.
    let none = allowit_core::Address::new(String::new());
    let context = allowit_core::AllowitContext {
        env: allowit_core::AllowitEnv::new(),
        params: allowit_core::AllowitParams::from(allowit_core::AllowitMap::new()),
        state,
        request: allowit_core::AllowitRequest {
            operation: allowit_core::AllowitOperation::transfer,
            asset: none.clone(),
            recipient: none.clone(),
            amount: usage.amount_units,
            token_in: none,
            amount_in: usage.amount_units,
            token_out: None,
            min_out: 0,
            now,
            expires_at: now,
            spent_units: usage.spent_units,
            gas_units: usage.gas_units,
            unpriced: usage.unpriced,
            args: allowit_core::AllowitArgs::from(allowit_core::AllowitMap::new()),
        },
    };
    let after = allowit_core::post_flight(&context, allowit_core::AllowitTier::Standard)
        .map_err(refused)?;
    Counters::read(&after).map_err(refused)
}

/// The server's account of a wallet after the settlement `s` of one of its operations, evaluated
/// at `evaluated_at` (Unix seconds) with the debit `debit` in asset units: what Core says the
/// operation adds (`Settlement::usage_for`) counts with Core's post-flight in the day of the
/// evaluation, the day whose limits admitted the operation, and the nonce is the settlement's when
/// it gives one. An operation that did not execute changes no figure. The server stores the gas
/// carry `s.gas_carry` beside the account.
pub fn apply_settlement(
    accounting: &crate::runtime::Accounting,
    evaluated_at: u64,
    debit: i128,
    s: &crate::runtime::Settlement,
) -> Result<crate::runtime::Accounting> {
    let mut a = accounting.clone();
    if let Some(usage) = s.usage_for(debit) {
        let counters = Counters {
            calls: a.calls,
            spent: a.spent_total,
            unpriced: a.unpriced_total,
            day: a.spent_day,
            day_spent: a.spent_daily,
            day_total: a.volume_daily,
            day_unpriced: a.unpriced_daily,
        };
        let after = post_flight(&counters, evaluated_at, &usage)?;
        a.calls = after.calls;
        a.spent_total = after.spent;
        a.unpriced_total = after.unpriced;
        a.spent_day = after.day;
        a.spent_daily = after.day_spent;
        a.volume_daily = after.day_total;
        a.unpriced_daily = after.day_unpriced;
    }
    if let Some(nonce) = s.next_nonce {
        a.nonce = nonce;
    }
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    const DAY: u64 = 86_400;
    fn usage(amount: i128, gas: i128, spent: i128, unpriced: bool) -> Usage {
        Usage {
            amount_units: amount,
            gas_units: gas,
            spent_units: spent,
            unpriced,
        }
    }
    /// One operation counts one call, its spent units in the day and in the lifetime, its gross
    /// amount with its gas in the day's volume, and an unpriced mark; the next UTC day starts its
    /// daily figures at zero.
    #[test]
    fn an_operation_counts_as_core_counts_it() {
        let start = Counters::default();
        let one = post_flight(&start, 3 * DAY + 10, &usage(2_000, 136, 2_136, false)).unwrap();
        assert_eq!(
            (one.calls, one.day, one.spent, one.day_spent, one.day_total),
            (1, 3, 2_136, 2_136, 2_136)
        );
        assert!(!one.unpriced && !one.day_unpriced);
        let two = post_flight(&one, 3 * DAY + 20, &usage(500, 15, 515, true)).unwrap();
        assert_eq!(
            (two.calls, two.spent, two.day_spent, two.day_total),
            (2, 2_651, 2_651, 2_651)
        );
        assert!(two.unpriced && two.day_unpriced);
        // The next day: the daily figures roll over, the lifetime ones stay.
        let three = post_flight(&two, 4 * DAY, &usage(100, 0, 100, false)).unwrap();
        assert_eq!(
            (
                three.calls,
                three.day,
                three.spent,
                three.day_spent,
                three.day_total
            ),
            (3, 4, 2_751, 100, 100)
        );
        assert!(three.unpriced && !three.day_unpriced);
    }
    fn settlement(
        outcome: crate::runtime::OperationOutcome,
        usage: Option<Usage>,
        next_nonce: Option<u64>,
    ) -> crate::runtime::Settlement {
        crate::runtime::Settlement {
            transaction: vec![7; 32],
            outcome,
            usage,
            fee_units: 0,
            counted_gas: None,
            next_nonce,
            gas_carry: 0,
        }
    }
    /// An executed operation counts the rail's usage in the day of its evaluation; one whose
    /// rail reports none counts its debit; the nonce is the settlement's.
    #[test]
    fn a_settlement_counts_the_usage_or_the_debit_and_takes_the_nonce() {
        use crate::runtime::OperationOutcome::*;
        let start = crate::runtime::Accounting {
            nonce: 4,
            ..Default::default()
        };
        let counted = apply_settlement(
            &start,
            3 * DAY + 10,
            999,
            &settlement(Executed, Some(usage(2_000, 136, 2_136, false)), None),
        )
        .unwrap();
        assert_eq!(
            (
                counted.calls,
                counted.spent_day,
                counted.spent_total,
                counted.spent_daily,
                counted.volume_daily,
                counted.nonce
            ),
            (1, 3, 2_136, 2_136, 2_136, 4)
        );
        let keypair =
            apply_settlement(&start, 3 * DAY, 500, &settlement(Executed, None, Some(5))).unwrap();
        assert_eq!(
            (keypair.spent_total, keypair.volume_daily, keypair.nonce),
            (500, 0, 5)
        );
        // An operation that did not execute changes no figure.
        for outcome in [Failed, Open] {
            let same = apply_settlement(
                &start,
                3 * DAY,
                500,
                &settlement(outcome, Some(usage(1, 1, 1, true)), Some(4)),
            )
            .unwrap();
            assert_eq!(same, start);
        }
    }
    /// Counters of a later day than the operation's are refused, as Core refuses them: the
    /// figures never move back a day.
    #[test]
    fn counters_of_a_later_day_are_refused() {
        let today = post_flight(&Counters::default(), 5 * DAY, &usage(1, 0, 1, false)).unwrap();
        assert!(post_flight(&today, 4 * DAY, &usage(1, 0, 1, false)).is_err());
        // Spending that leaves no room for another operation is refused too.
        let full = Counters {
            spent: i128::MAX - 1,
            day: 5,
            ..Counters::default()
        };
        assert!(post_flight(&full, 5 * DAY, &usage(1, 0, 1, false)).is_err());
    }
}
