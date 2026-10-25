#![cfg(test)]
//! #562 — `reactivate_asset`: zero / minimal input behaviour.
//!
//! Registry asset ids are 1-based and monotonically increasing; id `0` is
//! therefore never assigned to any asset.  Calling `reactivate_asset` with
//! id `0` must fail with [`Error::AssetNotFound`] (`Error(Contract, #4)`).
//!
//! This test documents that behaviour as deliberate rather than accidental:
//! id `0` is explicitly out of range, and the contract must reject it cleanly
//! rather than silently succeed, corrupt storage, or panic with an unhelpful
//! message.
//!
//! ## Behaviour documented here
//!
//! | input                              | expected outcome                        |
//! |------------------------------------|----------------------------------------|
//! | `asset_id = 0`                     | `Error::AssetNotFound` (#4)            |
//! | `asset_id` beyond registered range | `Error::AssetNotFound` (#4)            |
//! | already-active asset               | no-op (idempotent), no error            |
//! | already-active asset with id=1     | no-op, TVL and active_count unchanged   |

use super::*;
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

/// `reactivate_asset(0)` must return `AssetNotFound`.
///
/// Id `0` is the minimal / "zero" input for this entry point.  It is never
/// assigned (ids start at 1), so this call must be rejected cleanly.  The
/// registry's state must be completely unchanged after the failed call.
#[test]
fn reactivate_asset_id_zero_returns_asset_not_found() {
    let (_env, client, admin) = setup();
    assert_eq!(
        client.try_reactivate_asset(&admin, &0u64),
        Err(Ok(Error::AssetNotFound.into())),
        "id 0 must be rejected with AssetNotFound — it is never assigned"
    );
}

/// After calling `reactivate_asset(0)`, all counters remain at their
/// pre-call values (the failed invocation must be fully rolled back).
#[test]
fn reactivate_asset_id_zero_leaves_state_unchanged() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "bond", 500);

    let count_before = client.asset_count();
    let active_before = client.active_count();
    let tvl_before = client.total_value_locked();

    // id 0 does not exist — must panic and roll back.
    let _ = client.try_reactivate_asset(&admin, &0u64);

    assert_eq!(client.asset_count(), count_before, "asset_count must be unchanged");
    assert_eq!(client.active_count(), active_before, "active_count must be unchanged");
    assert_eq!(client.total_value_locked(), tvl_before, "TVL must be unchanged");
}

/// An id just beyond the registered range is also rejected with
/// `AssetNotFound`.  This complements the id=0 case and confirms that the
/// validation is not specific to zero but covers any non-existent id.
#[test]
fn reactivate_asset_id_beyond_range_returns_asset_not_found() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "real_estate", 1_000);
    // Only id 1 exists; id 2 has never been registered.
    assert_eq!(
        client.try_reactivate_asset(&admin, &2u64),
        Err(Ok(Error::AssetNotFound.into())),
        "id 2 does not exist — must be rejected with AssetNotFound"
    );
}

/// Calling `reactivate_asset` on an asset that is **already active** is a
/// documented no-op: it returns without error, emits no event, and leaves
/// TVL and active_count unchanged.  This is the minimal-meaningful-input case
/// for a valid id.
#[test]
fn reactivate_already_active_asset_is_noop_and_leaves_state_unchanged() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "invoice", 250);

    // Asset was just registered so it is active.
    assert!(client.get_asset(&id).active);
    let active_before = client.active_count();
    let tvl_before = client.total_value_locked();

    // Reactivate an already-active asset — must be a no-op.
    client.reactivate_asset(&admin, &id);

    assert_eq!(client.active_count(), active_before, "active_count must be unchanged");
    assert_eq!(client.total_value_locked(), tvl_before, "TVL must be unchanged");
    assert!(client.get_asset(&id).active, "asset must still be active");
}

/// On an empty registry (no assets registered yet) every id is out of range.
/// id=0 and id=1 must both return `AssetNotFound`.
#[test]
fn reactivate_asset_on_empty_registry_returns_asset_not_found() {
    let (_env, client, admin) = setup();
    assert_eq!(
        client.try_reactivate_asset(&admin, &0u64),
        Err(Ok(Error::AssetNotFound.into())),
        "id 0 on empty registry must return AssetNotFound"
    );
    assert_eq!(
        client.try_reactivate_asset(&admin, &1u64),
        Err(Ok(Error::AssetNotFound.into())),
        "id 1 on empty registry must return AssetNotFound"
    );
}

/// Non-admin callers are still rejected even when the id is valid.
/// This ensures the authorization check fires before the active-flag check.
#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn reactivate_asset_non_admin_rejected_before_state_check() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "commodity", 100);
    client.deactivate_asset(&admin, &id);
    // A stranger — not the admin or the asset issuer — must be rejected.
    let stranger = Address::generate(&env);
    client.reactivate_asset(&stranger, &id);
}
