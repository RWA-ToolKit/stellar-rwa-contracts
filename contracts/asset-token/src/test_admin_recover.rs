#![cfg(test)]
//! Tests for the narrowly scoped admin recovery path (issue #733).

use super::*;
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{
    testutils::Address as _, testutils::Events, Address, Env, IntoVal, String, Symbol,
};

struct Setup {
    env: Env,
    token: AssetTokenContractClient<'static>,
    compliance: ComplianceContractClient<'static>,
    admin: Address,
    holder: Address,
}

fn setup() -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    let admin = Address::generate(&env);
    let holder = Address::generate(&env);
    compliance.initialize(&admin);
    compliance.add_to_allowlist(&admin, &admin, &String::from_str(&env, "US"), &0);
    compliance.add_to_allowlist(&admin, &holder, &String::from_str(&env, "US"), &0);

    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);
    token.initialize(
        &admin,
        &String::from_str(&env, "Recovery Asset"),
        &String::from_str(&env, "RCV"),
        &String::from_str(&env, "real_estate"),
        &1_000,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "desc"),
        &100_000,
    );
    token.transfer(&admin, &holder, &300);
    Setup {
        env,
        token,
        compliance,
        admin,
        holder,
    }
}

#[test]
fn test_admin_recover_to_unallowlisted_treasury() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    // The destination is deliberately NOT allowlisted.
    assert!(!s.compliance.is_allowed(&treasury));
    s.token.admin_recover(&s.admin, &s.holder, &treasury, &200);

    assert_eq!(s.token.balance(&s.holder), 100);
    assert_eq!(s.token.balance(&treasury), 200);
    assert_eq!(s.token.balance(&s.admin), 700);

    // A distinct audit event is emitted rather than the normal transfer event.
    let recover_topic: soroban_sdk::Val = Symbol::new(&s.env, "recover").into_val(&s.env);
    let events = s.env.events().all();
    assert!(events
        .iter()
        .any(|(_, topics, _)| topics.get(0) == Some(recover_topic.clone())));
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_normal_transfer_to_unallowlisted_treasury_still_fails() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.token.transfer(&s.holder, &treasury, &1);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_non_admin_cannot_recover() {
    let s = setup();
    let stranger = Address::generate(&s.env);
    let treasury = Address::generate(&s.env);
    s.token.admin_recover(&stranger, &s.holder, &treasury, &100);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_recover_blocked_while_paused() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.token.pause(&s.admin);
    s.token.admin_recover(&s.admin, &s.holder, &treasury, &100);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_recover_cannot_seize_more_than_balance() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.token.admin_recover(&s.admin, &s.holder, &treasury, &301);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_recover_rejects_non_positive_amount() {
    let s = setup();
    let treasury = Address::generate(&s.env);
    s.token.admin_recover(&s.admin, &s.holder, &treasury, &0);
}
