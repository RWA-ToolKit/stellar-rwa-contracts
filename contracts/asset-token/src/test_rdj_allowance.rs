#![cfg(test)]
//! Coverage for the read-only `allowance` entry point: caller authority,
//! minimal input, event emission and resource cost.
use super::*;
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, Env, String,
};

fn setup() -> (Env, AssetTokenContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    let admin = Address::generate(&env);
    compliance.initialize(&admin);
    compliance.add_to_allowlist(&admin, &admin, &String::from_str(&env, "US"), &0);
    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);
    token.initialize(
        &admin,
        &String::from_str(&env, "Manhattan Loft"),
        &String::from_str(&env, "MLOFT"),
        &String::from_str(&env, "real_estate"),
        &1_000i128,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );
    (env, token, admin)
}

/// `allowance` is intentionally open to any caller: it is a pure view and
/// requires no authorization. Verified by calling it with no auths mocked.
#[test]
fn test_allowance_unprivileged_caller_can_read_without_auth() {
    let (env, token, admin) = setup();
    let spender = Address::generate(&env);
    token.approve(&admin, &spender, &250, &(env.ledger().sequence() + 100));

    // A view needs no require_auth, so the call records no authorization.
    assert_eq!(token.allowance(&admin, &spender), 250);
    assert!(env.auths().is_empty());
}

/// A stranger (neither owner nor spender) reads the same value and cannot
/// change it by querying.
#[test]
fn test_allowance_unknown_pair_reads_zero_and_does_not_mutate() {
    let (env, token, admin) = setup();
    let spender = Address::generate(&env);
    let stranger = Address::generate(&env);
    assert_eq!(token.allowance(&stranger, &spender), 0);
    assert_eq!(token.allowance(&admin, &stranger), 0);
    assert_eq!(token.allowance(&admin, &spender), 0);
}

/// Zero/minimal input: a never-approved pair, an explicit zero approval and a
/// zero approval with a past expiry all read as 0. `allowance` itself takes no
/// amount, so these are its minimal-input states (deliberate: it never errors).
#[test]
fn test_allowance_zero_and_minimal_inputs_read_zero() {
    let (env, token, admin) = setup();
    let spender = Address::generate(&env);
    assert_eq!(token.allowance(&admin, &spender), 0);

    token.approve(&admin, &spender, &100, &(env.ledger().sequence() + 50));
    assert_eq!(token.allowance(&admin, &spender), 100);

    // amount == 0 clears the allowance regardless of expiration_ledger.
    token.approve(&admin, &spender, &0, &0);
    assert_eq!(token.allowance(&admin, &spender), 0);
}

/// Owner == spender is not special-cased: it reads as zero until approved.
#[test]
fn test_allowance_self_pair_reads_zero() {
    let (_env, token, admin) = setup();
    assert_eq!(token.allowance(&admin, &admin), 0);
}

/// Expiry boundary: valid through `expiration_ledger` inclusive, zero after.
#[test]
fn test_allowance_expiry_boundary_inclusive() {
    let (env, token, admin) = setup();
    let spender = Address::generate(&env);
    let exp = env.ledger().sequence() + 10;
    token.approve(&admin, &spender, &7, &exp);
    env.ledger().set_sequence_number(exp);
    assert_eq!(token.allowance(&admin, &spender), 7);
    env.ledger().set_sequence_number(exp + 1);
    assert_eq!(token.allowance(&admin, &spender), 0);
}

/// `allowance` makes no state change, so it must emit no event (matches its
/// `# Events: None` doc). The state change that creates an allowance,
/// `approve`, emits exactly one event.
#[test]
fn test_allowance_emits_no_event_but_approve_does() {
    let (env, token, admin) = setup();
    let spender = Address::generate(&env);

    token.approve(&admin, &spender, &40, &(env.ledger().sequence() + 20));
    // `events().all()` reflects only the most recent invocation.
    assert_eq!(env.events().all().events().len(), 1);

    assert_eq!(token.allowance(&admin, &spender), 40);
    assert_eq!(env.events().all().events().len(), 0);
}

/// Resource baseline: `allowance` is a single temporary-storage read, so it
/// must stay cheap. Bounds are generous ceilings (not exact values) to flag
/// regressions without being brittle across SDK upgrades.
#[test]
fn test_allowance_resource_cost_baseline() {
    let (env, token, admin) = setup();
    let spender = Address::generate(&env);
    token.approve(&admin, &spender, &100, &(env.ledger().sequence() + 100));

    env.cost_estimate().budget().reset_default();
    assert_eq!(token.allowance(&admin, &spender), 100);
    let cpu_hit = env.cost_estimate().budget().cpu_instruction_cost();
    let mem_hit = env.cost_estimate().budget().memory_bytes_cost();

    let other = Address::generate(&env);
    env.cost_estimate().budget().reset_default();
    assert_eq!(token.allowance(&admin, &other), 0);
    let cpu_miss = env.cost_estimate().budget().cpu_instruction_cost();

    assert!(cpu_hit > 0 && cpu_hit < 5_000_000, "cpu_hit={cpu_hit}");
    assert!(mem_hit < 1_000_000, "mem_hit={mem_hit}");
    assert!(cpu_miss < 5_000_000, "cpu_miss={cpu_miss}");
}
