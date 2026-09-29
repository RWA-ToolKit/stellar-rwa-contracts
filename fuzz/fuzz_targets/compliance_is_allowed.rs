//! Fuzz target: `is_allowed` with all reachable state combinations
//!
//! Exercises every combination of KYC status, expiry, jurisdiction-block,
//! and holding-period state that is reachable through the public API, and
//! asserts that `is_allowed` returns the correct boolean. This covers all
//! branches in the gate function including the minimum-holding-period check
//! (issue #454).
//!
//! Invariants checked:
//! - `is_allowed` returns `false` for unknown addresses.
//! - `is_allowed` returns `false` for Suspended records regardless of other state.
//! - `is_allowed` returns `false` when `expires_at` is reached or passed.
//! - `is_allowed` returns `false` when the address is in a blocked jurisdiction.
//! - `is_allowed` returns `false` when the holding period has not elapsed.
//! - `is_allowed` returns `true` only when ALL conditions are satisfied.
//! - `is_allowed` never panics for any combination of inputs.

use arbitrary::{Arbitrary, Unstructured};
use compliance::ComplianceContract;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, String,
};

/// All dimensions that influence the output of `is_allowed`,
/// set via the public admin API only.
#[derive(Debug, Arbitrary)]
struct FuzzInput {
    /// Whether the address has a KYC record at all.
    has_record: bool,
    /// If it has a record, whether to suspend it afterwards.
    suspended: bool,
    /// 0 → never expires; > 0 → expires after this many ledgers.
    expires_after: u32,
    /// Whether the address's jurisdiction should be blocked.
    block_jurisdiction: bool,
    /// 0 → no holding period; > 0 → minimum ledgers required.
    min_holding_period: u64,
    /// Ledgers after creation at which `record_acquisition` fires.
    acquisition_offset: u32,
    /// Ledgers after creation at which `is_allowed` is checked.
    check_offset: u32,
}

fn main() {
    let raw: Vec<u8> = (0u8..=255).cycle().take(8192).collect();
    let mut u = Unstructured::new(&raw);
    for _ in 0..10_000 {
        let Ok(input) = FuzzInput::arbitrary(&mut u) else {
            break;
        };
        fuzz_one(&input);
    }
}

fn fuzz_one(input: &FuzzInput) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ComplianceContract, ());
    let client = compliance::ComplianceContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    let creation_ledger: u32 = 1_000;
    env.ledger().with_mut(|l| l.sequence_number = creation_ledger);

    let user = Address::generate(&env);
    let jur = String::from_str(&env, "US");

    // --- set up state via public API ---

    if input.has_record {
        let expires_at: u32 = if input.expires_after == 0 {
            0
        } else {
            creation_ledger.saturating_add(input.expires_after.min(1_000_000))
        };

        // expires_at must be > creation_ledger when non-zero; skip if not.
        if expires_at == 0 || expires_at > creation_ledger {
            let _ = client.try_add_to_allowlist(&admin, &user, &jur, &expires_at);

            if input.suspended {
                let _ = client.try_suspend(&admin, &user);
            }
        }
    }

    if input.block_jurisdiction {
        client.block_jurisdiction(&admin, &jur);
    }

    if input.min_holding_period > 0 {
        let capped = input.min_holding_period.min(2_000_000);
        client.set_min_holding_period(&admin, &user, &capped);

        let acq_ledger =
            creation_ledger.saturating_add(input.acquisition_offset.min(1_000_000));
        env.ledger().with_mut(|l| l.sequence_number = acq_ledger);
        client.record_acquisition(&admin, &user);
    }

    let check_ledger = creation_ledger.saturating_add(input.check_offset.min(2_000_000));
    env.ledger().with_mut(|l| l.sequence_number = check_ledger);

    // --- compute expected result ---

    let expires_at: u32 = if input.expires_after == 0 {
        0
    } else {
        creation_ledger.saturating_add(input.expires_after.min(1_000_000))
    };
    let record_was_created =
        input.has_record && (expires_at == 0 || expires_at > creation_ledger);

    let is_approved = record_was_created && !input.suspended;
    let is_expired = record_was_created && expires_at != 0 && check_ledger >= expires_at;
    let jur_blocked = input.block_jurisdiction;

    let holding_ok = if record_was_created && input.min_holding_period > 0 {
        let capped = input.min_holding_period.min(2_000_000);
        let acq_ledger = creation_ledger.saturating_add(input.acquisition_offset.min(1_000_000));
        let unlock_at = acq_ledger as u64 + capped;
        (check_ledger as u64) >= unlock_at
    } else {
        true
    };

    let expected = is_approved && !is_expired && !jur_blocked && holding_ok;

    // --- assert: is_allowed must never panic and must match expected ---

    let actual = client.is_allowed(&user);
    assert_eq!(
        actual, expected,
        "is_allowed mismatch: has_record={} suspended={} expires_at={expires_at} \
         check_ledger={check_ledger} jur_blocked={jur_blocked} \
         min_holding={} acq_offset={} expected={expected}",
        input.has_record,
        input.suspended,
        input.min_holding_period,
        input.acquisition_offset,
    );

    // Unknown address (never added) must always return false.
    let stranger = Address::generate(&env);
    assert!(
        !client.is_allowed(&stranger),
        "unknown address must always fail is_allowed"
    );
}
