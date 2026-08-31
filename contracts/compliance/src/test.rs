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

/// Issue #305: get_admin success path was untested.
/// After initialize has run, get_admin must return exactly the address that
/// was passed to initialize — not a default, not a different address.
#[test]
fn test_get_admin_returns_correct_address_after_initialize() {
    let (env, client, admin) = setup();
    // The primary assertion: get_admin must echo back the exact admin address.
    assert_eq!(client.get_admin(), admin);

    // Confirm that a second, distinct address is NOT reported as the admin,
    // which would catch an implementation that ignores the stored value.
    let other = Address::generate(&env);
    assert_ne!(client.get_admin(), other);
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
fn test_status_of_distinguishes_unseen_from_approved_and_suspended() {
    // Issue #183: `status_of` must let callers tell "never seen" (`None`)
    // apart from a recorded status such as `Approved` or `Suspended`.
    let (env, client, admin) = setup();
    let stranger = Address::generate(&env);
    let user = Address::generate(&env);

    assert_eq!(client.status_of(&stranger), None);

    client.add_to_allowlist(&admin, &user, &String::from_str(&env, "US"), &0);
    assert_eq!(client.status_of(&user), Some(ComplianceStatus::Approved));

    client.suspend(&admin, &user);
    assert_eq!(client.status_of(&user), Some(ComplianceStatus::Suspended));
}

#[test]
fn test_get_allowlist_basic_membership() {
    // Issue #303: get_allowlist has zero test coverage.
    let (env, client, admin) = setup();
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let us = String::from_str(&env, "US");
    let de = String::from_str(&env, "DE");

    // Initially empty
    assert_eq!(client.get_allowlist().len(), 0);

    // After adding first user
    client.add_to_allowlist(&admin, &user1, &us, &0);
    let list = client.get_allowlist();
    assert_eq!(list.len(), 1);
    assert_eq!(list.get(0).unwrap(), user1);

    // After adding second user
    client.add_to_allowlist(&admin, &user2, &de, &0);
    let list = client.get_allowlist();
    assert_eq!(list.len(), 2);
    assert_eq!(list.get(0).unwrap(), user1);
    assert_eq!(list.get(1).unwrap(), user2);
}

#[test]
fn test_get_allowlist_after_removal() {
    // Issue #303: get_allowlist must reflect removal via remove().
    let (env, client, admin) = setup();
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let us = String::from_str(&env, "US");

    client.add_to_allowlist(&admin, &user1, &us, &0);
    client.add_to_allowlist(&admin, &user2, &us, &0);
    assert_eq!(client.get_allowlist().len(), 2);

    client.remove(&admin, &user1);
    let list = client.get_allowlist();
    assert_eq!(list.len(), 1);
    assert_eq!(list.get(0).unwrap(), user2);
}

#[test]
fn test_get_allowlist_page_rollover() {
    // Issue #303: get_allowlist must handle page rollover at ALLOWLIST_PAGE_SIZE (200).
    let (env, client, admin) = setup();
    let us = String::from_str(&env, "US");

    // Add 250 addresses to force page rollover (200 + 1 = 201 > ALLOWLIST_PAGE_SIZE)
    let mut users = Vec::new();
    for _i in 0..250 {
        let user = Address::generate(&env);
        users.push(user.clone());
        client.add_to_allowlist(&admin, &user, &us, &0);
    }

    // Verify all 250 are in the allowlist
    let allowlist = client.get_allowlist();
    assert_eq!(allowlist.len(), 250);

    // Verify the expected users are present (spot-check first, middle, and last)
    assert_eq!(allowlist.get(0).unwrap(), users.get(0).unwrap());
    assert_eq!(allowlist.get(125).unwrap(), users.get(125).unwrap());
    assert_eq!(allowlist.get(249).unwrap(), users.get(249).unwrap());
}

#[test]
fn test_re_approve_removed_address_single_page_slot() {
    // Issue #304: re-approving a removed address should get a single fresh page slot,
    // not duplicated across old and new slots.
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");

    // Add, remove, then re-add the same address
    client.add_to_allowlist(&admin, &user, &us, &0);
    let initial_list = client.get_allowlist();
    assert_eq!(initial_list.len(), 1);
    assert_eq!(initial_list.get(0).unwrap(), user);

    client.remove(&admin, &user);
    let after_remove = client.get_allowlist();
    assert_eq!(after_remove.len(), 0);

    // Re-approve the same address
    client.add_to_allowlist(&admin, &user, &us, &0);
    let after_readd = client.get_allowlist();
    assert_eq!(after_readd.len(), 1);
    assert_eq!(after_readd.get(0).unwrap(), user);
}

#[test]
fn test_prune_expired_removes_from_allowlist() {
    // Issue #307: prune_expired must remove expired addresses from get_allowlist.
    let (env, client, admin) = setup();
    let user_expire = Address::generate(&env);
    let user_persist = Address::generate(&env);
    let us = String::from_str(&env, "US");

    env.ledger().with_mut(|l| l.sequence_number = 10);
    client.add_to_allowlist(&admin, &user_expire, &us, &100);
    client.add_to_allowlist(&admin, &user_persist, &us, &0);
    assert_eq!(client.get_allowlist().len(), 2);

    // Advance ledger past expiry
    env.ledger().with_mut(|l| l.sequence_number = 101);

    // Verify the expired user is no longer is_allowed
    assert!(!client.is_allowed(&user_expire));
    assert!(client.is_allowed(&user_persist));

    // Prune expired records
    client.prune_expired(&admin);

    // Verify get_allowlist no longer contains the expired user
    let list = client.get_allowlist();
    assert_eq!(list.len(), 1);
    assert_eq!(list.get(0).unwrap(), user_persist);

    // Verify get_record returns None for the pruned user
    assert!(client.get_record(&user_expire).is_none());
    assert!(client.get_record(&user_persist).is_some());
}
