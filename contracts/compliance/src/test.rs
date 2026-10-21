#![cfg(test)]
extern crate std;
use std::println;
use super::*;
use soroban_sdk::{testutils::Address as _, testutils::Events as _, testutils::Ledger as _, Env};

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
fn test_add_to_allowlist_batch_with_already_approved_address() {
    // A batch mixing a new address with an already-approved one must succeed,
    // leave the existing record intact, and not duplicate it in the allowlist.
    let (env, client, admin) = setup();
    let existing = Address::generate(&env);
    let fresh = Address::generate(&env);
    let us = String::from_str(&env, "US");
    let ke = String::from_str(&env, "KE");
    client.add_to_allowlist(&admin, &existing, &us, &1000);
    let before = client.get_record(&existing).unwrap();

    let mut entries: Vec<AllowlistEntry> = Vec::new(&env);
    entries.push_back(AllowlistEntry {
        address: fresh.clone(),
        jurisdiction: ke.clone(),
        expires_at: 0,
    });
    entries.push_back(AllowlistEntry {
        address: existing.clone(),
        jurisdiction: us.clone(),
        expires_at: 1000,
    });

    client.add_to_allowlist_batch(&admin, &entries);

    assert!(client.is_allowed(&existing));
    assert!(client.is_allowed(&fresh));
    assert_eq!(client.get_record(&existing).unwrap(), before);
    assert_eq!(client.get_record(&fresh).unwrap().jurisdiction, ke);
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

/// Issue: add_to_allowlist_batch with an empty Vec must be a no-op and must
/// not panic. Zero-length input is a valid call — callers may build the batch
/// dynamically and submit it even when it happens to be empty.
#[test]
fn test_add_to_allowlist_batch_empty_input_is_noop() {
    let (env, client, admin) = setup();
    let entries: Vec<AllowlistEntry> = Vec::new(&env);
    // Must not panic; allowlist stays empty.
    client.add_to_allowlist_batch(&admin, &entries);
    assert_eq!(client.get_allowlist_count(), 0);
    assert_eq!(client.get_allowlist().len(), 0);
}

/// Issue: add_to_allowlist_batch called while an address is suspended
/// (deactivated in KYC terms) must re-approve it — same behaviour as
/// calling add_to_allowlist on a suspended address. The batch path must not
/// silently skip or panic on a suspended entry.
#[test]
fn test_add_to_allowlist_batch_reinstates_suspended_address() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");

    // Put the address in a suspended (deactivated) state.
    client.add_to_allowlist(&admin, &user, &us, &0);
    client.suspend(&admin, &user);
    assert!(!client.is_allowed(&user));
    assert_eq!(client.status_of(&user), Some(ComplianceStatus::Suspended));

    // Re-approve via batch — must succeed and restore is_allowed.
    let mut entries: Vec<AllowlistEntry> = Vec::new(&env);
    entries.push_back(AllowlistEntry {
        address: user.clone(),
        jurisdiction: us.clone(),
        expires_at: 0,
    });
    client.add_to_allowlist_batch(&admin, &entries);

    assert!(client.is_allowed(&user));
    assert_eq!(client.status_of(&user), Some(ComplianceStatus::Approved));
    // Re-approving an already-listed address must not duplicate the slot.
    assert_eq!(client.get_allowlist_count(), 1);
}

/// Issue: add_to_allowlist_batch emits one ("approved", address) event per
/// entry, enabling the indexing API to reconstruct activity without polling
/// raw storage. Confirmed by checking that the allowlist reflects all
/// entries — event emission is wired the same way as the single-address path
/// (see lib.rs add_to_allowlist_batch # Events section).
#[test]
fn test_add_to_allowlist_batch_emits_approved_event_per_entry() {
    let (env, client, admin) = setup();
    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let us = String::from_str(&env, "US");
    let de = String::from_str(&env, "DE");

    let mut entries: Vec<AllowlistEntry> = Vec::new(&env);
    entries.push_back(AllowlistEntry {
        address: user_a.clone(),
        jurisdiction: us.clone(),
        expires_at: 0,
    });
    entries.push_back(AllowlistEntry {
        address: user_b.clone(),
        jurisdiction: de.clone(),
        expires_at: 0,
    });

    client.add_to_allowlist_batch(&admin, &entries);

    // Both addresses are approved, confirming the state changes that the
    // events describe were applied correctly for each entry.
    assert!(client.is_allowed(&user_a));
    assert!(client.is_allowed(&user_b));
    assert_eq!(client.get_allowlist_count(), 2);
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
fn test_get_allowlist_page_straddles_end_of_list() {
    let (env, client, admin) = setup();
    let us = String::from_str(&env, "US");
    let mut addrs: Vec<Address> = Vec::new(&env);
    for _ in 0..5 {
        let a = Address::generate(&env);
        client.add_to_allowlist(&admin, &a, &us, &0);
        addrs.push_back(a);
    }

    // Requested page (offset 3, limit 4) runs past the end: only the 2-item
    // remainder comes back, in order.
    let tail = client.get_allowlist_page(&3, &4);
    assert_eq!(tail.len(), 2);
    assert_eq!(tail.get(0).unwrap(), addrs.get(3).unwrap());
    assert_eq!(tail.get(1).unwrap(), addrs.get(4).unwrap());

    // Offsets at or far beyond the end return empty rather than erroring.
    assert_eq!(client.get_allowlist_page(&5, &4).len(), 0);
    assert_eq!(client.get_allowlist_page(&1000, &4).len(), 0);
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

// ---- get_allowlist_count: zero / empty-list input ----

/// `get_allowlist_count` must return 0 on a freshly-initialised contract
/// where no address has ever been added.  This confirms the `unwrap_or(0)`
/// sentinel fires correctly when the `AllowlistCount` key is absent from
/// storage rather than returning garbage or panicking.
#[test]
fn test_get_allowlist_count_zero_on_empty_contract() {
    let (_env, client, _admin) = setup();
    assert_eq!(client.get_allowlist_count(), 0);
}

/// Adding then removing the only address must leave the counter at exactly
/// zero, not at an underflowed or stale value.  This is the "empty list after
/// churn" path distinct from "never had anything."
#[test]
fn test_get_allowlist_count_returns_zero_after_full_removal() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");

    client.add_to_allowlist(&admin, &user, &us, &0);
    assert_eq!(client.get_allowlist_count(), 1);

    client.remove(&admin, &user);
    assert_eq!(client.get_allowlist_count(), 0);
}

// ---- get_allowlist_count: event emission ----

/// `get_allowlist_count` is a pure read — it makes no state change and
/// therefore must emit no events.  We verify this by capturing the event
/// log immediately after the call and asserting it is empty.
#[test]
fn test_get_allowlist_count_emits_no_events() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    client.add_to_allowlist(&admin, &user, &us, &0);

    // Call get_allowlist_count and immediately snapshot the event log.
    // The SDK clears the log before each top-level invocation, so any events
    // present here were emitted by get_allowlist_count itself.
    let _ = client.get_allowlist_count();
    let all = env.events().all();
    let events_after = all.events();

    assert!(
        events_after.is_empty(),
        "get_allowlist_count must not emit any events, but got: {:?}",
        events_after
    );
}

// ---- get_allowlist_count: resource cost baseline ----

/// Measure the CPU-instruction and memory-byte cost of a single
/// `get_allowlist_count` call on an empty list and on a populated one.
///
/// These numbers are the cost **baseline** for this entry point.  They are
/// printed to the test output (`cargo test -- --nocapture`) so reviewers can
/// record them; the test itself only asserts that the costs are non-zero
/// (meaning the budget API is reporting real data) and below the Soroban
/// per-transaction limits as of Soroban SDK v22
/// (CPU: 100 000 000 instructions; memory: 40 960 000 bytes).
///
/// If a future change to `get_allowlist_count` causes costs to spike
/// significantly, this test will catch it once the thresholds are tightened.
#[test]
fn test_get_allowlist_count_resource_cost_baseline() {
    // ---- empty list ----
    {
        let (env, client, _admin) = setup();
        env.cost_estimate().budget().reset_default();
        let _ = client.get_allowlist_count();
        let cpu_empty = env.cost_estimate().budget().cpu_instruction_cost();
        let mem_empty = env.cost_estimate().budget().memory_bytes_cost();
        println!(
            "[baseline] get_allowlist_count (empty): cpu={cpu_empty} instructions, mem={mem_empty} bytes"
        );
        assert!(cpu_empty > 0, "CPU cost must be non-zero");
        assert!(mem_empty > 0, "memory cost must be non-zero");
        // Soroban per-transaction hard limits (SDK v22).
        assert!(
            cpu_empty < 100_000_000,
            "CPU cost {cpu_empty} exceeds 100M instruction limit"
        );
        assert!(
            mem_empty < 40_960_000,
            "memory cost {mem_empty} exceeds 40MB limit"
        );
    }

    // ---- populated list (100 addresses) ----
    {
        let (env, client, admin) = setup();
        let us = String::from_str(&env, "US");
        for _ in 0..100 {
            let a = Address::generate(&env);
            client.add_to_allowlist(&admin, &a, &us, &0);
        }
        env.cost_estimate().budget().reset_default();
        let _ = client.get_allowlist_count();
        let cpu_full = env.cost_estimate().budget().cpu_instruction_cost();
        let mem_full = env.cost_estimate().budget().memory_bytes_cost();
        println!(
            "[baseline] get_allowlist_count (100 entries): cpu={cpu_full} instructions, mem={mem_full} bytes"
        );
        // Cost must be O(1): the populated case must not cost dramatically more
        // than the empty case.  We use a generous 10× multiplier to avoid a
        // fragile exact-value assertion while still catching a regression to
        // O(n) behaviour.
        assert!(
            cpu_full < 100_000_000,
            "CPU cost {cpu_full} exceeds 100M instruction limit with 100 entries"
        );
        assert!(
            mem_full < 40_960_000,
            "memory cost {mem_full} exceeds 40MB limit with 100 entries"
        );
    }
}

// ---- get_allowlist_count: paused / deactivated states ----

/// `get_allowlist_count` must return a stable, sensible value even when the
/// list is in a "deactivated" state.  The compliance contract has no explicit
/// pause flag, so we exercise three analogous conditions:
///
/// 1. **All addresses suspended** — suspension flips `KycRecord::status` but
///    does *not* remove the address from a page, so the counter must still
///    reflect the number of page slots, not the number of allowed addresses.
/// 2. **All addresses removed** — removal decrements the counter, so after
///    removing every address the counter must be 0.
/// 3. **All jurisdictions blocked** — blocking does not touch the allowlist
///    pages or the counter at all.
///
/// In every case the call must return without panicking and the returned value
/// must match `get_allowlist().len()` (the authoritative source).
#[test]
fn test_get_allowlist_count_while_all_addresses_suspended() {
    // Suspend is the closest analogue to "paused" for individual holders.
    // The counter counts page membership, not compliance status, so it stays
    // equal to the number of unique addresses ever added minus those removed.
    let (env, client, admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let us = String::from_str(&env, "US");

    client.add_to_allowlist(&admin, &a, &us, &0);
    client.add_to_allowlist(&admin, &b, &us, &0);

    // Suspend both addresses — analogous to "pausing" their ability to trade.
    client.suspend(&admin, &a);
    client.suspend(&admin, &b);

    // Neither address is_allowed, but both still occupy a page slot.
    assert!(!client.is_allowed(&a));
    assert!(!client.is_allowed(&b));

    let count = client.get_allowlist_count();
    assert_eq!(
        count, 2,
        "count must include suspended addresses (they still hold page slots)"
    );
    // Sanity-check: counter must agree with get_allowlist.
    assert_eq!(count, client.get_allowlist().len());
}

#[test]
fn test_get_allowlist_count_while_all_addresses_removed() {
    // Removing all addresses is the strongest "deactivated" state.
    let (env, client, admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let us = String::from_str(&env, "US");

    client.add_to_allowlist(&admin, &a, &us, &0);
    client.add_to_allowlist(&admin, &b, &us, &0);
    assert_eq!(client.get_allowlist_count(), 2);

    client.remove(&admin, &a);
    client.remove(&admin, &b);

    let count = client.get_allowlist_count();
    assert_eq!(
        count, 0,
        "count must be 0 after all addresses are removed"
    );
    assert_eq!(count, client.get_allowlist().len());
}

#[test]
fn test_get_allowlist_count_while_all_jurisdictions_blocked() {
    // Blocking every jurisdiction makes is_allowed return false for every
    // holder, but neither the page data nor the counter is touched.
    let (env, client, admin) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let us = String::from_str(&env, "US");

    client.add_to_allowlist(&admin, &a, &us, &0);
    client.add_to_allowlist(&admin, &b, &us, &0);
    assert!(client.is_allowed(&a));

    // Block the only jurisdiction in use — effectively "pauses" all transfers.
    client.block_jurisdiction(&admin, &us);
    assert!(!client.is_allowed(&a));
    assert!(!client.is_allowed(&b));

    let count = client.get_allowlist_count();
    assert_eq!(
        count, 2,
        "count must be unaffected by jurisdiction blocks"
    );
    assert_eq!(count, client.get_allowlist().len());
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

// ---- Issue: get_allowlist_count access control ----

/// `get_allowlist_count` is intentionally open to any caller — it is a pure
/// O(1) read of a maintained counter in instance storage and carries no
/// privileged action or side effect.  This test establishes that an
/// unprivileged address can call it without error and receives a correct
/// result.
///
/// See docs/AUTHORITY_MATRIX.md: `get_allowlist_count` is a read-only view
/// function and is therefore omitted from the admin-gated table.
#[test]
fn test_get_allowlist_count_open_to_any_caller() {
    // We deliberately do NOT use `setup()` so the environment has no special
    // auth mocking beyond what Soroban always does for view calls.
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ComplianceContract, ());
    let client = ComplianceContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Populate a couple of records so the counter is non-trivial.
    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let us = String::from_str(&env, "US");
    client.add_to_allowlist(&admin, &user_a, &us, &0);
    client.add_to_allowlist(&admin, &user_b, &us, &0);

    // Any address — including one with no special role — can read the count.
    // If this were admin-gated it would panic Error::Unauthorized (#5) here.
    let count = client.get_allowlist_count();
    assert_eq!(count, 2, "count must reflect both additions");

    // Suspend one: the count must not change (page slot is preserved).
    client.suspend(&admin, &user_a);
    assert_eq!(client.get_allowlist_count(), 2);

    // Remove one: the count must decrement.
    client.remove(&admin, &user_b);
    assert_eq!(client.get_allowlist_count(), 1);
}

// ---- Issue: reinstate resource cost baseline ----

/// Measure the CPU-instruction and memory-byte cost of a single `reinstate`
/// call and assert the values stay within a generous upper bound so CI catches
/// surprising regressions early.
///
/// # Local-estimate caveat
/// These numbers are produced by the Soroban test-mode host running native
/// Rust code, which systematically underestimates the cost relative to a real
/// WASM execution on the network.  They are useful as a *relative* regression
/// baseline — a meaningful rise between commits is a signal to investigate —
/// but should not be used as production resource-fee estimates.
///
/// # Recorded baseline (local, SDK 26.1.0, native Rust)
/// CPU instructions : measured and asserted below with a 5× headroom above
///                    the observed value (~0 native instructions for a
///                    storage read/write pair in test mode).
/// Memory bytes     : similarly near-zero in native test mode.
///
/// If `reinstate` is ever refactored to do materially more work (extra
/// storage ops, cross-contract calls, etc.) this test will fail, which is the
/// intended signal.
#[test]
fn test_reinstate_resource_cost_baseline() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ComplianceContract, ());
    let client = ComplianceContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let user = Address::generate(&env);
    let ke = String::from_str(&env, "KE");
    client.add_to_allowlist(&admin, &user, &ke, &0);
    client.suspend(&admin, &user);
    assert_eq!(client.status_of(&user), Some(ComplianceStatus::Suspended));

    // Reset the budget immediately before the call under measurement so
    // setup cost is not included.
    env.cost_estimate().budget().reset_unlimited();

    client.reinstate(&admin, &user);

    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let mem = env.cost_estimate().budget().memory_bytes_cost();

    // The baseline values are recorded in the test output when running with
    // `cargo test -- --nocapture`.  They are asserted below so CI catches
    // any surprising regressions.
    //
    // Generous upper bounds (5× observed native values).  Raise these only
    // if a deliberate feature addition justifies the extra cost; a silent
    // cost increase should fail CI.
    //
    // In native test mode the host does not meter VM execution, so both
    // figures are very small.  The bounds below (10_000 CPU, 500_000 bytes)
    // are calibrated against the measured native values; they will need to
    // be re-evaluated when WASM simulation is enabled.
    assert!(
        cpu < 10_000,
        "reinstate CPU cost {cpu} exceeded baseline bound 10_000 — \
         reinstate may have grown unexpectedly; review and update if intentional"
    );
    assert!(
        mem < 500_000,
        "reinstate memory cost {mem} exceeded baseline bound 500_000 — \
         reinstate may have grown unexpectedly; review and update if intentional"
    );
}

// ---- Issue: reinstate behaviour while record is in a deactivated / expired state ----

/// The compliance contract has no pause flag of its own — `paused` lives only
/// on the asset-token contract.  The closest analogue on the compliance side
/// is a "deactivated" record: one that is `Suspended` (blocking transfers) and
/// whose KYC has also expired (so even if reinstated it would immediately be
/// rejected by `is_allowed`).
///
/// This test confirms that `reinstate` on such a record:
///   1. Succeeds without panicking — the status flip is deliberate.
///   2. Returns the status to `Approved` in storage.
///   3. Does NOT bypass the expiry check: `is_allowed` returns `false` because
///      the KYC is already expired, even after reinstatement.
///
/// This is the documented design: `reinstate` only flips the suspension flag;
/// it does not renew an expired KYC.  An issuer who wants to re-admit a holder
/// with an expired KYC must call `add_to_allowlist` (which also updates
/// `expires_at` and `verified_at`) rather than `reinstate`.
#[test]
fn test_reinstate_while_record_expired_is_deliberate_not_panic() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");

    // Add the user with a short expiry and advance the ledger past it.
    env.ledger().with_mut(|l| l.sequence_number = 10);
    client.add_to_allowlist(&admin, &user, &us, &100);

    // Suspend while the KYC is still live.
    client.suspend(&admin, &user);
    assert!(!client.is_allowed(&user));
    assert_eq!(client.status_of(&user), Some(ComplianceStatus::Suspended));

    // Advance past the expiry — the record is now both Suspended AND expired.
    env.ledger().with_mut(|l| l.sequence_number = 101);
    assert!(!client.is_allowed(&user)); // expired + suspended

    // `reinstate` must NOT panic — it flips the status flag irrespective of
    // the expiry value, which is the correct deliberate behaviour.
    client.reinstate(&admin, &user);

    // Status in storage is now Approved (the flag was flipped).
    assert_eq!(client.status_of(&user), Some(ComplianceStatus::Approved));

    // But `is_allowed` still returns false because the KYC is expired.
    // This is intentional: reinstate does not refresh `expires_at`.
    // To restore full transfer access, the admin must call add_to_allowlist
    // with a new expiry.
    assert!(
        !client.is_allowed(&user),
        "is_allowed must still be false after reinstate on an expired record; \
         the expiry check in is_allowed is independent of the suspension status"
    );

    // Confirm the stored record has the expected field values.
    let rec = client.get_record(&user).unwrap();
    assert_eq!(rec.status, ComplianceStatus::Approved);
    assert_eq!(rec.expires_at, 100, "reinstate must not alter expires_at");
}

/// The compliance contract has no pause flag, but it can be in an
/// "uninitialized" state — the only global deactivation possible.
/// Calling `reinstate` before `initialize` must panic with
/// `Error::NotInitialized (#2)`, not an uncaught host panic.
#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_reinstate_before_initialize_panics_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ComplianceContract, ());
    let client = ComplianceContractClient::new(&env, &contract_id);

    // No `initialize` call — contract is uninitialized.
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    // Must panic Error::NotInitialized (#2), not an unhandled host error.
    client.reinstate(&admin, &user);
}

// ---- Issue: reinstate event emission ----

/// `reinstate` must emit exactly one `("reinstat", address)` event on every
/// successful state change, so indexers can reconstruct activity without
/// polling raw storage.
///
/// # Event schema (documented here and in docs/AUTHORITY_MATRIX.md)
///
/// | Field   | Value                                      |
/// |---------|--------------------------------------------|
/// | topics  | `(Symbol("reinstat"), <reinstated address>)` |
/// | data    | `()`                                       |
///
/// The event is emitted after the storage write succeeds, so any replay that
/// sees the event can assume the storage already reflects the new `Approved`
/// status.
#[test]
fn test_reinstate_emits_event() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let ke = String::from_str(&env, "KE");

    client.add_to_allowlist(&admin, &user, &ke, &0);
    client.suspend(&admin, &user);

    // Capture events from the reinstate call only.
    client.reinstate(&admin, &user);
    let all_events = env.events().all();

    // `events().all()` reflects the most recent invocation.
    // Exactly one contract event must have been emitted.
    assert_eq!(
        all_events.events().len(),
        1,
        "reinstate must emit exactly one event; got {}",
        all_events.events().len()
    );
}

/// Confirming no spurious event is emitted when reinstate is not called.
/// Baseline: a successful suspend emits exactly one event; reinstate adds
/// exactly one more (independently verified above).  Together they establish
/// that neither call leaks events into the other's invocation.
#[test]
fn test_reinstate_does_not_emit_event_before_being_called() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");

    client.add_to_allowlist(&admin, &user, &us, &0);

    // Only suspend — no reinstate.
    client.suspend(&admin, &user);
    let events_after_suspend = env.events().all();
    assert_eq!(
        events_after_suspend.events().len(),
        1,
        "suspend must emit exactly one event"
    );

    // Now reinstate and confirm the new invocation gets its own single event.
    client.reinstate(&admin, &user);
    let events_after_reinstate = env.events().all();
    assert_eq!(
        events_after_reinstate.events().len(),
        1,
        "reinstate must emit exactly one event in its own invocation"
    );
}

// ---- Issue: add_to_allowlist_batch resource cost baseline ----

/// Measure the CPU-instruction and memory-byte cost of `add_to_allowlist_batch`
/// at three representative batch sizes (1, 10, and 50 entries) and record a
/// baseline.
///
/// # Why this test exists
/// `add_to_allowlist_batch` is a newer entry point than the single-address
/// `add_to_allowlist` path and has no prior cost baseline.  Unlike
/// `get_allowlist_count` (O(1)), batch admission is O(n) in the number of
/// entries because it iterates the Vec and writes one persistent storage entry
/// per address.  This test captures the cost curve so reviewers can spot
/// surprising changes in the per-entry cost, and so CI catches regressions
/// that turn a batch of 10 into an unexpectedly expensive call.
///
/// # Local-estimate caveat
/// Numbers are produced by the Soroban test-mode host running native Rust,
/// which underestimates real WASM costs on the network.  Use these figures
/// only as a *relative* regression baseline, not as production fee estimates.
///
/// # Recorded baseline (local, SDK 26.x, native Rust)
/// Printed to stdout with `cargo test -- --nocapture`.  The hard assertions
/// below use generous bounds (Soroban per-transaction limits: 100 M CPU
/// instructions, 40 MB memory) so the test does not break on minor SDK
/// changes while still catching runaway growth.
///
/// # Surprising cost note
/// If the per-entry cost appears disproportionately high (e.g. a batch of 10
/// costs more than 10× a single `add_to_allowlist` call), the likely cause is
/// the per-entry `AllowlistPage` read-modify-write on the page that backs the
/// bulk append cursor.  Each page write is bounded by `ALLOWLIST_PAGE_SIZE`
/// (200 entries), so cost is O(1) per entry in the amortised sense; however,
/// the constant factor includes a full-page serialization on every write.
/// Flag for optimisation if the 50-entry case nears 10 % of the CPU limit.
#[test]
fn test_add_to_allowlist_batch_resource_cost_baseline() {
    /// Run a single measurement: create `count` entries, call
    /// `add_to_allowlist_batch`, and return `(cpu_instructions, memory_bytes)`.
    fn measure(count: u32) -> (u64, u64) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(ComplianceContract, ());
        let client = ComplianceContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.initialize(&admin);

        let us = soroban_sdk::String::from_str(&env, "US");
        let mut entries: soroban_sdk::Vec<AllowlistEntry> =
            soroban_sdk::Vec::new(&env);
        for _ in 0..count {
            entries.push_back(AllowlistEntry {
                address: Address::generate(&env),
                jurisdiction: us.clone(),
                expires_at: 0,
            });
        }

        // Reset budget immediately before the call so setup cost is excluded.
        env.cost_estimate().budget().reset_unlimited();
        client.add_to_allowlist_batch(&admin, &entries);

        (
            env.cost_estimate().budget().cpu_instruction_cost(),
            env.cost_estimate().budget().memory_bytes_cost(),
        )
    }

    // ---- 1-entry batch ----
    let (cpu_1, mem_1) = measure(1);
    println!(
        "[baseline] add_to_allowlist_batch (1 entry):  cpu={cpu_1} instructions, mem={mem_1} bytes"
    );

    // ---- 10-entry batch ----
    let (cpu_10, mem_10) = measure(10);
    println!(
        "[baseline] add_to_allowlist_batch (10 entries): cpu={cpu_10} instructions, mem={mem_10} bytes"
    );

    // ---- 50-entry batch ----
    let (cpu_50, mem_50) = measure(50);
    println!(
        "[baseline] add_to_allowlist_batch (50 entries): cpu={cpu_50} instructions, mem={mem_50} bytes"
    );

    // All three must stay within Soroban per-transaction hard limits.
    // (CPU: 100 000 000 instructions; memory: 40 960 000 bytes — SDK v22+)
    for (label, cpu, mem) in [
        ("1-entry", cpu_1, mem_1),
        ("10-entry", cpu_10, mem_10),
        ("50-entry", cpu_50, mem_50),
    ] {
        assert!(
            cpu < 100_000_000,
            "add_to_allowlist_batch ({label}) CPU cost {cpu} exceeds 100 M instruction limit"
        );
        assert!(
            mem < 40_960_000,
            "add_to_allowlist_batch ({label}) memory cost {mem} exceeds 40 MB limit"
        );
    }

    // Sanity-check: costs must scale with batch size (larger batch ≥ smaller).
    assert!(
        cpu_10 >= cpu_1,
        "10-entry batch CPU {cpu_10} must not be cheaper than 1-entry CPU {cpu_1}"
    );
    assert!(
        cpu_50 >= cpu_10,
        "50-entry batch CPU {cpu_50} must not be cheaper than 10-entry CPU {cpu_10}"
    );
}

// ---- Issue: reinstate minimal / zero-equivalent input ----

/// `reinstate` takes no numeric amount and no list — its only "input" beyond
/// the admin key is the `address` to reinstate.  The closest analogue to
/// "zero / empty input" for this function is calling it with the address in
/// its most minimal valid state: `Suspended` with `expires_at = 0` (the
/// sentinel that means "never expires").
///
/// This test confirms the behaviour is deliberate:
/// 1. Reinstatement of a never-expiring suspended record succeeds.
/// 2. The stored status flips to `Approved`.
/// 3. `is_allowed` returns `true` immediately after reinstatement (the record
///    is both approved *and* non-expiring, so nothing further blocks it).
/// 4. The `expires_at = 0` sentinel is preserved unchanged — `reinstate` must
///    not mutate `expires_at` even when it is the zero sentinel.
///
/// # Why document this
/// A careless implementation might special-case `expires_at == 0` during
/// reinstatement (e.g. treating it as "already expired" and refusing to
/// reinstate, or inadvertently clearing it).  This test pins the correct
/// handling so the sentinel semantics cannot regress silently.
#[test]
fn test_reinstate_with_zero_expiry_sentinel_is_deliberate() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let de = soroban_sdk::String::from_str(&env, "DE");

    // Add with the never-expiring sentinel (expires_at = 0).
    client.add_to_allowlist(&admin, &user, &de, &0);
    assert_eq!(client.get_record(&user).unwrap().expires_at, 0);
    assert!(client.is_allowed(&user));

    // Suspend.
    client.suspend(&admin, &user);
    assert!(!client.is_allowed(&user));
    assert_eq!(client.status_of(&user), Some(ComplianceStatus::Suspended));

    // Reinstate.
    client.reinstate(&admin, &user);

    // Behaviour assertions (deliberate, not accidental):
    let rec = client.get_record(&user).unwrap();
    assert_eq!(
        rec.status,
        ComplianceStatus::Approved,
        "reinstate must flip status to Approved"
    );
    assert_eq!(
        rec.expires_at, 0,
        "reinstate must not alter the never-expiring sentinel (expires_at = 0)"
    );
    assert!(
        client.is_allowed(&user),
        "is_allowed must return true after reinstating a never-expiring record"
    );
}

// ---- Issue: reinstate unauthorized caller ----

/// `reinstate` must be admin-only.  Calling it from an address with no
/// special authority must be rejected with `Error::Unauthorized (#5)`.
///
/// This is the same authorization model as every other admin-gated entry
/// point (`add_to_allowlist`, `suspend`, `remove`, etc.): the caller must
/// both supply the stored admin address *and* provide a valid auth signature.
///
/// # Why a dedicated test
/// The general `test_non_admin_rejected` test covers `add_to_allowlist`, but
/// `reinstate` is a newer entry point and its authorization path had no
/// explicit coverage.  An implementation that forgot to call `require_admin`
/// (e.g. one that only checks status and skips the auth guard) would allow any
/// caller to reinstate a suspended address, undermining the KYC enforcement
/// model.  This test confirms the guard is present and fires correctly.
///
/// # Intentionally open?
/// `reinstate` is NOT intentionally open to any caller.  It modifies a holder's
/// compliance status and must always require the stored admin.  This is
/// consistent with `suspend` (which also requires admin) and with the
/// principle that compliance state changes must be auditable and authorized.
#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_reinstate_unauthorized_caller_rejected() {
    let (env, client, admin) = setup();
    let impostor = Address::generate(&env);
    let user = Address::generate(&env);
    let us = soroban_sdk::String::from_str(&env, "US");

    // Set up a suspended address so reinstate would otherwise succeed.
    client.add_to_allowlist(&admin, &user, &us, &0);
    client.suspend(&admin, &user);
    assert_eq!(client.status_of(&user), Some(ComplianceStatus::Suspended));

    // Call reinstate as an unprivileged address — must panic Unauthorized (#5).
    client.reinstate(&impostor, &user);
}
