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
fn test_get_record_unknown_address_returns_none() {
    // Issue #208: get_record must return None (not trap) for an address that
    // was never added to the allowlist.
    let (env, client, _admin) = setup();
    let unknown = Address::generate(&env);
    assert_eq!(client.get_record(&unknown), None);
}

#[test]
fn test_add_to_allowlist_rejects_expiry_at_or_before_now() {
    // Issue #207: a non-zero expires_at at or below the current ledger must
    // fail with the typed InvalidExpiry error (not a bare panic).
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    env.ledger().with_mut(|l| l.sequence_number = 500);

    // expires_at == now.
    assert!(matches!(
        client.try_add_to_allowlist(&admin, &user, &us, &500),
        Err(Ok(Error::InvalidExpiry))
    ));

    // expires_at < now.
    assert!(matches!(
        client.try_add_to_allowlist(&admin, &user, &us, &100),
        Err(Ok(Error::InvalidExpiry))
    ));
}

#[test]
fn test_add_to_allowlist_accepts_zero_expiry_as_never_expires() {
    // Issue #207: expires_at == 0 means "never expires" and must be accepted
    // even when the current ledger sequence is non-zero.
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    env.ledger().with_mut(|l| l.sequence_number = 500);

    assert!(client.try_add_to_allowlist(&admin, &user, &us, &0).is_ok());
    assert!(client.is_allowed(&user));
}

#[test]
fn test_admin_only_entry_points_reject_non_admin() {
    // Issue #209: every admin-gated entry point must fail with the typed
    // Unauthorized error (not succeed, and not a bare host panic) when called
    // by an address other than the configured admin.
    let (env, client, admin) = setup();
    let non_admin = Address::generate(&env);
    let user = Address::generate(&env);
    let us = String::from_str(&env, "US");
    // Give `user` a real record so suspend/remove/prune_expired would have
    // something to act on if the admin check were (incorrectly) skipped.
    client.add_to_allowlist(&admin, &user, &us, &0);

    macro_rules! assert_unauthorized {
        ($label:expr, $call:expr) => {
            assert!(
                matches!($call, Err(Ok(Error::Unauthorized))),
                "{} should reject a non-admin caller with Unauthorized",
                $label
            );
        };
    }

    assert_unauthorized!(
        "add_to_allowlist",
        client.try_add_to_allowlist(&non_admin, &user, &us, &0)
    );
    assert_unauthorized!("suspend", client.try_suspend(&non_admin, &user));
    assert_unauthorized!("remove", client.try_remove(&non_admin, &user));
    assert_unauthorized!(
        "block_jurisdiction",
        client.try_block_jurisdiction(&non_admin, &us)
    );
    assert_unauthorized!(
        "unblock_jurisdiction",
        client.try_unblock_jurisdiction(&non_admin, &us)
    );
    assert_unauthorized!("prune_expired", client.try_prune_expired(&non_admin));
}

#[test]
fn test_block_jurisdiction_matching_is_case_insensitive() {
    // Issue #210: `normalize_jurisdiction` uppercases and trims on every write
    // and read path (add_to_allowlist, block/unblock_jurisdiction,
    // is_jurisdiction_blocked), so blocking "US" also blocks "us" — matching
    // is intentionally case-insensitive, not case-sensitive/exact. Documented
    // and pinned here so a future change to that behavior is deliberate.
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    client.add_to_allowlist(&admin, &user, &String::from_str(&env, "us"), &0);
    assert!(client.is_allowed(&user));

    client.block_jurisdiction(&admin, &String::from_str(&env, "US"));

    assert!(client.is_jurisdiction_blocked(&String::from_str(&env, "us")));
    assert!(!client.is_allowed(&user));
}
