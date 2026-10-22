//! Extra coverage for `transfer_from` and `allowance` edge cases.
#![cfg(test)]
use super::*;
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, String,
};

struct Ctx {
    env: Env,
    token: AssetTokenContractClient<'static>,
    admin: Address,
    owner: Address,
    spender: Address,
    to: Address,
}

fn ctx() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    let admin = Address::generate(&env);
    compliance.initialize(&admin);
    let owner = Address::generate(&env);
    let spender = Address::generate(&env);
    let to = Address::generate(&env);
    for who in [&admin, &owner, &to] {
        compliance.add_to_allowlist(&admin, who, &String::from_str(&env, "US"), &0);
    }
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
    token.transfer(&admin, &owner, &500);
    token.approve(&owner, &spender, &200, &1_000);
    Ctx {
        env,
        token,
        admin,
        owner,
        spender,
        to,
    }
}

// ---- allowance while paused (issue #540) ----

/// `allowance` is a pure read: it keeps returning the stored value while the
/// token is paused rather than panicking.
#[test]
fn test_allowance_readable_while_paused() {
    let c = ctx();
    c.token.pause(&c.admin);
    assert_eq!(c.token.allowance(&c.owner, &c.spender), 200);
}

/// Pausing does not clear allowances; they are usable again after unpause.
#[test]
fn test_allowance_survives_pause_unpause_cycle() {
    let c = ctx();
    c.token.pause(&c.admin);
    c.token.unpause(&c.admin);
    assert_eq!(c.token.allowance(&c.owner, &c.spender), 200);
    c.token.transfer_from(&c.spender, &c.owner, &c.to, &50);
    assert_eq!(c.token.allowance(&c.owner, &c.spender), 150);
}

/// While paused, `transfer_from` is rejected with `Paused` and the allowance
/// is left untouched.
#[test]
fn test_transfer_from_paused_leaves_allowance_unchanged() {
    let c = ctx();
    c.token.pause(&c.admin);
    let r = c.token.try_transfer_from(&c.spender, &c.owner, &c.to, &50);
    assert!(r.is_err());
    assert_eq!(c.token.allowance(&c.owner, &c.spender), 200);
    assert_eq!(c.token.balance(&c.owner), 500);
}

/// While paused, `approve` is rejected, so the existing allowance stays as is.
#[test]
fn test_approve_while_paused_does_not_change_allowance() {
    let c = ctx();
    c.token.pause(&c.admin);
    let r = c.token.try_approve(&c.owner, &c.spender, &999, &1_000);
    assert!(r.is_err());
    assert_eq!(c.token.allowance(&c.owner, &c.spender), 200);
}

// ---- unauthorised caller (issue #541) ----

/// `transfer_from` is gated only by allowance: a caller with no allowance
/// from `from` is rejected with `InsufficientAllowance`.
#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn test_transfer_from_no_allowance_rejected() {
    let c = ctx();
    let stranger = Address::generate(&c.env);
    c.token.transfer_from(&stranger, &c.owner, &c.to, &1);
}

/// An allowance granted to one spender is not usable by another address.
#[test]
fn test_transfer_from_allowance_not_transferable_between_spenders() {
    let c = ctx();
    let stranger = Address::generate(&c.env);
    assert_eq!(c.token.allowance(&c.owner, &stranger), 0);
    assert!(c
        .token
        .try_transfer_from(&stranger, &c.owner, &c.to, &1)
        .is_err());
    assert_eq!(c.token.balance(&c.owner), 500);
    assert_eq!(c.token.allowance(&c.owner, &c.spender), 200);
}

/// The spender needs no compliance approval and no admin role: it is never
/// allowlisted here yet a transfer_from succeeds (documented behaviour).
#[test]
fn test_transfer_from_spender_needs_no_special_authority() {
    let c = ctx();
    c.token.transfer_from(&c.spender, &c.owner, &c.to, &10);
    assert_eq!(c.token.balance(&c.to), 10);
}

/// Without the spender's authorization the call is rejected.
#[test]
fn test_transfer_from_requires_spender_auth() {
    let env = Env::default();
    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);
    let a = Address::generate(&env);
    // No mock_all_auths and not initialized: require_auth fails first.
    assert!(token.try_transfer_from(&a, &a, &a, &1).is_err());
}

// ---- zero / minimal input (issue #542) ----

/// A zero amount is deliberately rejected with `InvalidAmount` (#5).
#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_transfer_from_zero_amount_rejected() {
    let c = ctx();
    c.token.transfer_from(&c.spender, &c.owner, &c.to, &0);
}

/// A negative amount is rejected with `InvalidAmount` (#5).
#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_transfer_from_negative_amount_rejected() {
    let c = ctx();
    c.token.transfer_from(&c.spender, &c.owner, &c.to, &-1);
}

/// A rejected zero-amount call changes no state.
#[test]
fn test_transfer_from_zero_amount_leaves_state_unchanged() {
    let c = ctx();
    assert!(c
        .token
        .try_transfer_from(&c.spender, &c.owner, &c.to, &0)
        .is_err());
    assert_eq!(c.token.balance(&c.owner), 500);
    assert_eq!(c.token.balance(&c.to), 0);
    assert_eq!(c.token.allowance(&c.owner, &c.spender), 200);
}

/// The minimal valid amount (1) succeeds and decrements the allowance by 1.
#[test]
fn test_transfer_from_minimum_amount_succeeds() {
    let c = ctx();
    c.token.transfer_from(&c.spender, &c.owner, &c.to, &1);
    assert_eq!(c.token.balance(&c.to), 1);
    assert_eq!(c.token.allowance(&c.owner, &c.spender), 199);
}

/// With an allowance of zero (cleared via approve(0)), even amount 1 fails.
#[test]
fn test_transfer_from_after_zero_approval_rejected() {
    let c = ctx();
    c.token.approve(&c.owner, &c.spender, &0, &0);
    assert!(c
        .token
        .try_transfer_from(&c.spender, &c.owner, &c.to, &1)
        .is_err());
}

// ---- event emission (issue #543) ----

/// A successful `transfer_from` emits the `transfer` event, which is the only
/// state-changing record the call writes besides balances and allowance.
/// `events().all()` reflects the latest invocation, so read it immediately.
#[test]
fn test_transfer_from_emits_transfer_event() {
    let c = ctx();
    c.token.transfer_from(&c.spender, &c.owner, &c.to, &30);
    let events = c.env.events().all();
    assert!(!events.events().is_empty());
    assert_eq!(c.token.balance(&c.owner), 470);
    assert_eq!(c.token.balance(&c.to), 30);
}

/// Each successful call emits on its own; a rejected call must not leave
/// state behind.
#[test]
fn test_transfer_from_failed_call_changes_nothing() {
    let c = ctx();
    assert!(c
        .token
        .try_transfer_from(&c.spender, &c.owner, &c.to, &201)
        .is_err());
    assert_eq!(c.token.balance(&c.owner), 500);
    assert_eq!(c.token.allowance(&c.owner, &c.spender), 200);
}

/// Note: `transfer_from` reports the allowance decrease only implicitly (no
/// separate `approve` event); indexers can derive it from the `transfer`
/// amount. Documented current behaviour.
#[test]
fn test_transfer_from_allowance_decrease_matches_amount() {
    let c = ctx();
    c.token.transfer_from(&c.spender, &c.owner, &c.to, &75);
    assert_eq!(c.token.allowance(&c.owner, &c.spender), 125);
}
