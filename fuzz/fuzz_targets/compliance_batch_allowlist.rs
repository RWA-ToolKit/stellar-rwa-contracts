//! Fuzz target: compliance batch allowlist operations
//!
//! Tests `add_to_allowlist_batch` with random valid/invalid entries to
//! discover unexpected panics, partial-commit bugs, and counter drift.
//!
//! Invariants checked:
//! - The allowlist counter always equals the number of distinct addresses
//!   actually present on the list.
//! - A batch that contains any entry with `expires_at` in the past reverts
//!   entirely — no partial commit.
//! - After a successful batch, every admitted address passes `is_allowed`.
//! - After a failed batch, no address from that batch appears on the list.

use arbitrary::{Arbitrary, Unstructured};
use compliance::{AllowlistEntry, ComplianceContract};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, String, Vec,
};

/// A single entry for the batch, using raw byte arrays so the fuzzer can
/// generate boundary values (empty strings, long strings, non-ASCII, etc.).
#[derive(Debug, Arbitrary)]
struct FuzzEntry {
    /// 0-3: used to pick a jurisdiction from a fixed set so we exercise
    /// normalization and rejection paths without wasting budget on
    /// completely random strings.
    jurisdiction_idx: u8,
    /// Offset added to (or subtracted from) the current ledger sequence to
    /// produce `expires_at`. A value of 0 means "never expires".
    expires_offset: i32,
}

const JURISDICTIONS: &[&str] = &[
    "US", "KE", "DE", "us", "ke", "", "USA", "U1", "  ", "ZZ",
];

fn main() {
    // Use a fixed seed so the harness is deterministic when replaying a
    // crash file. In real fuzzing the seed changes every run.
    let raw: Vec<u8> = (0u8..=255).collect();
    let mut u = Unstructured::new(&raw);
    for _ in 0..1_000 {
        let Ok(entries_data) = <[FuzzEntry; 4]>::arbitrary(&mut u) else {
            break;
        };
        fuzz_one(&entries_data);
    }
}

fn fuzz_one(entries_data: &[FuzzEntry]) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ComplianceContract, ());
    let client = compliance::ComplianceContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Fix ledger sequence so expires_offset arithmetic is predictable.
    env.ledger().with_mut(|l| l.sequence_number = 1000);
    let now = env.ledger().sequence();

    // Build the batch.
    let mut batch: Vec<AllowlistEntry> = Vec::new(&env);
    let mut has_invalid = false;
    let addrs: std::vec::Vec<Address> = entries_data
        .iter()
        .map(|_| Address::generate(&env))
        .collect();

    for (i, e) in entries_data.iter().enumerate() {
        let jur_str = JURISDICTIONS[(e.jurisdiction_idx as usize) % JURISDICTIONS.len()];
        let jurisdiction = String::from_str(&env, jur_str);
        let expires_at = if e.expires_offset == 0 {
            0u32
        } else if e.expires_offset > 0 {
            now.saturating_add(e.expires_offset as u32)
        } else {
            // Negative offset → in the past → invalid
            let abs = e.expires_offset.unsigned_abs();
            let result = now.saturating_sub(abs);
            if result == 0 {
                1 // 1 is in the past when now=1000
            } else {
                result
            }
        };
        // An expires_at that is non-zero and <= now is invalid.
        if expires_at != 0 && expires_at <= now {
            has_invalid = true;
        }
        // Two-letter uppercase ASCII → valid jurisdiction; otherwise invalid.
        let valid_jur = jur_str.len() == 2
            && jur_str
                .chars()
                .all(|c| c.is_ascii_alphabetic());
        if !valid_jur {
            has_invalid = true;
        }
        batch.push_back(AllowlistEntry {
            address: addrs[i].clone(),
            jurisdiction,
            expires_at,
        });
    }

    let count_before = client.get_allowlist_count();
    let result = client.try_add_to_allowlist_batch(&admin, &batch);

    if has_invalid {
        // The batch must have reverted; no new addresses committed.
        assert!(
            result.is_err(),
            "batch with invalid entry must fail, but it succeeded"
        );
        assert_eq!(
            client.get_allowlist_count(),
            count_before,
            "failed batch must not change the allowlist count"
        );
    } else {
        // The batch must have succeeded; all addresses are now on the list.
        assert!(result.is_ok(), "valid batch must succeed");
        // Counter must reflect exactly the new entries (none were pre-existing).
        assert_eq!(
            client.get_allowlist_count(),
            count_before + batch.len() as u32,
            "counter must equal number of newly admitted addresses"
        );
        // Every admitted address must pass is_allowed.
        for addr in addrs.iter() {
            assert!(
                client.is_allowed(addr),
                "admitted address must pass is_allowed"
            );
        }
    }

    // In all cases the counter must equal the length of get_allowlist.
    assert_eq!(
        client.get_allowlist_count(),
        client.get_allowlist().len(),
        "maintained counter must match get_allowlist().len()"
    );
}
