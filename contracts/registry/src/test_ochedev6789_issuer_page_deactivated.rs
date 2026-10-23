//! Tests for `get_assets_by_issuer_page` while assets are deactivated
//! (issue #555).
//!
//! The registry contract has **no global pause flag**; there is no concept of
//! "pausing the registry" analogous to the asset-token contract's pause. The
//! closest equivalent is deactivating individual assets via `deactivate_asset`.
//!
//! ## Documented behaviour
//!
//! * `get_assets_by_issuer_page` returns **all** assets belonging to an issuer,
//!   both active and deactivated. It is up to the caller to inspect the `active`
//!   field on each [`AssetEntry`] and filter as needed.
//!
//! * Deactivating an asset does **not** remove it from the per-issuer id index;
//!   it only sets `entry.active = false` on the stored entry.
//!
//! * Querying the function while all of an issuer's assets are deactivated is
//!   therefore safe and deliberate: the returned entries have `active == false`
//!   but no panic, error, or empty-result occurs.
//!
//! * There is no global pause to test against. If a global pause is added in
//!   the future, a separate test covering its interaction with this function
//!   should be added.
#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, String};

// ---- helpers ----

fn setup() -> (Env, RegistryContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(RegistryContract, ());
    let client = RegistryContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

fn register_asset(
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

// ---- issue #555 tests ----

/// Deactivating a single asset does not remove it from the paged query.
/// The entry is still returned; only `entry.active` changes to `false`.
#[test]
fn test_issuer_page_deactivated_asset_still_returned() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id = register_asset(&env, &client, &issuer, "real_estate", 500);

    client.deactivate_asset(&admin, &id);
    assert!(!client.get_asset(&id).active);

    let result = client.get_assets_by_issuer_page(&issuer, &0u32, &10u32);
    assert_eq!(result.len(), 1, "deactivated asset must still appear in paged result");
    assert_eq!(result.get(0).unwrap().id, id);
    assert!(
        !result.get(0).unwrap().active,
        "returned entry must reflect deactivated state"
    );
}

/// When every asset belonging to an issuer is deactivated the query still
/// returns all of them (with `active == false`). This is deliberate: the
/// indexer needs to see deactivated assets to reconstruct the full history.
#[test]
fn test_issuer_page_all_assets_deactivated_all_still_returned() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id1 = register_asset(&env, &client, &issuer, "invoice", 100);
    let id2 = register_asset(&env, &client, &issuer, "bond", 200);
    let id3 = register_asset(&env, &client, &issuer, "equity", 300);

    // Deactivate every asset.
    client.deactivate_asset(&admin, &id1);
    client.deactivate_asset(&admin, &id2);
    client.deactivate_asset(&admin, &id3);

    // All three must still appear in the paged result.
    let result = client.get_assets_by_issuer_page(&issuer, &0u32, &10u32);
    assert_eq!(
        result.len(),
        3,
        "all three deactivated assets must still be returned"
    );
    for entry in result.iter() {
        assert!(
            !entry.active,
            "every returned entry must reflect deactivated state"
        );
    }
}

/// Deactivating an asset mid-list does not shift the positions of the other
/// entries in the paged result; the id index is append-only and is never
/// compacted on deactivation.
#[test]
fn test_issuer_page_deactivation_does_not_shift_page_positions() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id1 = register_asset(&env, &client, &issuer, "real_estate", 100);
    let id2 = register_asset(&env, &client, &issuer, "commodity", 200);
    let id3 = register_asset(&env, &client, &issuer, "fund", 300);

    // Record positions before deactivation.
    let before = client.get_assets_by_issuer_page(&issuer, &0u32, &3u32);
    assert_eq!(before.get(0).unwrap().id, id1);
    assert_eq!(before.get(1).unwrap().id, id2);
    assert_eq!(before.get(2).unwrap().id, id3);

    // Deactivate the middle asset.
    client.deactivate_asset(&admin, &id2);

    // Positions must be unchanged; id2 is still at index 1.
    let after = client.get_assets_by_issuer_page(&issuer, &0u32, &3u32);
    assert_eq!(after.len(), 3);
    assert_eq!(after.get(0).unwrap().id, id1);
    assert_eq!(after.get(1).unwrap().id, id2);
    assert_eq!(after.get(2).unwrap().id, id3);
    assert!(after.get(0).unwrap().active);
    assert!(!after.get(1).unwrap().active, "middle asset must be deactivated");
    assert!(after.get(2).unwrap().active);
}

/// After reactivating a previously deactivated asset the paged result reflects
/// the restored `active == true` state.
#[test]
fn test_issuer_page_reflects_reactivated_state() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id = register_asset(&env, &client, &issuer, "bond", 750);

    client.deactivate_asset(&admin, &id);
    let deactivated = client.get_assets_by_issuer_page(&issuer, &0u32, &1u32);
    assert!(!deactivated.get(0).unwrap().active);

    client.reactivate_asset(&admin, &id);
    let reactivated = client.get_assets_by_issuer_page(&issuer, &0u32, &1u32);
    assert_eq!(reactivated.len(), 1);
    assert!(reactivated.get(0).unwrap().active, "reactivated asset must show active=true");
}

/// Callers can distinguish deactivated assets from active ones by inspecting
/// `entry.active`. This test documents the pattern for filtering the paged
/// result to active-only entries.
#[test]
fn test_issuer_page_caller_can_filter_active_from_deactivated() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id_a = register_asset(&env, &client, &issuer, "real_estate", 100);
    let id_b = register_asset(&env, &client, &issuer, "invoice", 200);
    let id_c = register_asset(&env, &client, &issuer, "equity", 300);

    client.deactivate_asset(&admin, &id_b);

    let all = client.get_assets_by_issuer_page(&issuer, &0u32, &10u32);
    assert_eq!(all.len(), 3);

    // Caller-side active filter.
    let active: soroban_sdk::Vec<AssetEntry> = {
        let mut v = soroban_sdk::Vec::new(&env);
        for e in all.iter() {
            if e.active {
                v.push_back(e);
            }
        }
        v
    };

    assert_eq!(active.len(), 2, "only 2 of 3 assets are active");
    assert_eq!(active.get(0).unwrap().id, id_a);
    assert_eq!(active.get(1).unwrap().id, id_c);
}

/// Pagination is consistent across deactivated assets: splitting a list of
/// deactivated assets across two pages returns all of them with no gaps or
/// duplicates.
#[test]
fn test_issuer_page_pagination_consistent_when_all_deactivated() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let mut ids = soroban_sdk::Vec::new(&env);
    for _ in 0..6 {
        ids.push_back(register_asset(&env, &client, &issuer, "commodity", 50));
    }
    // Deactivate all.
    for i in 0..ids.len() {
        client.deactivate_asset(&admin, &ids.get(i).unwrap());
    }

    let page0 = client.get_assets_by_issuer_page(&issuer, &0u32, &3u32);
    let page1 = client.get_assets_by_issuer_page(&issuer, &1u32, &3u32);
    let page2 = client.get_assets_by_issuer_page(&issuer, &2u32, &3u32);

    assert_eq!(page0.len(), 3);
    assert_eq!(page1.len(), 3);
    assert_eq!(page2.len(), 0, "page past end must be empty");

    // No duplicates across pages.
    for e0 in page0.iter() {
        for e1 in page1.iter() {
            assert_ne!(e0.id, e1.id, "duplicate id across pages: {}", e0.id);
        }
    }

    // All returned entries are deactivated.
    for e in page0.iter().chain(page1.iter()) {
        assert!(!e.active);
    }
}
