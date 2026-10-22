#![cfg(test)]
//! Edge-case tests for `approve` (zero input, events, paused state, cost).
use super::*;
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, Env, String,
};

struct S {
    env: Env,
    token: AssetTokenContractClient<'static>,
    admin: Address,
}

fn setup() -> S {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    compliance.initialize(&admin);
    compliance.add_to_allowlist(&admin, &admin, &String::from_str(&env, "US"), &0);
    let tid = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &tid);
    token.initialize(
        &admin,
        &String::from_str(&env, "Manhattan Loft"),
        &String::from_str(&env, "MLOFT"),
        &String::from_str(&env, "real_estate"),
        &1_000i128,
        &2u32,
        &cid,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );
    S { env, token, admin }
}

// ---- #532: zero / minimal input ----

#[test]
fn zero_amount_approve_succeeds_and_reads_zero() {
    let s = setup();
    let spender = Address::generate(&s.env);
    s.token.approve(&s.admin, &spender, &0, &0);
    assert_eq!(s.token.allowance(&s.admin, &spender), 0);
}

#[test]
fn zero_amount_clears_existing_allowance() {
    let s = setup();
    let spender = Address::generate(&s.env);
    let exp = s.env.ledger().sequence() + 100;
    s.token.approve(&s.admin, &spender, &500, &exp);
    assert_eq!(s.token.allowance(&s.admin, &spender), 500);
    s.token.approve(&s.admin, &spender, &0, &exp);
    assert_eq!(s.token.allowance(&s.admin, &spender), 0);
}

#[test]
fn zero_amount_ignores_past_expiration() {
    let s = setup();
    s.env.ledger().with_mut(|l| l.sequence_number = 50);
    let spender = Address::generate(&s.env);
    assert!(s.token.try_approve(&s.admin, &spender, &0, &1).is_ok());
}

#[test]
fn positive_amount_with_past_expiration_rejected() {
    let s = setup();
    s.env.ledger().with_mut(|l| l.sequence_number = 50);
    let spender = Address::generate(&s.env);
    assert!(s.token.try_approve(&s.admin, &spender, &1, &10).is_err());
}

#[test]
fn negative_amount_rejected() {
    let s = setup();
    let spender = Address::generate(&s.env);
    assert!(s.token.try_approve(&s.admin, &spender, &-1, &1_000).is_err());
}

// ---- #533: event emission ----
// `events().all()` reflects only the most recent invocation, so each check
// reads it immediately after the `approve` call.

#[test]
fn approve_emits_one_event() {
    let s = setup();
    let spender = Address::generate(&s.env);
    let exp = s.env.ledger().sequence() + 100;
    s.token.approve(&s.admin, &spender, &250, &exp);
    assert_eq!(s.env.events().all().events().len(), 1);
}

#[test]
fn zero_amount_approve_still_emits_event() {
    let s = setup();
    let spender = Address::generate(&s.env);
    s.token.approve(&s.admin, &spender, &0, &0);
    assert_eq!(s.env.events().all().events().len(), 1);
}

#[test]
fn overwriting_approve_emits_event_each_time() {
    let s = setup();
    let spender = Address::generate(&s.env);
    let exp = s.env.ledger().sequence() + 100;
    s.token.approve(&s.admin, &spender, &100, &exp);
    assert_eq!(s.env.events().all().events().len(), 1);
    s.token.approve(&s.admin, &spender, &200, &exp);
    assert_eq!(s.env.events().all().events().len(), 1);
}

#[test]
fn rejected_approve_emits_no_event() {
    let s = setup();
    let spender = Address::generate(&s.env);
    assert!(s.token.try_approve(&s.admin, &spender, &-1, &1_000).is_err());
    assert_eq!(s.env.events().all().events().len(), 0);
}

// ---- #534: resource cost ----
// Generous ceiling guards against order-of-magnitude regressions without
// being brittle across SDK versions. Observed cost is printed with
// `cargo test -- --nocapture` so a baseline can be recorded.

#[test]
fn approve_cpu_cost_within_baseline() {
    let s = setup();
    let spender = Address::generate(&s.env);
    let exp = s.env.ledger().sequence() + 100;
    s.env.cost_estimate().budget().reset_default();
    s.token.approve(&s.admin, &spender, &250, &exp);
    let cpu = s.env.cost_estimate().budget().cpu_instruction_cost();
    let mem = s.env.cost_estimate().budget().memory_bytes_cost();
    std::println!("approve cost: cpu={cpu} mem={mem}");
    assert!(cpu < 5_000_000, "approve cpu cost unexpectedly high: {cpu}");
    assert!(mem < 1_000_000, "approve memory cost unexpectedly high: {mem}");
}

#[test]
fn approve_overwrite_cost_not_greater_than_double_first() {
    let s = setup();
    let spender = Address::generate(&s.env);
    let exp = s.env.ledger().sequence() + 100;
    s.env.cost_estimate().budget().reset_default();
    s.token.approve(&s.admin, &spender, &1, &exp);
    let first = s.env.cost_estimate().budget().cpu_instruction_cost();
    s.env.cost_estimate().budget().reset_default();
    s.token.approve(&s.admin, &spender, &2, &exp);
    let second = s.env.cost_estimate().budget().cpu_instruction_cost();
    assert!(second <= first * 2, "overwrite cost {second} vs first {first}");
}

// ---- #535: paused state ----
// The contract has no separate "deactivated" state; `paused` is the only
// gate and it is checked by `approve`.

#[test]
fn approve_rejected_while_paused() {
    let s = setup();
    let spender = Address::generate(&s.env);
    s.token.pause(&s.admin);
    let exp = s.env.ledger().sequence() + 100;
    assert!(s.token.try_approve(&s.admin, &spender, &10, &exp).is_err());
    assert_eq!(s.token.allowance(&s.admin, &spender), 0);
}

#[test]
fn zero_approve_also_rejected_while_paused() {
    // Current behaviour: the pause check precedes the zero-amount path, so
    // an owner cannot revoke an allowance while paused.
    let s = setup();
    let spender = Address::generate(&s.env);
    let exp = s.env.ledger().sequence() + 100;
    s.token.approve(&s.admin, &spender, &10, &exp);
    s.token.pause(&s.admin);
    assert!(s.token.try_approve(&s.admin, &spender, &0, &exp).is_err());
    assert_eq!(s.token.allowance(&s.admin, &spender), 10);
}

#[test]
fn approve_works_again_after_unpause() {
    let s = setup();
    let spender = Address::generate(&s.env);
    s.token.pause(&s.admin);
    s.token.unpause(&s.admin);
    let exp = s.env.ledger().sequence() + 100;
    s.token.approve(&s.admin, &spender, &10, &exp);
    assert_eq!(s.token.allowance(&s.admin, &spender), 10);
}
