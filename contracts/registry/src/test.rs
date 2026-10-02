#![cfg(test)]
use super::*;
use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, Address, Env, String};

fn setup() -> (Env, RegistryContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(RegistryContract, ());
    let client = RegistryContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

fn register(
    env: &Env,
    client: &RegistryContractClient,
    issuer: &Address,
    kind: &str,
    valuation: i128,
) -> u64 {
    let token = Address::generate(env);
    client.register_asset(
        issuer,
        &token,
        &String::from_str(env, "Asset"),
        &String::from_str(env, kind),
        &valuation,
    )
}

#[test]
fn test_version() {
    let (_env, client, _admin) = setup();
    assert_eq!(client.version(), VERSION);
}

#[test]
fn test_initialize_admin() {
    let (_env, client, admin) = setup();
    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.asset_count(), 0);
    assert_eq!(client.total_value_locked(), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_get_admin_before_init_panics_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(RegistryContract, ());
    let client = RegistryContractClient::new(&env, &id);
    client.get_admin();
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_double_init() {
    let (env, client, _admin) = setup();
    client.initialize(&Address::generate(&env));
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_register_before_init_panics_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(RegistryContract, ());
    let client = RegistryContractClient::new(&env, &id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_asset(
        &issuer,
        &token,
        &String::from_str(&env, "Asset"),
        &String::from_str(&env, "real_estate"),
        &100,
    );
}

#[test]
fn test_register_and_get_asset() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "real_estate", 10_000);
    assert_eq!(id, 1);
    let entry = client.get_asset(&id);
    assert_eq!(entry.issuer, issuer);
    assert_eq!(entry.valuation, 10_000);
    assert!(entry.active);
}

#[test]
fn test_ids_increment() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    let a = register(&env, &client, &issuer, "invoice", 1);
    let b = register(&env, &client, &issuer, "invoice", 1);
    let c = register(&env, &client, &issuer, "invoice", 1);
    assert_eq!(a, 1);
    assert_eq!(b, 2);
    assert_eq!(c, 3);
    assert_eq!(client.asset_count(), 3);
}

proptest! {
    #[test]
    fn prop_active_count_matches_active_entries(
        ops in prop::collection::vec((any::<u8>(), any::<u8>()), 1..20),
    ) {
        let (env, client, admin) = setup();
        let mut active_ids = Vec::new(&env);
        for op in ops {
            if active_ids.len() == 0 || op.0 % 2 == 0 {
                let issuer = Address::generate(&env);
                let token = Address::generate(&env);
                let id = client.register_asset(
                    &issuer,
                    &token,
                    &String::from_str(&env, "Asset"),
                    &String::from_str(&env, "real_estate"),
                    &1_000,
                );
                active_ids.push_back(id);
            } else {
                let idx = (op.1 as usize) % active_ids.len() as usize;
                let id = active_ids.get(idx as u32).unwrap();
                client.deactivate_asset(&admin, &id);
                let mut filtered = Vec::new(&env);
                for active_id in active_ids.iter() {
                    if active_id != id {
                        filtered.push_back(active_id);
                    }
                }
                active_ids = filtered;
            }

            let active_in_registry = client
                .get_all_assets(&0, &u32::MAX)
                .iter()
                .filter(|entry| entry.active)
                .count() as u64;
            assert_eq!(client.active_count(), active_in_registry);
            assert_eq!(client.active_count(), active_ids.len() as u64);
        }
    }
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_get_missing_asset() {
    let (_env, client, _admin) = setup();
    client.get_asset(&999);
}

#[test]
fn test_get_assets_by_issuer() {
    let (env, client, _admin) = setup();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    register(&env, &client, &alice, "real_estate", 5);
    register(&env, &client, &alice, "commodity", 5);
    register(&env, &client, &bob, "invoice", 5);
    assert_eq!(client.get_assets_by_issuer(&alice).len(), 2);
    assert_eq!(client.get_assets_by_issuer(&bob).len(), 1);
}

#[test]
fn test_issuer_can_deactivate_asset() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "real_estate", 100);
    client.deactivate_asset(&issuer, &id);
    assert!(!client.get_asset(&id).active);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_unrelated_address_cannot_deactivate_asset() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "real_estate", 100);
    let stranger = Address::generate(&env);
    client.deactivate_asset(&stranger, &id);
}

#[test]
fn test_get_assets_by_issuer_with_no_assets_returns_empty() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    let result = client.get_assets_by_issuer(&issuer);
    assert_eq!(result.len(), 0);
}

#[test]
fn test_get_assets_by_type() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "real_estate", 5);
    register(&env, &client, &issuer, "real_estate", 5);
    register(&env, &client, &issuer, "commodity", 5);
    assert_eq!(
        client
            .get_assets_by_type(&String::from_str(&env, "real_estate"))
            .len(),
        2
    );
    assert_eq!(
        client
            .get_assets_by_type(&String::from_str(&env, "commodity"))
            .len(),
        1
    );
}

#[test]
fn test_get_all_and_tvl() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "real_estate", 100);
    register(&env, &client, &issuer, "invoice", 250);
    assert_eq!(client.get_all_assets(&0, &2).len(), 2);
    assert_eq!(client.total_value_locked(), 350);
}

#[test]
fn test_get_all_assets_pagination_edge_cases() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "real_estate", 100);
    register(&env, &client, &issuer, "invoice", 250);
    register(&env, &client, &issuer, "commodity", 300);

    // Test start_id = 0 (should clamp to 1)
    let result = client.get_all_assets(&0, &10);
    assert_eq!(result.len(), 3);

    // Test limit = 0 (should return empty)
    let result = client.get_all_assets(&1, &0);
    assert_eq!(result.len(), 0);

    // Test start_id past counter (should return empty)
    let result = client.get_all_assets(&99, &10);
    assert_eq!(result.len(), 0);

    // Test limit past counter (should cap at counter + 1)
    let result = client.get_all_assets(&2, &1000);
    assert_eq!(result.len(), 2);

    // Test normal pagination
    let result = client.get_all_assets(&1, &2);
    assert_eq!(result.len(), 2);
    let result = client.get_all_assets(&3, &2);
    assert_eq!(result.len(), 1);
}

#[test]
fn test_deactivate_excludes_from_tvl() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "real_estate", 100);
    register(&env, &client, &issuer, "invoice", 250);
    assert_eq!(client.total_value_locked(), 350);
    client.deactivate_asset(&admin, &id);
    assert!(!client.get_asset(&id).active);
    assert_eq!(client.total_value_locked(), 250);
}

#[test]
fn test_tvl_sums_only_active() {
    let (env, client, admin) = setup();
    assert_eq!(client.total_value_locked(), 0);

    let issuer = Address::generate(&env);
    let a = register(&env, &client, &issuer, "real_estate", 100);
    let b = register(&env, &client, &issuer, "invoice", 250);
    let c = register(&env, &client, &issuer, "commodity", 40);
    assert_eq!(client.total_value_locked(), 390);

    client.deactivate_asset(&admin, &a);
    assert_eq!(client.total_value_locked(), 290);

    client.deactivate_asset(&admin, &c);
    assert_eq!(client.total_value_locked(), 250);

    client.deactivate_asset(&admin, &b);
    assert_eq!(client.total_value_locked(), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_deactivate_requires_admin() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "real_estate", 100);
    let impostor = Address::generate(&env);
    client.deactivate_asset(&impostor, &id);
}

#[test]
fn test_active_count_excludes_deactivated() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let a = register(&env, &client, &issuer, "real_estate", 100);
    register(&env, &client, &issuer, "invoice", 250);
    assert_eq!(client.active_count(), 2);
    assert_eq!(client.asset_count(), 2);
    client.deactivate_asset(&admin, &a);
    assert_eq!(client.active_count(), 1);
    assert_eq!(client.asset_count(), 2);
}

#[test]
fn test_active_count_matches_asset_count_after_deactivations() {
    // Stresses the asset_count()/active_count() invariant across a sequence
    // of deactivations, not just a single before/after snapshot.
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);

    let mut ids = Vec::new(&env);
    for _ in 0..5 {
        ids.push_back(register(&env, &client, &issuer, "real_estate", 100));
    }

    assert_eq!(client.asset_count(), 5);
    assert_eq!(client.active_count(), 5);
    assert_eq!(client.asset_count() - client.active_count(), 0);

    let mut deactivated = 0u64;
    for id in ids.iter() {
        client.deactivate_asset(&admin, &id);
        deactivated += 1;
        assert_eq!(client.asset_count(), 5);
        assert_eq!(client.active_count(), 5 - deactivated);
        assert_eq!(client.asset_count() - client.active_count(), deactivated);
    }

    assert_eq!(client.active_count(), 0);
    assert_eq!(client.asset_count() - client.active_count(), 5);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_negative_valuation_rejected() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "real_estate", -1);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_empty_name_rejected() {
    // Issue #48: empty asset name must panic InvalidInput (#7).
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_asset(
        &issuer,
        &token,
        &String::from_str(&env, ""),
        &String::from_str(&env, "real_estate"),
        &100,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_invalid_asset_type_rejected() {
    // Issue #48: unknown asset_type must panic InvalidInput (#7).
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_asset(
        &issuer,
        &token,
        &String::from_str(&env, "My Asset"),
        &String::from_str(&env, "garbage"),
        &100,
    );
}

#[test]
fn test_asset_ids_increment_monotonically_and_are_never_reused() {
    // Issue #223.
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id1 = register(&env, &client, &issuer, "real_estate", 1_000);
    let id2 = register(&env, &client, &issuer, "invoice", 2_000);
    let id3 = register(&env, &client, &issuer, "commodity", 3_000);
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(id3, 3);

    client.deactivate_asset(&admin, &id2);

    let id4 = register(&env, &client, &issuer, "bond", 4_000);
    assert_eq!(id4, 4);
    assert_ne!(id4, id2);
}

#[test]
fn test_get_asset_on_unknown_id_fails_asset_not_found() {
    // Issue #224.
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "real_estate", 1_000);

    assert_eq!(
        client.try_get_asset(&0),
        Err(Ok(Error::AssetNotFound.into()))
    );
    assert_eq!(
        client.try_get_asset(&2),
        Err(Ok(Error::AssetNotFound.into()))
    );
}

#[test]
fn test_get_assets_by_issuer_and_by_type_return_empty_vec_not_error() {
    // Issue #225.
    let (env, client, _admin) = setup();
    let unknown_issuer = Address::generate(&env);

    let by_issuer = client.get_assets_by_issuer(&unknown_issuer);
    assert_eq!(by_issuer.len(), 0);

    let by_type = client.get_assets_by_type(&String::from_str(&env, "fund"));
    assert_eq!(by_type.len(), 0);
}

#[test]
fn test_deactivate_asset_on_unknown_id_fails_and_active_count_unchanged() {
    // Issue #226.
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "real_estate", 1_000);
    assert_eq!(client.active_count(), 1);

    assert_eq!(
        client.try_deactivate_asset(&admin, &99),
        Err(Ok(Error::AssetNotFound.into()))
    );
    assert_eq!(client.active_count(), 1);
}

#[test]
fn test_deactivate_already_inactive_asset_is_noop() {
    // Issue #298: deactivating an already-inactive asset should be a no-op (no event emitted).
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "real_estate", 100);
    assert_eq!(client.active_count(), 1);
    assert_eq!(client.total_value_locked(), 100);

    client.deactivate_asset(&admin, &id);
    assert_eq!(client.active_count(), 0);
    assert_eq!(client.total_value_locked(), 0);
    assert!(!client.get_asset(&id).active);

    // Deactivate again — should be a no-op (counts and TVL unchanged)
    client.deactivate_asset(&admin, &id);
    assert_eq!(client.active_count(), 0);
    assert_eq!(client.total_value_locked(), 0);
    assert!(!client.get_asset(&id).active);
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
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_asset(
        &issuer,
        &token,
        &String::from_str(&env, "Asset"),
        &String::from_str(&env, "real_estate"),
        &1_000,
    );
    let res = client.try_deactivate_asset(&admin, &1);
    assert_eq!(res, Err(Ok(Error::Unauthorized.into())));
    // The new admin can act.
    client.deactivate_asset(&successor, &1);
}

#[test]
fn test_cancel_admin_proposal_by_current_admin() {
    let (env, client, admin) = setup();
    let successor = Address::generate(&env);

    client.propose_admin(&admin, &successor);
    client.cancel_admin_proposal(&admin);
    assert_eq!(client.get_pending_admin(), None);

    // The cancelled successor can no longer accept.
    let res = client.try_accept_admin(&successor);
    assert_eq!(res, Err(Ok(Error::NoPendingAdmin.into())));
    // The admin is unchanged.
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_cancel_admin_proposal_with_nothing_pending_fails() {
    let (_env, client, admin) = setup();
    let res = client.try_cancel_admin_proposal(&admin);
    assert_eq!(res, Err(Ok(Error::NoPendingAdmin.into())));
}

#[test]
fn test_non_admin_cannot_propose_admin() {
    let (env, client, _admin) = setup();
    let non_admin = Address::generate(&env);
    let successor = Address::generate(&env);
    let res = client.try_propose_admin(&non_admin, &successor);
    assert_eq!(res, Err(Ok(Error::Unauthorized.into())));
}

#[test]
fn test_non_admin_cannot_cancel_admin_proposal() {
    let (env, client, admin) = setup();
    let non_admin = Address::generate(&env);
    let successor = Address::generate(&env);
    client.propose_admin(&admin, &successor);
    let res = client.try_cancel_admin_proposal(&non_admin);
    assert_eq!(res, Err(Ok(Error::Unauthorized.into())));
}

#[test]
fn test_only_proposed_successor_can_accept() {
    let (env, client, admin) = setup();
    let successor = Address::generate(&env);
    let impostor = Address::generate(&env);

    client.propose_admin(&admin, &successor);
    let res = client.try_accept_admin(&impostor);
    assert_eq!(res, Err(Ok(Error::Unauthorized.into())));
    // Role is unaffected by the failed attempt.
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_accept_admin_with_no_pending_proposal_fails() {
    let (env, client, _admin) = setup();
    let stranger = Address::generate(&env);
    let res = client.try_accept_admin(&stranger);
    assert_eq!(res, Err(Ok(Error::NoPendingAdmin.into())));
}

#[test]
fn test_reactivate_asset_restores_tvl_and_active_count() {
    // Deactivation must not be permanent — reactivation restores TVL and
    // active_count for the same asset id.
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "bond", 100);

    client.deactivate_asset(&admin, &id);
    assert_eq!(client.total_value_locked(), 0);
    assert_eq!(client.active_count(), 0);
    assert!(!client.get_asset(&id).active);

    client.reactivate_asset(&admin, &id);
    assert_eq!(client.total_value_locked(), 100);
    assert_eq!(client.active_count(), 1);
    assert!(client.get_asset(&id).active);
}

#[test]
fn test_reactivate_already_active_asset_is_noop() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "bond", 100);

    client.reactivate_asset(&admin, &id);
    assert_eq!(client.total_value_locked(), 100);
    assert_eq!(client.active_count(), 1);
    assert!(client.get_asset(&id).active);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_reactivate_requires_admin() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "bond", 100);
    client.deactivate_asset(&admin, &id);
    client.reactivate_asset(&issuer, &id);
}

#[test]
fn test_reactivate_unknown_id_fails() {
    let (_env, client, admin) = setup();
    assert_eq!(
        client.try_reactivate_asset(&admin, &999u64),
        Err(Ok(Error::AssetNotFound.into()))
    );
}

#[test]
fn test_tvl_running_total_matches_full_recomputation() {
    // TVL must stay O(1) to read while remaining correct across every
    // mutation. Prove the running total always equals a brute-force
    // recomputation over every asset (active only) via get_all_assets.
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let a = register(&env, &client, &issuer, "real_estate", 100);
    let b = register(&env, &client, &issuer, "invoice", 250);
    let c = register(&env, &client, &issuer, "commodity", 75);

    let recompute = |client: &RegistryContractClient| -> i128 {
        client
            .get_all_assets(&0, &1000)
            .iter()
            .filter(|e| e.active)
            .map(|e| e.valuation)
            .sum()
    };

    assert_eq!(client.total_value_locked(), recompute(&client));

    client.deactivate_asset(&admin, &b);
    assert_eq!(client.total_value_locked(), recompute(&client));

    client.reactivate_asset(&admin, &b);
    assert_eq!(client.total_value_locked(), recompute(&client));

    client.deactivate_asset(&admin, &a);
    client.deactivate_asset(&admin, &c);
    assert_eq!(client.total_value_locked(), recompute(&client));

    client.reactivate_asset(&admin, &a);
    assert_eq!(client.total_value_locked(), recompute(&client));
}

#[test]
fn test_get_assets_by_type_is_case_sensitive() {
    // Matching rule for get_assets_by_type: byte-exact, so case must matter.
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "real_estate", 5);

    assert_eq!(
        client
            .get_assets_by_type(&String::from_str(&env, "real_estate"))
            .len(),
        1
    );
    assert_eq!(
        client
            .get_assets_by_type(&String::from_str(&env, "Real_Estate"))
            .len(),
        0
    );
    assert_eq!(
        client
            .get_assets_by_type(&String::from_str(&env, "REAL_ESTATE"))
            .len(),
        0
    );
}

#[test]
fn test_get_assets_by_type_is_whitespace_sensitive() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "invoice", 5);

    assert_eq!(
        client
            .get_assets_by_type(&String::from_str(&env, "invoice"))
            .len(),
        1
    );
    assert_eq!(
        client
            .get_assets_by_type(&String::from_str(&env, " invoice"))
            .len(),
        0
    );
    assert_eq!(
        client
            .get_assets_by_type(&String::from_str(&env, "invoice "))
            .len(),
        0
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_register_rejects_asset_type_with_whitespace() {
    // A padded variant of a valid type must still be rejected at
    // registration, not silently normalised.
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "invoice ", 100);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_register_rejects_asset_type_with_wrong_case() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "Invoice", 100);
}

#[test]
fn test_duplicate_registration_does_not_double_count_tvl() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "real_estate", 100);
    assert_eq!(client.total_value_locked(), 100);
    assert_eq!(client.asset_count(), 1);
}

/// Registering the same token contract twice must be rejected, otherwise TVL
/// and any client reading `get_all_assets` would double-count the same
/// underlying asset under two distinct registry ids.
#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_duplicate_token_contract_registration_rejected() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_asset(
        &issuer,
        &token,
        &String::from_str(&env, "Asset One"),
        &String::from_str(&env, "real_estate"),
        &100,
    );
    // Same token_contract, even under a different issuer/name, must be rejected.
    let other_issuer = Address::generate(&env);
    client.register_asset(
        &other_issuer,
        &token,
        &String::from_str(&env, "Asset One Again"),
        &String::from_str(&env, "invoice"),
        &200,
    );
}

/// `limit` beyond `MAX_PAGE_SIZE` is silently clamped, bounding response size
/// regardless of what a caller requests.
#[test]
fn test_get_all_assets_enforces_max_page_size() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    for i in 0..5 {
        register(&env, &client, &issuer, "real_estate", 100 + i);
    }
    // Requesting far more than exist, and far more than MAX_PAGE_SIZE, still
    // only returns what's actually registered (small-registry call keeps working).
    let result = client.get_all_assets(&1, &(MAX_PAGE_SIZE * 10));
    assert_eq!(result.len(), 5);
}

/// The final page of a paginated walk may be partial (fewer than `limit`
/// items) once it reaches the end of the registry.
#[test]
fn test_get_all_assets_final_partial_page() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    for i in 0..7 {
        register(&env, &client, &issuer, "real_estate", 100 + i);
    }
    let page_size = 3u32;
    let first = client.get_all_assets(&1, &page_size);
    assert_eq!(first.len(), 3);
    let second = client.get_all_assets(&4, &page_size);
    assert_eq!(second.len(), 3);
    // Final page is partial: only 1 asset remains (7 total, 6 already read).
    let third = client.get_all_assets(&7, &page_size);
    assert_eq!(third.len(), 1);
    let fourth = client.get_all_assets(&8, &page_size);
    assert_eq!(fourth.len(), 0);
}

// ---- TVL overflow tests (issue #436) ----

/// Registering an asset with valuation i128::MAX succeeds: TVL == i128::MAX is
/// a valid, representable state.
#[test]
fn test_register_exact_max_valuation_accepted() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "real_estate", i128::MAX);
    assert_eq!(id, 1);
    assert_eq!(client.total_value_locked(), i128::MAX);
    assert_eq!(client.asset_count(), 1);
    assert_eq!(client.active_count(), 1);
}

/// Registering a second asset when TVL is already at i128::MAX must fail with
/// Overflow (#6). Soroban rolls back the entire invocation, so the second
/// asset must not appear as a registered entry, and the counts/TVL must remain
/// unchanged from before the failed call.
#[test]
fn test_register_overflows_tvl_panics_and_rolls_back() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);

    // First registration: valuation exactly i128::MAX — this must succeed.
    let id1 = register(&env, &client, &issuer, "real_estate", i128::MAX);
    assert_eq!(id1, 1);
    assert_eq!(client.total_value_locked(), i128::MAX);
    assert_eq!(client.asset_count(), 1);
    assert_eq!(client.active_count(), 1);

    // Second registration: any positive valuation would overflow i128 when
    // added to i128::MAX, so this must panic with Error(Contract, #6).
    let token2 = Address::generate(&env);
    let result = client.try_register_asset(
        &issuer,
        &token2,
        &String::from_str(&env, "Asset2"),
        &String::from_str(&env, "invoice"),
        &1i128,
    );
    assert_eq!(result, Err(Ok(Error::Overflow.into())));

    // The failed call must have been fully rolled back:
    // - asset_count is still 1 (no partial write of id=2)
    // - active_count is still 1
    // - TVL is still i128::MAX
    // - get_asset(2) must not exist
    assert_eq!(client.asset_count(), 1);
    assert_eq!(client.active_count(), 1);
    assert_eq!(client.total_value_locked(), i128::MAX);
    assert_eq!(
        client.try_get_asset(&2u64),
        Err(Ok(Error::AssetNotFound.into()))
    );
}

/// Overflow in reactivate_asset: deactivate the giant asset, register another,
/// then reactivate the giant one — the TVL addition would overflow, so
/// reactivate_asset must panic with Error(Contract, #6).
#[test]
fn test_reactivate_overflows_tvl_panics() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);

    // Register an asset with valuation i128::MAX.
    let id_max = register(&env, &client, &issuer, "real_estate", i128::MAX);
    assert_eq!(client.total_value_locked(), i128::MAX);

    // Deactivate it — TVL drops to 0, giving room for another registration.
    client.deactivate_asset(&admin, &id_max);
    assert_eq!(client.total_value_locked(), 0);
    assert_eq!(client.active_count(), 0);

    // Register a second asset with valuation 1. This succeeds because TVL is 0.
    let id2 = register(&env, &client, &issuer, "invoice", 1);
    assert_eq!(client.total_value_locked(), 1);
    assert_eq!(client.active_count(), 1);

    // Now try to reactivate the i128::MAX asset. TVL would become 1 + i128::MAX
    // which overflows — must panic with Overflow (#6).
    let result = client.try_reactivate_asset(&admin, &id_max);
    assert_eq!(result, Err(Ok(Error::Overflow.into())));

    // State must be unchanged: active_count is still 1, TVL is still 1.
    assert_eq!(client.active_count(), 1);
    assert_eq!(client.total_value_locked(), 1);

    // The max-valuation asset must still be inactive (reactivate rolled back).
    assert!(!client.get_asset(&id_max).active);
    // The second asset must still be active.
    assert!(client.get_asset(&id2).active);
}

// ---- pagination helpers (issue #455) ----

/// `get_total_asset_count` returns 0 on an empty registry and increments on
/// every registration regardless of active/inactive status.
#[test]
fn test_get_total_asset_count_empty_and_grows() {
    let (env, client, _admin) = setup();
    assert_eq!(client.get_total_asset_count(), 0);
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "real_estate", 1000);
    assert_eq!(client.get_total_asset_count(), 1);
    register(&env, &client, &issuer, "invoice", 500);
    assert_eq!(client.get_total_asset_count(), 2);
}

/// `get_assets_page` with a zero start_index and a large page_size returns
/// all registered assets (up to MAX_PAGE_SIZE).
#[test]
fn test_get_assets_page_all_in_one_shot() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    for _ in 0..5 {
        register(&env, &client, &issuer, "real_estate", 100);
    }
    let page = client.get_assets_page(&0u32, &10u32);
    assert_eq!(page.len(), 5);
    // ids must be 1-based and ordered
    for (i, entry) in page.iter().enumerate() {
        assert_eq!(entry.id, (i as u64) + 1);
    }
}

/// `get_assets_page` on an empty registry returns an empty vec.
#[test]
fn test_get_assets_page_empty_registry() {
    let (_env, client, _admin) = setup();
    let page = client.get_assets_page(&0u32, &10u32);
    assert_eq!(page.len(), 0);
}

/// `get_assets_page` with start_index past the end returns an empty vec.
#[test]
fn test_get_assets_page_out_of_bounds_returns_empty() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "bond", 100);
    // start_index = 1 is beyond the single registered asset.
    let page = client.get_assets_page(&1u32, &10u32);
    assert_eq!(page.len(), 0);
}

/// `get_assets_page` returns the correct final partial page.
#[test]
fn test_get_assets_page_final_partial_page() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    for _ in 0..5 {
        register(&env, &client, &issuer, "equity", 50);
    }
    // Page 0: items 0-1
    let p0 = client.get_assets_page(&0u32, &2u32);
    assert_eq!(p0.len(), 2);
    // Page 1: items 2-3
    let p1 = client.get_assets_page(&2u32, &2u32);
    assert_eq!(p1.len(), 2);
    // Page 2: item 4 (partial)
    let p2 = client.get_assets_page(&4u32, &2u32);
    assert_eq!(p2.len(), 1);
    assert_eq!(p2.get(0).unwrap().id, 5);
    // Page 3: past the end
    let p3 = client.get_assets_page(&5u32, &2u32);
    assert_eq!(p3.len(), 0);
}

/// page_size = 0 is clamped to MAX_PAGE_SIZE.
#[test]
fn test_get_assets_page_zero_size_clamped_to_max() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "fund", 10);
    let page = client.get_assets_page(&0u32, &0u32);
    // clamped to MAX_PAGE_SIZE; only 1 asset exists so we get 1 back
    assert_eq!(page.len(), 1);
}

/// `get_active_assets_page` with active_only=true returns only active assets.
#[test]
fn test_get_active_assets_page_active_only() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id1 = register(&env, &client, &issuer, "real_estate", 200);
    let _id2 = register(&env, &client, &issuer, "invoice", 100);
    let id3 = register(&env, &client, &issuer, "bond", 50);
    // Deactivate the second asset.
    client.deactivate_asset(&admin, &2u64);

    let page = client.get_active_assets_page(&0u32, &10u32, &true);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap().id, id1);
    assert_eq!(page.get(1).unwrap().id, id3);
}

/// `get_active_assets_page` with active_only=false returns only inactive assets.
#[test]
fn test_get_active_assets_page_inactive_only() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let _id1 = register(&env, &client, &issuer, "real_estate", 200);
    let id2 = register(&env, &client, &issuer, "invoice", 100);
    let _id3 = register(&env, &client, &issuer, "bond", 50);
    client.deactivate_asset(&admin, &id2);

    let page = client.get_active_assets_page(&0u32, &10u32, &false);
    assert_eq!(page.len(), 1);
    assert_eq!(page.get(0).unwrap().id, id2);
    assert!(!page.get(0).unwrap().active);
}

/// `get_active_assets_page` returns empty when nothing matches the filter.
#[test]
fn test_get_active_assets_page_no_match_returns_empty() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "commodity", 10);

    // No inactive assets → active_only=false returns empty.
    let page = client.get_active_assets_page(&0u32, &10u32, &false);
    assert_eq!(page.len(), 0);
}

/// `get_active_assets_page` pagination: start_index skips correctly within the
/// filtered list (not the full registry).
#[test]
fn test_get_active_assets_page_pagination_within_filtered_list() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    for _ in 0..5 {
        register(&env, &client, &issuer, "equity", 100);
    }
    // All 5 are active.
    let p0 = client.get_active_assets_page(&0u32, &2u32, &true);
    assert_eq!(p0.len(), 2);
    let p1 = client.get_active_assets_page(&2u32, &2u32, &true);
    assert_eq!(p1.len(), 2);
    let p2 = client.get_active_assets_page(&4u32, &2u32, &true);
    assert_eq!(p2.len(), 1);
    // Past the end.
    let p3 = client.get_active_assets_page(&5u32, &2u32, &true);
    assert_eq!(p3.len(), 0);
}
