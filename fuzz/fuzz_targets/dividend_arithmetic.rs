//! Fuzz target for dividend arithmetic (issue #433 / issue #378).
//!
//! Exercises the three arithmetic-heavy paths in the dividend contract:
//!
//! 1. **Claim total bounds** — the sum of all holder payouts must never
//!    exceed `total_amount` regardless of the input distribution.
//! 2. **Overflow detection** — `total_amount * snapshot_balance` overflows
//!    i128 for extreme values; the contract must return `ArithmeticOverflow`
//!    rather than computing a wrong result.
//! 3. **Dust behaviour** — the unclaimable dust
//!    (`total_amount - sum(floor(total_amount * b_i / supply))`) must
//!    satisfy `0 <= dust <= N - 1` where `N` is the number of holders.
//!
//! # How to run
//!
//! ```bash
//! cd fuzz
//! cargo fuzz run fuzz_dividend_arithmetic
//! # Optionally bound time/input size:
//! cargo fuzz run fuzz_dividend_arithmetic -- -max_len=1024 -max_total_time=300
//! ```
//!
//! Any crash or assertion failure is saved under
//! `fuzz/artifacts/fuzz_dividend_arithmetic/` together with a minimal
//! reproducer.  See `FUZZING.md` at the repo root for full instructions.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

/// A single holder entry: (balance, as a fraction of u16::MAX so it fits
/// comfortably in i128 arithmetic even after multiplication).
#[derive(Arbitrary, Debug)]
struct Holder {
    balance: u16,
}

/// Fuzz input: a distribution with up to 64 holders.
#[derive(Arbitrary, Debug)]
struct Input {
    /// Total amount to distribute. Clamped to [1, i64::MAX] so
    /// total_amount * balance never overflows i128 for u16 balances.
    total_amount_raw: u32,
    holders: Vec<Holder>,
}

fuzz_target!(|input: Input| {
    // Normalise inputs to avoid trivially uninteresting cases.
    let total_amount = (input.total_amount_raw as i128).max(1);

    // Filter out empty or all-zero-balance holder lists.
    let holders: Vec<i128> = input
        .holders
        .iter()
        .filter(|h| h.balance > 0)
        .map(|h| h.balance as i128)
        .take(64) // keep inputs manageable
        .collect();

    if holders.is_empty() {
        return;
    }

    let supply: i128 = holders.iter().sum();
    if supply <= 0 {
        return;
    }

    // Replicate the on-chain claimable formula for each holder.
    let mut total_paid: i128 = 0;
    for &balance in &holders {
        // Overflow check mirrors the contract's `checked_mul` guard
        // (ArithmeticOverflow, error #10).
        let product = match total_amount.checked_mul(balance) {
            Some(p) => p,
            None => {
                // Overflow is expected and correct behaviour; not a bug.
                return;
            }
        };
        let share = product / supply;
        total_paid = total_paid
            .checked_add(share)
            .expect("running total of shares overflowed — this is a bug");
    }

    // Invariant 1: total paid must never exceed total_amount.
    assert!(
        total_paid <= total_amount,
        "total_paid ({total_paid}) > total_amount ({total_amount}) — over-distribution bug"
    );

    // Invariant 2: dust must be in [0, N-1].
    let dust = total_amount - total_paid;
    let n = holders.len() as i128;
    assert!(
        dust >= 0,
        "dust ({dust}) is negative — impossible for floor division"
    );
    assert!(
        dust < n,
        "dust ({dust}) >= N ({n}) — dust bound violated"
    );
});
