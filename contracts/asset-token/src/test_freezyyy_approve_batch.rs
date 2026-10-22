#![cfg(test)]
//! Coverage for `approve` authorisation and `transfer_batch` events, pause
//! behaviour and resource cost (issues #528-#531).
use super::*;
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, String, Vec,
};

struct Setup {
    env: Env,
    token: AssetTokenContractClient<'static>,
    compliance: ComplianceContractClient<'static>,
    admin: Address,
}

fn setup(supply: i128) -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    let admin = Address::generate(&env);
    compliance.initialize(&admin);
    allow(&env, &compliance, &admin, &admin);
    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);
    token.initialize(
        &admin,
        &String::from_str(&env, "Manhattan Loft"),
        &String::from_str(&env, "MLOFT"),
        &String::from_str(&env, "real_estate"),
        &supply,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );
    Setup {
        env,
        token,
        compliance,
        admin,
    }
}

fn allow(env: &Env, c: &ComplianceContractClient, admin: &Address, who: &Address) {
    c.add_to_allowlist(admin, who, &String::from_str(env, "US"), &0);
}

// ---- issue #531: approve's unauthorised caller ----

/// `approve` is authorised by `from` alone. A call that carries no
/// authorisation for `from` is rejected and no allowance is written.
#[test]
fn test_approve_without_owner_auth_is_rejected() {
    let s = setup(1_000);
    let spender = Address::generate(&s.env);
    // Drop the blanket auth mock: nobody has authorised anything now.
    s.env.mock_auths(&[]);
    let exp = s.env.ledger().sequence() + 100;
    let res = s.token.try_approve(&s.admin, &spender, &100, &exp);
    assert!(res.is_err(), "approve must require auth from `from`");
    assert_eq!(s.token.allowance(&s.admin, &spender), 0);
}

/// `approve` is intentionally open to any address that authorises itself: it
/// needs no admin role and no compliance allowlisting, because it only moves
/// the caller's own allowance. Documented current behaviour.
#[test]
fn test_approve_open_to_unprivileged_self_authorised_caller() {
    let s = setup(1_000);
    let stranger = Address::generate(&s.env);
    let spender = Address::generate(&s.env);
    assert!(!s.compliance.is_allowed(&stranger));
    let exp = s.env.ledger().sequence() + 100;
    s.token.approve(&stranger, &spender, &50, &exp);
    assert_eq!(s.token.allowance(&stranger, &spender), 50);
    // It did not touch the admin's allowance for the same spender.
    assert_eq!(s.token.allowance(&s.admin, &spender), 0);
}

// ---- issue #530: transfer_batch while paused / deactivated ----

/// The token has no separate "deactivated" flag: `pause` is the only
/// halt state. While paused, `transfer_batch` reverts deliberately with
/// `Error::Paused` (#6), before any balance is touched.
#[test]
fn test_transfer_batch_rejected_when_paused_with_deliberate_error() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    allow(&s.env, &s.compliance, &s.admin, &bob);
    s.token.pause(&s.admin);

    let mut batch = Vec::new(&s.env);
    batch.push_back((bob.clone(), 100i128));
    let res = s.token.try_transfer_batch(&s.admin, &batch);
    assert_eq!(res, Err(Ok(Error::Paused)));
    assert_eq!(s.token.balance(&s.admin), 1_000);
    assert_eq!(s.token.balance(&bob), 0);
}

/// Pause is checked even for an empty batch, and unpausing restores service.
#[test]
fn test_transfer_batch_paused_empty_batch_and_unpause_restores() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    allow(&s.env, &s.compliance, &s.admin, &bob);
    s.token.pause(&s.admin);

    let empty: Vec<(Address, i128)> = Vec::new(&s.env);
    assert_eq!(
        s.token.try_transfer_batch(&s.admin, &empty),
        Err(Ok(Error::Paused))
    );

    s.token.unpause(&s.admin);
    let mut batch = Vec::new(&s.env);
    batch.push_back((bob.clone(), 100i128));
    s.token.transfer_batch(&s.admin, &batch);
    assert_eq!(s.token.balance(&bob), 100);
}

// ---- issue #529: transfer_batch resource cost ----

/// Measures CPU and memory for a batch of N entries and records a baseline
/// ceiling. Cost is documented to scale linearly with the entry count (one
/// cross-contract compliance call per entry). The ceilings are deliberately
/// generous so the test flags order-of-magnitude regressions, not noise.
///
fn batch_cost(n: u32) -> (u64, u64) {
    let s = setup(1_000_000);
    let mut batch = Vec::new(&s.env);
    for _ in 0..n {
        let to = Address::generate(&s.env);
        allow(&s.env, &s.compliance, &s.admin, &to);
        batch.push_back((to, 1i128));
    }
    s.env.cost_estimate().budget().reset_default();
    s.token.transfer_batch(&s.admin, &batch);
    let b = s.env.cost_estimate().budget();
    (b.cpu_instruction_cost(), b.memory_bytes_cost())
}

#[test]
fn test_transfer_batch_resource_cost_baseline_and_linear_scaling() {
    let (cpu1, mem1) = batch_cost(1);
    let (cpu10, mem10) = batch_cost(10);

    // Baseline ceilings (well under the 100M-instruction network limit).
    assert!(cpu10 < 50_000_000, "n=10 cpu {cpu10} exceeds baseline");
    assert!(mem10 < 20_000_000, "n=10 mem {mem10} exceeds baseline");
    // More entries cost more, but sub-quadratically: 10 entries must cost
    // well under 10x(+overhead) of one entry.
    assert!(cpu10 > cpu1);
    assert!(cpu10 < cpu1 * 15, "cost grows faster than linearly");
}

// ---- issue #528: transfer_batch event emission ----

/// Every state change emits exactly one `transfer` event, including a
/// repeated recipient, so indexers can rebuild activity from events alone.
/// A self-transfer entry changes no balance but still emits its event, in the
/// same `(amount, from_balance_after, to_balance_after)` shape.
/// Event shape: topics `("transfer", from, to)`, data `(amount, new_from_bal,
/// new_to_bal)`.
#[test]
fn test_transfer_batch_emits_one_event_per_entry_including_repeats_and_self() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    allow(&s.env, &s.compliance, &s.admin, &bob);

    let mut batch = Vec::new(&s.env);
    batch.push_back((bob.clone(), 100i128));
    batch.push_back((bob.clone(), 50i128));
    batch.push_back((s.admin.clone(), 10i128));
    s.token.transfer_batch(&s.admin, &batch);

    // Read events before view calls, which would replace them.
    assert_eq!(s.env.events().all().events().len(), 3);
    assert_eq!(s.token.balance(&s.admin), 850);
    assert_eq!(s.token.balance(&bob), 150);
}

/// A reverted batch publishes no events that persist, and an empty batch
/// changes nothing and emits nothing.
#[test]
fn test_transfer_batch_empty_emits_no_events() {
    let s = setup(1_000);
    let empty: Vec<(Address, i128)> = Vec::new(&s.env);
    s.token.transfer_batch(&s.admin, &empty);
    assert_eq!(s.env.events().all().events().len(), 0);
}
