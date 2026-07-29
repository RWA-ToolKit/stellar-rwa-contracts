#![cfg(test)]
use super::*;
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Env};

fn setup() -> (Env, ComplianceContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ComplianceContract, ());
    let client = ComplianceContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

#[test]
fn test_initialize_sets_admin() {
    let (_env, client, admin) = setup();
    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_allowlist().len(), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_double_initialize_fails() {
    let (env, client, _admin) = setup();
    let other = Address::generate(&env);
    client.initialize(&other);
}

#[test]
fn test_add_to_allowlist_is_allowed() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    client.add_to_allowlist(&admin, &user, &us, &0);
    assert!(client.is_allowed(&user));
    assert_eq!(client.get_allowlist().len(), 1);
}

#[test]
fn test_unknown_address_not_allowed() {
    let (env, client, _admin) = setup();
    let stranger = Address::generate(&env);
    assert!(!client.is_allowed(&stranger));
    assert!(client.get_record(&stranger).is_none());
}

#[test]
fn test_get_record_fields() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let ke = String::from_str(&env, "KE");
    client.add_to_allowlist(&admin, &user, &ke, &1000);
    let rec = client.get_record(&user).unwrap();
    assert_eq!(rec.address, user);
    assert_eq!(rec.status, ComplianceStatus::Approved);
    assert_eq!(rec.jurisdiction, ke);
    assert_eq!(rec.expires_at, 1000);
}

#[test]
fn test_suspend_blocks_transfer() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    client.add_to_allowlist(&admin, &user, &us, &0);
    assert!(client.is_allowed(&user));
    client.suspend(&admin, &user);
    assert!(!client.is_allowed(&user));
    assert_eq!(
        client.get_record(&user).unwrap().status,
        ComplianceStatus::Suspended
    );
}

#[test]
fn test_remove_clears_record() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    client.add_to_allowlist(&admin, &user, &us, &0);
    client.remove(&admin, &user);
    assert!(!client.is_allowed(&user));
    assert!(client.get_record(&user).is_none());
    assert_eq!(client.get_allowlist().len(), 0);
}

#[test]
fn test_expired_kyc_not_allowed() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    env.ledger().with_mut(|l| l.sequence_number = 10);
    client.add_to_allowlist(&admin, &user, &us, &100);
    assert!(client.is_allowed(&user));
    env.ledger().with_mut(|l| l.sequence_number = 101);
    assert!(!client.is_allowed(&user));
}

#[test]
fn test_block_jurisdiction_denies_approved() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let ir = String::from_str(&env, "IR");
    client.add_to_allowlist(&admin, &user, &ir, &0);
    assert!(client.is_allowed(&user));
    client.block_jurisdiction(&admin, &ir);
    assert!(client.is_jurisdiction_blocked(&ir));
    assert!(!client.is_allowed(&user));
}

#[test]
fn test_unblock_jurisdiction_restores() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let ir = String::from_str(&env, "IR");
    client.add_to_allowlist(&admin, &user, &ir, &0);
    client.block_jurisdiction(&admin, &ir);
    assert!(!client.is_allowed(&user));
    client.unblock_jurisdiction(&admin, &ir);
    assert!(!client.is_jurisdiction_blocked(&ir));
    assert!(client.is_allowed(&user));
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_non_admin_rejected() {
    let (env, client, _admin) = setup();
    let impostor = Address::generate(&env);
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    client.add_to_allowlist(&impostor, &user, &us, &0);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_expiry_in_past_rejected() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    env.ledger().with_mut(|l| l.sequence_number = 500);
    client.add_to_allowlist(&admin, &user, &us, &100);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_suspend_missing_record_rejected() {
    let (env, client, admin) = setup();
    let ghost = Address::generate(&env);
    client.suspend(&admin, &ghost);
}

// Issue #56: suspend flips is_allowed to false and record shows Suspended
#[test]
fn test_suspend_flips_is_allowed_false() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    client.add_to_allowlist(&admin, &user, &String::from_str(&env, "US"), &0);
    assert!(client.is_allowed(&user));
    client.suspend(&admin, &user);
    assert!(!client.is_allowed(&user));
    assert_eq!(client.get_record(&user).unwrap().status, ComplianceStatus::Suspended);
}

// Issue #58: remove deletes record and removes address from allowlist
#[test]
fn test_remove_deletes_record_and_allowlist_entry() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    client.add_to_allowlist(&admin, &user, &String::from_str(&env, "US"), &0);
    client.remove(&admin, &user);
    assert!(client.get_record(&user).is_none());
    assert!(!client.get_allowlist().contains(&user));
}

// Issue #59: remove on unknown address panics RecordNotFound
#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_remove_missing_record_panics() {
    let (env, client, admin) = setup();
    client.remove(&admin, &Address::generate(&env));
}

// Issue #60: blocked jurisdiction fails is_allowed; unblock restores it
#[test]
fn test_blocked_jurisdiction_fails_is_allowed() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    client.add_to_allowlist(&admin, &user, &us, &0);
    client.block_jurisdiction(&admin, &us);
    assert!(!client.is_allowed(&user));
    client.unblock_jurisdiction(&admin, &us);
    assert!(client.is_allowed(&user));
}
