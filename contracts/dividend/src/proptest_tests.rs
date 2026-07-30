#![cfg(test)]
//! Property tests for the proportional dividend math (issue #109).
//!
//! The floored share formula (`total_amount * balance / supply`) must never
//! overflow for bounded inputs, and the sum of every holder's floored share
//! must never exceed the distribution's `total_amount` — the invariant
//! `claim()` relies on via its `assert!(dist.distributed <= dist.total_amount)`.

use super::proportional_share;
use proptest::prelude::*;

// Bound inputs to i64::MAX so total_amount * balance (product of two such
// bounds, ~2^126) stays within i128::MAX (~2^127), matching the "no overflow
// for bounded inputs" requirement from the issue.
const MAX_BOUND: i128 = i64::MAX as i128;

proptest! {
    /// A single holder's floored share is never negative and never exceeds
    /// total_amount.
    #[test]
    fn share_is_bounded_by_total(
        total_amount in 1i128..=MAX_BOUND,
        supply in 1i128..=MAX_BOUND,
        raw_balance in 0i128..=MAX_BOUND,
    ) {
        let balance = raw_balance.min(supply);
        let share = proportional_share(total_amount, balance, supply);
        prop_assert!(share >= 0);
        prop_assert!(share <= total_amount);
    }

    /// sum(shares) over a set of holders whose balances sum to <= supply
    /// never exceeds total_amount, i.e. floor-division dust is never
    /// double-paid across holders.
    #[test]
    fn sum_of_shares_never_exceeds_total(
        total_amount in 1i128..=MAX_BOUND,
        supply in 1i128..=MAX_BOUND,
        raw_balances in prop::collection::vec(0i128..=MAX_BOUND, 1..16),
    ) {
        // Scale the (possibly supply-exceeding) random balances down so
        // their sum never exceeds supply, mirroring a real holder set where
        // sum(balances) <= total_supply always holds.
        let raw_sum: i128 = raw_balances.iter().sum();
        let balances: Vec<i128> = if raw_sum > supply && raw_sum > 0 {
            raw_balances
                .iter()
                .map(|b| (*b * supply) / raw_sum)
                .collect()
        } else {
            raw_balances
        };
        prop_assert!(balances.iter().sum::<i128>() <= supply);

        let sum_shares: i128 = balances
            .iter()
            .map(|&b| proportional_share(total_amount, b, supply))
            .sum();

        prop_assert!(sum_shares <= total_amount);
    }

    /// Zero balance always yields a zero share, regardless of the other
    /// bounded inputs.
    #[test]
    fn zero_balance_is_zero_share(
        total_amount in 1i128..=MAX_BOUND,
        supply in 1i128..=MAX_BOUND,
    ) {
        prop_assert_eq!(proportional_share(total_amount, 0, supply), 0);
    }

    /// A holder owning the entire supply claims the entire distribution
    /// (no dust lost when there is only one holder).
    #[test]
    fn full_supply_claims_full_amount(
        total_amount in 1i128..=MAX_BOUND,
        supply in 1i128..=MAX_BOUND,
    ) {
        prop_assert_eq!(proportional_share(total_amount, supply, supply), total_amount);
    }
}
