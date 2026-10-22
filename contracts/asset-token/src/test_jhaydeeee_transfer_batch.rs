//! Edge-case tests for `AssetTokenContract::transfer_batch`
//! (zero/empty input and unprivileged callers).
#![cfg(test)]
use super::*;
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, String, Vec};

struct Ctx {
    env: Env,
    token: AssetTokenContractClient<'static>,
    compliance: ComplianceContractClient<'static>,
    admin: Address,
}

fn ctx(supply: i128) -> Ctx {
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
        &supply,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );
    Ctx {
        env,
        token,
        compliance,
        admin,
    }
}

fn approved(c: &Ctx) -> Address {
    let a = Address::generate(&c.env);
    c.compliance
        .add_to_allowlist(&c.admin, &a, &String::from_str(&c.env, "US"), &0);
    a
}

// ---- issue #527: zero / empty input ----

/// An empty batch is deliberately a no-op: the sender is still authenticated,
/// pause-checked and compliance-checked, but no balances change.
#[test]
fn test_transfer_batch_empty_list_is_noop() {
    let c = ctx(1_000);
    let bob = approved(&c);
    let batch: Vec<(Address, i128)> = Vec::new(&c.env);
    c.token.transfer_batch(&c.admin, &batch);
    assert_eq!(c.token.balance(&c.admin), 1_000);
    assert_eq!(c.token.balance(&bob), 0);
}

/// The sender gate runs before the loop, so even an empty batch from a
/// non-compliant sender reverts with `SenderNotCompliant` (#7).
#[test]
fn test_transfer_batch_empty_list_noncompliant_sender_rejected() {
    let c = ctx(1_000);
    let stranger = Address::generate(&c.env);
    let batch: Vec<(Address, i128)> = Vec::new(&c.env);
    let res = c.token.try_transfer_batch(&stranger, &batch);
    assert!(res.is_err());
}

/// An empty batch is also rejected while the token is paused (#6).
#[test]
fn test_transfer_batch_empty_list_rejected_when_paused() {
    let c = ctx(1_000);
    c.token.pause(&c.admin);
    let batch: Vec<(Address, i128)> = Vec::new(&c.env);
    assert!(c.token.try_transfer_batch(&c.admin, &batch).is_err());
}

/// A zero amount is rejected with `InvalidAmount` (#5), matching `transfer`.
#[test]
fn test_transfer_batch_zero_amount_rejected() {
    let c = ctx(1_000);
    let bob = approved(&c);
    let mut batch: Vec<(Address, i128)> = Vec::new(&c.env);
    batch.push_back((bob.clone(), 0));
    assert!(c.token.try_transfer_batch(&c.admin, &batch).is_err());
    assert_eq!(c.token.balance(&c.admin), 1_000);
    assert_eq!(c.token.balance(&bob), 0);
}

/// A zero entry after a valid one reverts the whole batch atomically.
#[test]
fn test_transfer_batch_zero_entry_reverts_earlier_entries() {
    let c = ctx(1_000);
    let bob = approved(&c);
    let carol = approved(&c);
    let mut batch: Vec<(Address, i128)> = Vec::new(&c.env);
    batch.push_back((bob.clone(), 100));
    batch.push_back((carol, 0));
    assert!(c.token.try_transfer_batch(&c.admin, &batch).is_err());
    assert_eq!(c.token.balance(&c.admin), 1_000);
    assert_eq!(c.token.balance(&bob), 0);
}

/// A negative amount is rejected like zero.
#[test]
fn test_transfer_batch_negative_amount_rejected() {
    let c = ctx(1_000);
    let bob = approved(&c);
    let mut batch: Vec<(Address, i128)> = Vec::new(&c.env);
    batch.push_back((bob, -1));
    assert!(c.token.try_transfer_batch(&c.admin, &batch).is_err());
}

// ---- issue #526: unauthorised / unprivileged caller ----

/// `transfer_batch` is deliberately open to any compliant token holder: it
/// requires no admin or guardian role, only that `from` authorises the call
/// and moves its own balance. An ordinary holder can batch-send.
#[test]
fn test_transfer_batch_open_to_unprivileged_compliant_holder() {
    let c = ctx(1_000);
    let alice = approved(&c);
    let bob = approved(&c);
    c.token.mint(&c.admin, &alice, &500);
    let mut batch: Vec<(Address, i128)> = Vec::new(&c.env);
    batch.push_back((bob.clone(), 200));
    c.token.transfer_batch(&alice, &batch);
    assert_eq!(c.token.balance(&alice), 300);
    assert_eq!(c.token.balance(&bob), 200);
    // Admin balance is untouched: the caller can only spend its own funds.
    assert_eq!(c.token.balance(&c.admin), 1_000);
}

/// The call is authorised against `from`, never against the admin.
#[test]
fn test_transfer_batch_requires_auth_of_from() {
    let c = ctx(1_000);
    let alice = approved(&c);
    let bob = approved(&c);
    c.token.mint(&c.admin, &alice, &50);
    let mut batch: Vec<(Address, i128)> = Vec::new(&c.env);
    batch.push_back((bob, 10));
    c.token.transfer_batch(&alice, &batch);
    let auths = c.env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths.get(0).unwrap().0, alice);
}

/// A caller outside the compliance allowlist is rejected (#7), even if it
/// holds a balance.
#[test]
fn test_transfer_batch_noncompliant_caller_rejected() {
    let c = ctx(1_000);
    let alice = approved(&c);
    let bob = approved(&c);
    c.token.mint(&c.admin, &alice, &50);
    c.compliance.suspend(&c.admin, &alice);
    let mut batch: Vec<(Address, i128)> = Vec::new(&c.env);
    batch.push_back((bob.clone(), 10));
    assert!(c.token.try_transfer_batch(&alice, &batch).is_err());
    assert_eq!(c.token.balance(&alice), 50);
    assert_eq!(c.token.balance(&bob), 0);
}

/// A compliant caller with no balance cannot draw on anyone else's funds:
/// the batch reverts with `InsufficientBalance` (#4).
#[test]
fn test_transfer_batch_caller_cannot_spend_others_balance() {
    let c = ctx(1_000);
    let alice = approved(&c);
    let bob = approved(&c);
    let mut batch: Vec<(Address, i128)> = Vec::new(&c.env);
    batch.push_back((bob.clone(), 1));
    assert!(c.token.try_transfer_batch(&alice, &batch).is_err());
    assert_eq!(c.token.balance(&c.admin), 1_000);
    assert_eq!(c.token.balance(&bob), 0);
}
