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
fn test_version() {
    let (_env, client, _admin) = setup();
    assert_eq!(client.version(), VERSION);
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
#[should_panic(expected = "Error(Contract, #5)")]
fn test_non_admin_add_to_allowlist_is_unauthorized() {
    // Issue #52: a non-admin caller must receive Unauthorized, not silently succeed.
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ComplianceContract, ());
    let client = ComplianceContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let non_admin = Address::generate(&env);
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    // non_admin is not the stored admin → must panic Unauthorized (#5).
    client.add_to_allowlist(&non_admin, &user, &us, &0);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_suspend_missing_record_rejected() {
    let (env, client, admin) = setup();
    let ghost = Address::generate(&env);
    client.suspend(&admin, &ghost);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_get_admin_before_init_panics_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ComplianceContract, ());
    let client = ComplianceContractClient::new(&env, &contract_id);
    // Contract is not initialized — get_admin must panic with NotInitialized (#2).
    client.get_admin();
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_invalid_jurisdiction_rejected() {
    // Issue #47: non-ISO-3166 jurisdiction codes must panic InvalidJurisdiction (#6).
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    // "United States" is not a valid 2-letter code.
    client.add_to_allowlist(&admin, &user, &String::from_str(&env, "United States"), &0);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_empty_jurisdiction_rejected() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    client.add_to_allowlist(&admin, &user, &String::from_str(&env, ""), &0);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_single_char_jurisdiction_rejected() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    client.add_to_allowlist(&admin, &user, &String::from_str(&env, "U"), &0);
}

#[test]
fn test_lowercase_jurisdiction_normalized() {
    // Issue #47: lowercase input "us" must be normalised to "US" and accepted.
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    client.add_to_allowlist(&admin, &user, &String::from_str(&env, "us"), &0);
    assert!(client.is_allowed(&user));
    assert_eq!(
        client.get_record(&user).unwrap().jurisdiction,
        String::from_str(&env, "US")
    );
}

#[test]
fn test_block_jurisdiction_denies_all_approved_in_jurisdiction() {
    // Issue #206: blocking a jurisdiction denies every approved address sharing
    // it at once, and unblocking restores all of them.
    let (env, client, admin) = setup();
    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let ir = String::from_str(&env, "IR");
    client.add_to_allowlist(&admin, &user_a, &ir, &0);
    client.add_to_allowlist(&admin, &user_b, &ir, &0);
    assert!(client.is_allowed(&user_a));
    assert!(client.is_allowed(&user_b));

    client.block_jurisdiction(&admin, &ir);
    assert!(!client.is_allowed(&user_a));
    assert!(!client.is_allowed(&user_b));

    client.unblock_jurisdiction(&admin, &ir);
    assert!(client.is_allowed(&user_a));
    assert!(client.is_allowed(&user_b));
}

#[test]
fn test_remove_missing_record_fails_typed_error_and_leaves_allowlist_untouched() {
    // Issue #205: `remove` on an address with no record fails RecordNotFound
    // and leaves the allowlist untouched.
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    client.add_to_allowlist(&admin, &user, &us, &0);

    let ghost = Address::generate(&env);
    let result = client.try_remove(&admin, &ghost);
    assert_eq!(result, Err(Ok(Error::RecordNotFound)));

    let list = client.get_allowlist();
    assert_eq!(list.len(), 1);
    assert_eq!(list.get(0).unwrap(), user);
}

#[test]
fn test_reapproving_suspended_address_restores_is_allowed() {
    // Issue #204: re-approving a suspended address flips status back to
    // Approved, restores is_allowed, and does not duplicate get_allowlist.
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    client.add_to_allowlist(&admin, &user, &us, &0);
    client.suspend(&admin, &user);
    assert!(!client.is_allowed(&user));
    assert_eq!(
        client.get_record(&user).unwrap().status,
        ComplianceStatus::Suspended
    );

    client.add_to_allowlist(&admin, &user, &us, &0);
    assert!(client.is_allowed(&user));
    assert_eq!(
        client.get_record(&user).unwrap().status,
        ComplianceStatus::Approved
    );
    assert_eq!(client.get_allowlist().len(), 1);
}

#[test]
fn test_expires_at_boundary_treated_as_expired() {
    // Issue #203: `is_allowed` uses `now >= record.expires_at`, so the ledger
    // equal to expires_at must be treated as expired, one before must not.
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    env.ledger().with_mut(|l| l.sequence_number = 10);
    client.add_to_allowlist(&admin, &user, &us, &100);

    env.ledger().with_mut(|l| l.sequence_number = 99);
    assert!(client.is_allowed(&user));

    env.ledger().with_mut(|l| l.sequence_number = 100);
    assert!(!client.is_allowed(&user));
}
