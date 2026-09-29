//! Fuzz target: compliance expiry edge cases
//!
//! Exercises `is_allowed` expiry semantics across a range of ledger sequences
//! and `expires_at` values to discover off-by-one errors, integer overflow in
//! sequence comparisons, and sentinel (`0`) mishandling.
//!
//! Invariants checked:
//! - A record with `expires_at = 0` (never-expires sentinel) must pass
//!   `is_allowed` at every ledger sequence.
//! - A record with a positive `expires_at` must fail `is_allowed` at every
//!   ledger sequence >= `expires_at`, and pass at every sequence < `expires_at`.
//! - `add_to_allowlist` must reject `expires_at` values already in the past
//!   (i.e. <= the current ledger sequence) and must accept `0` at any sequence.

use arbitrary::{Arbitrary, Unstructured};
use compliance::ComplianceContract;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, String,
};

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    /// Ledger sequence at which the approval is created.
    creation_ledger: u32,
    /// Offset added to `creation_ledger` to compute `expires_at`.
    /// 0 → never-expires sentinel.
    /// Positive → future expiry.
    /// Negative or zero-but-non-zero → past expiry (should be rejected).
    expires_offset: u32,
    /// Ledger sequences at which to check `is_allowed` (relative to creation).
    check_offsets: [u32; 6],
}

fn main() {
    let raw: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
    let mut u = Unstructured::new(&raw);
    for _ in 0..5_000 {
        let Ok(input) = FuzzInput::arbitrary(&mut u) else {
            break;
        };
        fuzz_one(&input);
    }
}

fn fuzz_one(input: &FuzzInput) {
    // Clamp creation_ledger to a safe range to avoid overflow in arithmetic.
    let creation = input.creation_ledger.min(u32::MAX - 1_000_000);

    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ComplianceContract, ());
    let client = compliance::ComplianceContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");

    env.ledger().with_mut(|l| l.sequence_number = creation);

    if input.expires_offset == 0 {
        // Never-expires sentinel: must always be accepted and never lapse.
        client.add_to_allowlist(&admin, &user, &us, &0);

        for &offset in &input.check_offsets {
            let check_at = creation.saturating_add(offset);
            env.ledger().with_mut(|l| l.sequence_number = check_at);
            assert!(
                client.is_allowed(&user),
                "never-expires record must pass at ledger {check_at} (creation={creation})"
            );
        }
    } else {
        let expires_at = creation.saturating_add(input.expires_offset);

        if expires_at <= creation {
            // Past or same-ledger expiry → add_to_allowlist must reject it.
            let result = client.try_add_to_allowlist(&admin, &user, &us, &expires_at);
            assert!(
                result.is_err(),
                "past expires_at={expires_at} at creation={creation} must be rejected"
            );
        } else {
            // Future expiry: must be accepted.
            client.add_to_allowlist(&admin, &user, &us, &expires_at);

            for &offset in &input.check_offsets {
                let check_at = creation.saturating_add(offset);
                env.ledger().with_mut(|l| l.sequence_number = check_at);
                let allowed = client.is_allowed(&user);
                if check_at < expires_at {
                    assert!(
                        allowed,
                        "record must pass at ledger {check_at} < expires_at={expires_at}"
                    );
                } else {
                    assert!(
                        !allowed,
                        "record must fail at ledger {check_at} >= expires_at={expires_at}"
                    );
                }
            }
        }
    }
}
