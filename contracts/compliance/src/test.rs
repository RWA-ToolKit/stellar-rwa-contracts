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

// Issue #341: pin the exact boundary at which a KYC approval lapses.
// Semantics: `expires_at` is exclusive — the record is valid through
// ledger `expires_at - 1`, and is expired starting at ledger `expires_at`
// itself (not one ledger after it).
#[test]
fn test_expiry_boundary_one_before_is_allowed() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    env.ledger().with_mut(|l| l.sequence_number = 10);
    client.add_to_allowlist(&admin, &user, &us, &100);
    env.ledger().with_mut(|l| l.sequence_number = 99);
    assert!(client.is_allowed(&user));
}

#[test]
fn test_expiry_boundary_exactly_at_expiry_is_expired() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    env.ledger().with_mut(|l| l.sequence_number = 10);
    client.add_to_allowlist(&admin, &user, &us, &100);
    env.ledger().with_mut(|l| l.sequence_number = 100);
    assert!(!client.is_allowed(&user));
}

#[test]
fn test_expiry_boundary_one_after_is_expired() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    env.ledger().with_mut(|l| l.sequence_number = 10);
    client.add_to_allowlist(&admin, &user, &us, &100);
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

/// Issue #61: re-adding an already-approved address must refresh the stored
/// record (jurisdiction/expiry) rather than leaving stale values behind.
#[test]
fn test_readd_refreshes_existing_record() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    client.add_to_allowlist(&admin, &user, &String::from_str(&env, "US"), &1000);
    client.add_to_allowlist(&admin, &user, &String::from_str(&env, "KE"), &2000);

    let rec = client.get_record(&user).unwrap();
    assert_eq!(rec.jurisdiction, String::from_str(&env, "KE"));
    assert_eq!(rec.expires_at, 2000);
    // Re-adding must not duplicate the allowlist entry.
    assert_eq!(client.get_allowlist().len(), 1);
}

/// Issue #88: suspending an address must not remove it from the allowlist
/// index; the record stays but its status flips to Suspended.
#[test]
fn test_suspend_keeps_allowlist_entry() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    client.add_to_allowlist(&admin, &user, &String::from_str(&env, "US"), &0);
    client.suspend(&admin, &user);

    assert_eq!(client.get_allowlist().len(), 1);
    assert!(client.get_record(&user).is_some());
    assert!(!client.is_allowed(&user));
}

/// Issue #102: blocking a jurisdiction must not affect addresses in other
/// jurisdictions.
#[test]
fn test_block_jurisdiction_isolated_to_that_jurisdiction() {
    let (env, client, admin) = setup();
    let blocked_user = Address::generate(&env);
    let ok_user = Address::generate(&env);
    let ir = String::from_str(&env, "IR");
    let us = String::from_str(&env, "US");
    client.add_to_allowlist(&admin, &blocked_user, &ir, &0);
    client.add_to_allowlist(&admin, &ok_user, &us, &0);

    client.block_jurisdiction(&admin, &ir);

    assert!(!client.is_allowed(&blocked_user));
    assert!(client.is_allowed(&ok_user));
}

/// Issue #120: an address whose KYC expires exactly at the current ledger
/// sequence must be treated as expired (boundary is exclusive).
#[test]
fn test_expiry_boundary_is_exclusive() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    env.ledger().with_mut(|l| l.sequence_number = 10);
    client.add_to_allowlist(&admin, &user, &String::from_str(&env, "US"), &100);

    // At exactly the expiry ledger, the record is no longer valid.
    env.ledger().with_mut(|l| l.sequence_number = 100);
    assert!(!client.is_allowed(&user));
}

/// Issue #141: removing an address must also drop it from the allowlist index
/// so subsequent length checks reflect the removal.
#[test]
fn test_remove_updates_allowlist_length() {
    let (env, client, admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let us = String::from_str(&env, "US");
    client.add_to_allowlist(&admin, &a, &us, &0);
    client.add_to_allowlist(&admin, &b, &us, &0);
    assert_eq!(client.get_allowlist().len(), 2);

    client.remove(&admin, &a);
    assert_eq!(client.get_allowlist().len(), 1);
    assert!(!client.is_allowed(&a));
    assert!(client.is_allowed(&b));
}

/// Issue #163: unblocking a jurisdiction that was never blocked must be a
/// no-op and must not panic.
#[test]
fn test_unblock_unblocked_jurisdiction_is_noop() {
    let (env, client, admin) = setup();
    let us = String::from_str(&env, "US");
    assert!(!client.is_jurisdiction_blocked(&us));
    client.unblock_jurisdiction(&admin, &us);
    assert!(!client.is_jurisdiction_blocked(&us));
}

/// Issue #201: a suspended address must remain suspended after an unrelated
/// allowlist mutation on a different address.
#[test]
fn test_suspend_survives_unrelated_mutation() {
    let (env, client, admin) = setup();
    let suspended = Address::generate(&env);
    let other = Address::generate(&env);
    let us = String::from_str(&env, "US");
    client.add_to_allowlist(&admin, &suspended, &us, &0);
    client.suspend(&admin, &suspended);

    client.add_to_allowlist(&admin, &other, &us, &0);

    assert_eq!(
        client.get_record(&suspended).unwrap().status,
        ComplianceStatus::Suspended
    );
    assert!(!client.is_allowed(&suspended));
}

/// Issue #244: get_record for an unknown address must return None rather than
/// panicking.
#[test]
fn test_get_record_unknown_returns_none() {
    let (env, client, _admin) = setup();
    let unknown = Address::generate(&env);
    assert!(client.get_record(&unknown).is_none());
}

/// Issue #277: is_allowed must return false for an address that was never
/// added, even when other addresses are approved.
#[test]
fn test_is_allowed_false_for_unknown_with_others_present() {
    let (env, client, admin) = setup();
    let known = Address::generate(&env);
    let unknown = Address::generate(&env);
    client.add_to_allowlist(&admin, &known, &String::from_str(&env, "US"), &0);

    assert!(client.is_allowed(&known));
    assert!(!client.is_allowed(&unknown));
}

/// Issue #318: a jurisdiction code with more than two characters must be
/// rejected as InvalidJurisdiction (#6).
#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_three_char_jurisdiction_rejected() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    client.add_to_allowlist(&admin, &user, &String::from_str(&env, "USA"), &0);
}

/// Issue #333: mixed-case jurisdiction input must be normalised to uppercase
/// before being stored.
#[test]
fn test_mixed_case_jurisdiction_normalized() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    client.add_to_allowlist(&admin, &user, &String::from_str(&env, "uS"), &0);
    assert_eq!(
        client.get_record(&user).unwrap().jurisdiction,
        String::from_str(&env, "US")
    );
}

/// Issue #356: removing an address that was never added must reject with
/// RecordNotFound rather than silently succeeding — `remove` intentionally
/// requires an existing record (see `Error::RecordNotFound`).
#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_remove_unknown_address_is_noop() {
    let (env, client, admin) = setup();
    let ghost = Address::generate(&env);
    client.remove(&admin, &ghost);
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

    // Prune expired records. max_records = 0 means unbounded, matching the
    // pre-#333 behaviour for a small allowlist: a single call finishes the
    // whole pass and reports 0 remaining.
    let remaining = client.prune_expired(&admin, &0);
    assert_eq!(remaining, 0);

    // Verify get_allowlist no longer contains the expired user
    let list = client.get_allowlist();
    assert_eq!(list.len(), 1);
    assert_eq!(list.get(0).unwrap(), user_persist);

    // Verify get_record returns None for the pruned user
    assert!(client.get_record(&user_expire).is_none());
    assert!(client.get_record(&user_persist).is_some());
}

#[test]
fn test_add_to_allowlist_batch_admits_several_addresses() {
    // Issue #338: several addresses can be admitted in one transaction, with
    // the same validation/normalization as the single-address path.
    let (env, client, admin) = setup();
    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let us = String::from_str(&env, "us"); // lowercase, mirrors normalize path
    let ke = String::from_str(&env, "KE");
    let mut entries: Vec<AllowlistEntry> = Vec::new(&env);
    entries.push_back(AllowlistEntry {
        address: user_a.clone(),
        jurisdiction: us.clone(),
        expires_at: 0,
    });
    entries.push_back(AllowlistEntry {
        address: user_b.clone(),
        jurisdiction: ke.clone(),
        expires_at: 0,
    });

    client.add_to_allowlist_batch(&admin, &entries);

    assert!(client.is_allowed(&user_a));
    assert!(client.is_allowed(&user_b));
    assert_eq!(
        client.get_record(&user_a).unwrap().jurisdiction,
        String::from_str(&env, "US")
    );
    assert_eq!(client.get_allowlist().len(), 2);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_add_to_allowlist_batch_partial_failure_reverts_whole_batch() {
    // Issue #338: a failure in one entry must not silently skip that entry
    // while committing the others — the whole call reverts.
    let (env, client, admin) = setup();
    let user_a = Address::generate(&env);
    let user_bad = Address::generate(&env);
    let us = String::from_str(&env, "US");
    env.ledger().with_mut(|l| l.sequence_number = 500);
    let mut entries: Vec<AllowlistEntry> = Vec::new(&env);
    entries.push_back(AllowlistEntry {
        address: user_a.clone(),
        jurisdiction: us.clone(),
        expires_at: 0,
    });
    // Second entry has an expiry already in the past: identical to what
    // add_to_allowlist rejects with Error::InvalidExpiry (#4).
    entries.push_back(AllowlistEntry {
        address: user_bad.clone(),
        jurisdiction: us.clone(),
        expires_at: 100,
    });

    client.add_to_allowlist_batch(&admin, &entries);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_add_to_allowlist_batch_non_admin_rejected() {
    let (env, client, _admin) = setup();
    let impostor = Address::generate(&env);
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    let mut entries: Vec<AllowlistEntry> = Vec::new(&env);
    entries.push_back(AllowlistEntry {
        address: user,
        jurisdiction: us,
        expires_at: 0,
    });
    client.add_to_allowlist_batch(&impostor, &entries);
}

#[test]
fn test_prune_expired_respects_bound_and_reports_remaining() {
    // Issue #333: prune_expired must accept a bound on how many records a
    // single call processes, and report how many are left to examine.
    let (env, client, admin) = setup();
    let us = String::from_str(&env, "US");
    let mut users: Vec<Address> = Vec::new(&env);
    for _ in 0..5 {
        users.push_back(Address::generate(&env));
    }
    env.ledger().with_mut(|l| l.sequence_number = 10);
    for u in users.iter() {
        client.add_to_allowlist(&admin, &u, &us, &100);
    }
    env.ledger().with_mut(|l| l.sequence_number = 101);
    assert_eq!(client.get_allowlist().len(), 5);

    // First call only examines 2 of the 5 expired entries.
    let remaining = client.prune_expired(&admin, &2);
    assert_eq!(remaining, 3);
    assert_eq!(client.get_allowlist().len(), 3);

    // Second call examines the rest.
    let remaining = client.prune_expired(&admin, &2);
    assert_eq!(remaining, 1);
    assert_eq!(client.get_allowlist().len(), 1);

    // Final call clears the last one; nothing left to examine.
    let remaining = client.prune_expired(&admin, &2);
    assert_eq!(remaining, 0);
    assert_eq!(client.get_allowlist().len(), 0);
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
    let mut users = Vec::new(&env);
    for _i in 0..250 {
        let user = Address::generate(&env);
        users.push_back(user.clone());
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

// Issue #371: Test that state survives a TTL boundary (ledger advance).
#[test]
fn test_admin_state_survives_ttl_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);

    let contract_id = env.register(ComplianceContract, ());
    let client = ComplianceContractClient::new(&env, &contract_id);

    // Initialize the contract (sets Admin key, bumps TTL)
    client.initialize(&admin);

    // Verify Admin is readable immediately
    assert_eq!(client.get_admin(), admin);

    // Advance the ledger by a large amount (simulate time passing)
    // This tests that the TTL bump extends past this advance.
    // The bump amount is typically 30 days of ledgers (~500K ledgers).
    // Advancing by a significant amount and verifying the key survives
    // ensures the TTL was properly extended.
    env.ledger().set_sequence_number(500_000);

    // Verify Admin key still exists after ledger advance
    assert_eq!(
        client.get_admin(),
        admin,
        "Admin state must survive TTL boundary"
    );
}

// Issue #371: Test that allowlist entries survive TTL boundary.
#[test]
fn test_allowlist_entries_survive_ttl_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);

    let contract_id = env.register(ComplianceContract, ());
    let client = ComplianceContractClient::new(&env, &contract_id);

    // Initialize and add users
    client.initialize(&admin);
    let us = String::from_str(&env, "US");
    client.add_to_allowlist(&admin, &user1, &us, &0);
    client.add_to_allowlist(&admin, &user2, &us, &0);

    // Verify entries exist
    let initial_list = client.get_allowlist();
    assert_eq!(initial_list.len(), 2);

    // Advance ledger past TTL threshold
    env.ledger().set_sequence_number(500_000);

    // Verify entries still exist after ledger advance
    let after_ttl_list = client.get_allowlist();
    assert_eq!(
        after_ttl_list.len(),
        2,
        "Allowlist entries must survive TTL boundary"
    );
    assert!(client.is_allowed(&user1));
    assert!(client.is_allowed(&user2));
}

// ---- admin handover (issue #4) ----

#[test]
fn test_propose_accept_admin_moves_role_only_on_acceptance() {
    let (env, client, admin) = setup();
    let successor = Address::generate(&env);

    client.propose_admin(&admin, &successor);
    // Role must not move until accepted.
    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_pending_admin(), Some(successor.clone()));

    client.accept_admin(&successor);
    assert_eq!(client.get_admin(), successor);
    assert_eq!(client.get_pending_admin(), None);

    // The old admin has lost its privileges.
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    assert_eq!(
        client.try_add_to_allowlist(&admin, &user, &us, &0),
        Err(Ok(Error::Unauthorized.into()))
    );
    // The new admin can act.
    client.add_to_allowlist(&successor, &user, &us, &0);
    assert!(client.is_allowed(&user));
}

#[test]
fn test_cancel_admin_proposal_by_current_admin() {
    let (env, client, admin) = setup();
    let successor = Address::generate(&env);

    client.propose_admin(&admin, &successor);
    client.cancel_admin_proposal(&admin);
    assert_eq!(client.get_pending_admin(), None);

    // The cancelled successor can no longer accept.
    assert_eq!(
        client.try_accept_admin(&successor),
        Err(Ok(Error::NoPendingAdmin.into()))
    );
    // The admin is unchanged.
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_cancel_admin_proposal_with_nothing_pending_fails() {
    let (_env, client, admin) = setup();
    assert_eq!(
        client.try_cancel_admin_proposal(&admin),
        Err(Ok(Error::NoPendingAdmin.into()))
    );
}

#[test]
fn test_non_admin_cannot_propose_admin() {
    let (env, client, _admin) = setup();
    let non_admin = Address::generate(&env);
    let successor = Address::generate(&env);
    assert_eq!(
        client.try_propose_admin(&non_admin, &successor),
        Err(Ok(Error::Unauthorized.into()))
    );
}

#[test]
fn test_non_admin_cannot_cancel_admin_proposal() {
    let (env, client, admin) = setup();
    let non_admin = Address::generate(&env);
    let successor = Address::generate(&env);
    client.propose_admin(&admin, &successor);
    assert_eq!(
        client.try_cancel_admin_proposal(&non_admin),
        Err(Ok(Error::Unauthorized.into()))
    );
}

#[test]
fn test_only_proposed_successor_can_accept() {
    let (env, client, admin) = setup();
    let successor = Address::generate(&env);
    let impostor = Address::generate(&env);

    client.propose_admin(&admin, &successor);
    assert_eq!(
        client.try_accept_admin(&impostor),
        Err(Ok(Error::Unauthorized.into()))
    );
    // Role is unaffected by the failed attempt.
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_accept_admin_with_no_pending_proposal_fails() {
    let (env, client, _admin) = setup();
    let stranger = Address::generate(&env);
    assert_eq!(
        client.try_accept_admin(&stranger),
        Err(Ok(Error::NoPendingAdmin.into()))
    );
}

#[test]
fn test_propose_admin_can_be_re_proposed_to_a_different_successor() {
    let (env, client, admin) = setup();
    let first = Address::generate(&env);
    let second = Address::generate(&env);

    client.propose_admin(&admin, &first);
    client.propose_admin(&admin, &second);
    assert_eq!(client.get_pending_admin(), Some(second.clone()));

    // The first proposed successor can no longer accept.
    assert_eq!(
        client.try_accept_admin(&first),
        Err(Ok(Error::Unauthorized.into()))
    );
    client.accept_admin(&second);
    assert_eq!(client.get_admin(), second);
}

// ---- issue: expires_at = 0 sentinel never lapses ----

#[test]
fn test_zero_expiry_never_lapses() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    client.add_to_allowlist(&admin, &user, &us, &0);
    assert!(client.is_allowed(&user));

    // Advance the ledger sequence far past any realistic expiry and confirm
    // a zero-expiry record is still valid.
    env.ledger().with_mut(|l| l.sequence_number = 10_000_000);
    assert!(client.is_allowed(&user));
    let rec = client.get_record(&user).unwrap();
    assert_eq!(rec.expires_at, 0);
}

// ---- issue: get_allowlist_count backed by a maintained counter ----

#[test]
fn test_allowlist_count_tracks_add_remove_suspend() {
    let (env, client, admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let us = String::from_str(&env, "US");

    assert_eq!(client.get_allowlist_count(), 0);

    client.add_to_allowlist(&admin, &a, &us, &0);
    assert_eq!(client.get_allowlist_count(), 1);

    client.add_to_allowlist(&admin, &b, &us, &0);
    assert_eq!(client.get_allowlist_count(), 2);

    // Suspend does not remove the address from the allowlist, so the
    // maintained counter must not drift.
    client.suspend(&admin, &a);
    assert_eq!(client.get_allowlist_count(), 2);
    assert_eq!(client.get_allowlist_count(), client.get_allowlist().len());

    // Re-approving an existing (suspended) address is not a fresh append.
    client.add_to_allowlist(&admin, &a, &us, &0);
    assert_eq!(client.get_allowlist_count(), 2);

    client.remove(&admin, &a);
    assert_eq!(client.get_allowlist_count(), 1);
    assert_eq!(client.get_allowlist_count(), client.get_allowlist().len());

    client.remove(&admin, &b);
    assert_eq!(client.get_allowlist_count(), 0);
}

// ---- issue: reinstate a suspended address preserving KYC metadata ----

#[test]
fn test_reinstate_preserves_jurisdiction_and_verified_at() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let ke = String::from_str(&env, "KE");

    env.ledger().with_mut(|l| l.sequence_number = 50);
    client.add_to_allowlist(&admin, &user, &ke, &0);
    let original = client.get_record(&user).unwrap();

    env.ledger().with_mut(|l| l.sequence_number = 60);
    client.suspend(&admin, &user);
    assert!(!client.is_allowed(&user));
    assert_eq!(client.status_of(&user), Some(ComplianceStatus::Suspended));

    env.ledger().with_mut(|l| l.sequence_number = 70);
    client.reinstate(&admin, &user);

    assert!(client.is_allowed(&user));
    let reinstated = client.get_record(&user).unwrap();
    assert_eq!(reinstated.status, ComplianceStatus::Approved);
    assert_eq!(reinstated.jurisdiction, original.jurisdiction);
    assert_eq!(reinstated.verified_at, original.verified_at);
    assert_eq!(reinstated.expires_at, original.expires_at);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_reinstate_non_suspended_rejected() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    client.add_to_allowlist(&admin, &user, &us, &0);
    client.reinstate(&admin, &user);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_reinstate_missing_record_rejected() {
    let (env, client, admin) = setup();
    let ghost = Address::generate(&env);
    client.reinstate(&admin, &ghost);
}

// ---- issue: paginated allowlist listing ----

#[test]
fn test_get_allowlist_page_offset_and_limit() {
    let (env, client, admin) = setup();
    let us = String::from_str(&env, "US");
    let mut addrs: Vec<Address> = Vec::new(&env);
    for _ in 0..5 {
        let a = Address::generate(&env);
        client.add_to_allowlist(&admin, &a, &us, &0);
        addrs.push_back(a);
    }

    let page1 = client.get_allowlist_page(&0, &2);
    assert_eq!(page1.len(), 2);
    let page2 = client.get_allowlist_page(&2, &2);
    assert_eq!(page2.len(), 2);
    // Final partial page.
    let page3 = client.get_allowlist_page(&4, &2);
    assert_eq!(page3.len(), 1);
    assert_eq!(page3.get(0).unwrap(), addrs.get(4).unwrap());

    // Past the end returns empty.
    let page4 = client.get_allowlist_page(&5, &2);
    assert_eq!(page4.len(), 0);
}

#[test]
fn test_get_allowlist_page_limit_clamped_to_max() {
    let (env, client, admin) = setup();
    let us = String::from_str(&env, "US");
    let a = Address::generate(&env);
    client.add_to_allowlist(&admin, &a, &us, &0);

    // limit=0 and an oversized limit both clamp to MAX_ALLOWLIST_PAGE_SIZE,
    // which is still satisfied by whatever is actually on the allowlist.
    let via_zero = client.get_allowlist_page(&0, &0);
    let via_huge = client.get_allowlist_page(&0, &(MAX_ALLOWLIST_PAGE_SIZE + 1000));
    assert_eq!(via_zero.len(), 1);
    assert_eq!(via_huge.len(), 1);
}

// ---- issue: is_allowed must explicitly reject every non-Approved status ----

#[test]
fn test_is_allowed_rejects_pending_status() {
    // `Pending` has no public setter today, so we write the record directly
    // via storage (as a future KYC-submission workflow would) and assert
    // `is_allowed` still rejects it, not just "no record found".
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    env.as_contract(&client.address, || {
        env.storage().persistent().set(
            &DataKey::Record(user.clone()),
            &KycRecord {
                address: user.clone(),
                status: ComplianceStatus::Pending,
                jurisdiction: us,
                verified_at: 0,
                expires_at: 0,
            },
        );
    });
    assert_eq!(client.status_of(&user), Some(ComplianceStatus::Pending));
    assert!(!client.is_allowed(&user));
}

#[test]
fn test_is_allowed_rejects_rejected_status() {
    let (env, client, _admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    env.as_contract(&client.address, || {
        env.storage().persistent().set(
            &DataKey::Record(user.clone()),
            &KycRecord {
                address: user.clone(),
                status: ComplianceStatus::Rejected,
                jurisdiction: us,
                verified_at: 0,
                expires_at: 0,
            },
        );
    });
    assert_eq!(client.status_of(&user), Some(ComplianceStatus::Rejected));
    assert!(!client.is_allowed(&user));
}

#[test]
fn test_is_allowed_rejects_suspended_status() {
    // Suspended is already exercised via `test_suspend_blocks_transfer`, but
    // this asserts it alongside its Pending/Rejected siblings so all three
    // non-Approved statuses are covered by name, not just generically.
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    client.add_to_allowlist(&admin, &user, &us, &0);
    client.suspend(&admin, &user);
    assert_eq!(client.status_of(&user), Some(ComplianceStatus::Suspended));
    assert!(!client.is_allowed(&user));
}

// ---- issue: expose the blocked-jurisdiction set as a direct read ----

#[test]
fn test_get_blocked_jurisdictions_round_trip() {
    let (env, client, admin) = setup();
    let ir = String::from_str(&env, "IR");
    let kp = String::from_str(&env, "KP");

    assert_eq!(client.get_blocked_jurisdictions().len(), 0);

    client.block_jurisdiction(&admin, &ir);
    let after_first = client.get_blocked_jurisdictions();
    assert_eq!(after_first.len(), 1);
    assert_eq!(after_first.get(0).unwrap(), ir);

    client.block_jurisdiction(&admin, &kp);
    let after_second = client.get_blocked_jurisdictions();
    assert_eq!(after_second.len(), 2);

    // Blocking an already-blocked jurisdiction again must not duplicate it.
    client.block_jurisdiction(&admin, &ir);
    assert_eq!(client.get_blocked_jurisdictions().len(), 2);

    client.unblock_jurisdiction(&admin, &ir);
    let after_unblock = client.get_blocked_jurisdictions();
    assert_eq!(after_unblock.len(), 1);
    assert_eq!(after_unblock.get(0).unwrap(), kp);
    assert!(!client.is_jurisdiction_blocked(&ir));
    assert!(client.is_jurisdiction_blocked(&kp));
}

// ---- minimum holding period (issue #454) ----

/// Happy path: an address whose holding period has fully elapsed passes.
#[test]
fn test_holding_period_satisfied_is_allowed() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");

    // Acquire at ledger 100, lock for 50 ledgers → unlocks at 150.
    env.ledger().with_mut(|l| l.sequence_number = 100);
    client.add_to_allowlist(&admin, &user, &us, &0);
    client.record_acquisition(&admin, &user);
    client.set_min_holding_period(&admin, &user, &50u64);

    // Still inside the lock-up window.
    env.ledger().with_mut(|l| l.sequence_number = 149);
    assert!(!client.is_allowed(&user));

    // Exactly at the unlock ledger.
    env.ledger().with_mut(|l| l.sequence_number = 150);
    assert!(client.is_allowed(&user));

    // Well after the unlock ledger.
    env.ledger().with_mut(|l| l.sequence_number = 200);
    assert!(client.is_allowed(&user));
}

/// Rejection path: transfer attempted before the holding period has elapsed.
#[test]
fn test_holding_period_not_yet_elapsed_is_denied() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");

    env.ledger().with_mut(|l| l.sequence_number = 100);
    client.add_to_allowlist(&admin, &user, &us, &0);
    client.record_acquisition(&admin, &user);
    client.set_min_holding_period(&admin, &user, &90u64);

    // Transfer attempted 50 ledgers in: should be denied.
    env.ledger().with_mut(|l| l.sequence_number = 150);
    assert!(!client.is_allowed(&user));
}

/// Clearing a holding period (min_ledgers = 0) removes the restriction.
#[test]
fn test_clear_holding_period_removes_restriction() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");

    env.ledger().with_mut(|l| l.sequence_number = 100);
    client.add_to_allowlist(&admin, &user, &us, &0);
    client.record_acquisition(&admin, &user);
    client.set_min_holding_period(&admin, &user, &500u64);

    // Inside the window.
    env.ledger().with_mut(|l| l.sequence_number = 200);
    assert!(!client.is_allowed(&user));

    // Admin clears the period.
    client.set_min_holding_period(&admin, &user, &0u64);
    assert!(client.is_allowed(&user));
    assert_eq!(client.get_min_holding_period(&user), None);
}

/// Fail-open: no `FirstAcquiredLedger` record means the holding period is
/// treated as already satisfied, so existing holders are not locked out.
#[test]
fn test_holding_period_without_acquisition_record_is_allowed() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");

    env.ledger().with_mut(|l| l.sequence_number = 100);
    client.add_to_allowlist(&admin, &user, &us, &0);
    // Deliberately skip record_acquisition.
    client.set_min_holding_period(&admin, &user, &500u64);

    // Should still be allowed because acquisition ledger is unknown.
    assert!(client.is_allowed(&user));
}

/// A second call to `record_acquisition` must not overwrite the first one:
/// the clock always starts at the *earliest* acquisition.
#[test]
fn test_record_acquisition_is_idempotent() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");

    env.ledger().with_mut(|l| l.sequence_number = 100);
    client.add_to_allowlist(&admin, &user, &us, &0);
    client.record_acquisition(&admin, &user);
    assert_eq!(client.get_first_acquired_ledger(&user), Some(100u64));

    // Simulate a top-up at a later ledger: the stored value must stay at 100.
    env.ledger().with_mut(|l| l.sequence_number = 200);
    client.record_acquisition(&admin, &user);
    assert_eq!(client.get_first_acquired_ledger(&user), Some(100u64));
}

/// An address with no holding period set is not affected by the check at all.
#[test]
fn test_no_holding_period_set_has_no_effect() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");

    env.ledger().with_mut(|l| l.sequence_number = 1);
    client.add_to_allowlist(&admin, &user, &us, &0);
    // No set_min_holding_period call at all.
    assert!(client.is_allowed(&user));
    assert_eq!(client.get_min_holding_period(&user), None);
}

/// Non-admin cannot set or record acquisition.
#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_set_min_holding_period_non_admin_rejected() {
    let (env, client, _admin) = setup();
    let impostor = Address::generate(&env);
    let user = Address::generate(&env);
    client.set_min_holding_period(&impostor, &user, &100u64);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_record_acquisition_non_admin_rejected() {
    let (env, client, _admin) = setup();
    let impostor = Address::generate(&env);
    let user = Address::generate(&env);
    client.record_acquisition(&impostor, &user);
}

/// Holding period is per-address: other addresses are unaffected.
#[test]
fn test_holding_period_does_not_affect_other_addresses() {
    let (env, client, admin) = setup();
    let locked = Address::generate(&env);
    let free = Address::generate(&env);
    let us = String::from_str(&env, "US");

    env.ledger().with_mut(|l| l.sequence_number = 100);
    client.add_to_allowlist(&admin, &locked, &us, &0);
    client.add_to_allowlist(&admin, &free, &us, &0);

    client.record_acquisition(&admin, &locked);
    client.set_min_holding_period(&admin, &locked, &200u64);

    env.ledger().with_mut(|l| l.sequence_number = 150);
    // `locked` is still inside the window.
    assert!(!client.is_allowed(&locked));
    // `free` has no holding period — must pass.
    assert!(client.is_allowed(&free));
}
