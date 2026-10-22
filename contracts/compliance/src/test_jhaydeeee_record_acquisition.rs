//! Tests for `ComplianceContract::record_acquisition`: interaction with
//! suspended/removed holders (#525) and resource cost baseline (#524).
//!
//! The compliance contract has no global pause. The nearest "paused or
//! deactivated" states are a holder being `Suspended` or removed from the
//! allowlist. `record_acquisition` only touches the holder's
//! `FirstAcquiredLedger` entry and never consults the KYC record, so it is
//! deliberately state-independent: it succeeds in those states.
#![cfg(test)]
use super::*;
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Env};

fn setup() -> (Env, ComplianceContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ComplianceContract, ());
    let client = ComplianceContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

// ---- issue #525: suspended / removed holder ----

#[test]
fn test_record_acquisition_for_suspended_holder_succeeds() {
    let (env, client, admin) = setup();
    let holder = Address::generate(&env);
    client.add_to_allowlist(&admin, &holder, &String::from_str(&env, "US"), &0);
    client.suspend(&admin, &holder);
    env.ledger().set_sequence_number(77);
    client.record_acquisition(&admin, &holder);
    assert_eq!(client.get_first_acquired_ledger(&holder), Some(77));
    // The suspension is unaffected.
    assert!(!client.is_allowed(&holder));
}

#[test]
fn test_record_acquisition_for_removed_holder_succeeds() {
    let (env, client, admin) = setup();
    let holder = Address::generate(&env);
    client.add_to_allowlist(&admin, &holder, &String::from_str(&env, "US"), &0);
    client.remove(&admin, &holder);
    client.record_acquisition(&admin, &holder);
    assert!(client.get_first_acquired_ledger(&holder).is_some());
    assert!(!client.is_allowed(&holder));
}

#[test]
fn test_record_acquisition_for_unknown_holder_succeeds() {
    let (env, client, admin) = setup();
    let holder = Address::generate(&env);
    client.record_acquisition(&admin, &holder);
    assert!(client.get_first_acquired_ledger(&holder).is_some());
}

#[test]
fn test_record_acquisition_while_suspended_keeps_earliest_ledger() {
    let (env, client, admin) = setup();
    let holder = Address::generate(&env);
    client.add_to_allowlist(&admin, &holder, &String::from_str(&env, "US"), &0);
    env.ledger().set_sequence_number(10);
    client.record_acquisition(&admin, &holder);
    client.suspend(&admin, &holder);
    env.ledger().set_sequence_number(500);
    client.record_acquisition(&admin, &holder);
    assert_eq!(client.get_first_acquired_ledger(&holder), Some(10));
}

// ---- issue #524: resource cost ----

/// Baseline cost for `record_acquisition`. Values are measured via the budget
/// API; the test asserts they are non-zero and far below Soroban's
/// per-transaction limits (CPU 100M instructions, memory 40,960,000 bytes).
/// The repeat call (existing entry, no write) must not cost more than the
/// first call, which writes storage and emits an event.
#[test]
fn test_record_acquisition_resource_cost_baseline() {
    let (env, client, admin) = setup();
    let holder = Address::generate(&env);

    env.cost_estimate().budget().reset_default();
    client.record_acquisition(&admin, &holder);
    let cpu_first = env.cost_estimate().budget().cpu_instruction_cost();
    let mem_first = env.cost_estimate().budget().memory_bytes_cost();

    env.cost_estimate().budget().reset_default();
    client.record_acquisition(&admin, &holder);
    let cpu_repeat = env.cost_estimate().budget().cpu_instruction_cost();

    assert!(cpu_first > 0, "CPU cost must be non-zero");
    assert!(mem_first > 0, "memory cost must be non-zero");
    assert!(cpu_first < 10_000_000, "first call CPU {cpu_first} unexpectedly high");
    assert!(mem_first < 4_000_000, "first call memory {mem_first} unexpectedly high");
    assert!(
        cpu_repeat <= cpu_first,
        "repeat call CPU {cpu_repeat} exceeds first call {cpu_first}"
    );
}
