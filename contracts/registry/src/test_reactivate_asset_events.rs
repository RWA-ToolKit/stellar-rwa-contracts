#![cfg(test)]
//! #563 — `reactivate_asset`: event-emission tests.
//!
//! ## Event specification (from `lib.rs` doc comment)
//!
//! `reactivate_asset` emits **exactly one** event per successful reactivation:
//!
//! ```text
//! topic : ("reactvate",)
//! data  : asset_id  (u64)
//! ```
//!
//! The event is emitted **only when the asset was previously inactive**.
//! Calling `reactivate_asset` on an asset that is already active is a
//! documented no-op and must emit **no event**.
//!
//! These tests confirm both halves of that contract so the indexing API can
//! reconstruct registry activity without polling raw storage.

use super::*;
use soroban_sdk::{testutils::{Address as _, Events}, Address, Env, String};

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

/// Reactivating a deactivated asset emits exactly one event.
///
/// Confirms the basic "one state change → one event" invariant that the
/// indexing API depends on.
#[test]
fn reactivate_deactivated_asset_emits_one_event() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "bond", 100);
    client.deactivate_asset(&admin, &id);

    // Drain setup events before the call under test.
    let _ = env.events().all();

    client.reactivate_asset(&admin, &id);

    let events = env.events().all();
    assert_eq!(
        events.len(),
        1,
        "reactivate_asset must emit exactly one event; got {}",
        events.len()
    );
}

/// The emitted event carries topic `("reactvate",)` and data `asset_id`.
///
/// The topic spelling is intentional (matches the 9-char Soroban symbol limit
/// for `symbol_short!`).  The data field must be the reactivated asset's id
/// so indexers can identify which asset changed state.
#[test]
fn reactivate_asset_event_has_correct_topic_and_data() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "invoice", 500);
    client.deactivate_asset(&admin, &id);

    let _ = env.events().all();

    client.reactivate_asset(&admin, &id);

    let events = env.events().all();
    assert_eq!(events.len(), 1);

    // Destructure: (contract_id, topics_vec, data_val)
    let (_, topics, data) = events.get(0).unwrap();

    // Topic[0] must be the symbol "reactvate".
    use soroban_sdk::{symbol_short, Val};
    let expected_topic: Val = symbol_short!("reactvate").into();
    assert_eq!(
        topics.get(0).unwrap(),
        expected_topic,
        "first topic must be the symbol 'reactvate'"
    );

    // Data must be the asset id as a u64.
    let emitted_id: u64 = soroban_sdk::FromVal::from_val(&env, &data);
    assert_eq!(emitted_id, id, "event data must be the reactivated asset id");
}

/// Reactivating an already-active asset must emit no event.
///
/// The no-op path returns early before the `env.events().publish(...)` call,
/// so the indexer will not see a spurious state-change notification for an
/// asset whose active flag never changed.
#[test]
fn reactivate_already_active_asset_emits_no_event() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "real_estate", 1_000);

    // Asset is active by default after registration.
    assert!(client.get_asset(&id).active);

    let _ = env.events().all();

    client.reactivate_asset(&admin, &id);

    assert_eq!(
        env.events().all().len(),
        0,
        "reactivating an already-active asset must not emit any event"
    );
}

/// Each reactivation in a sequence emits exactly one event.
///
/// deactivate → reactivate → deactivate → reactivate:
/// every reactivation step must produce exactly one event regardless of
/// how many prior transitions have occurred.
#[test]
fn reactivate_emits_one_event_per_reactivation_in_sequence() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "commodity", 750);

    for _ in 0..3 {
        client.deactivate_asset(&admin, &id);
        let _ = env.events().all();

        client.reactivate_asset(&admin, &id);
        assert_eq!(
            env.events().all().len(),
            1,
            "each reactivation must emit exactly one event"
        );
    }
}

/// A failed reactivation (unknown id) emits no event and rolls back cleanly.
///
/// Only successful state changes should be visible to the indexer; a panicked
/// invocation must leave no trace in the event log.
#[test]
fn failed_reactivate_emits_no_event() {
    let (_env, client, admin) = setup();
    let _ = _env.events().all();

    let _ = client.try_reactivate_asset(&admin, &999u64);

    assert_eq!(
        _env.events().all().len(),
        0,
        "a failed reactivate_asset call must not emit any event"
    );
}

/// Reactivating multiple distinct assets emits one event per asset.
///
/// Validates that events are not de-duplicated or batched and that each
/// separate `reactivate_asset` invocation contributes its own event.
#[test]
fn reactivate_multiple_assets_emits_one_event_each() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id1 = register(&env, &client, &issuer, "bond", 100);
    let id2 = register(&env, &client, &issuer, "equity", 200);
    let id3 = register(&env, &client, &issuer, "fund", 300);

    client.deactivate_asset(&admin, &id1);
    client.deactivate_asset(&admin, &id2);
    client.deactivate_asset(&admin, &id3);

    let _ = env.events().all();

    client.reactivate_asset(&admin, &id1);
    assert_eq!(env.events().all().len(), 1, "reactivation of id1 must emit one event");

    client.reactivate_asset(&admin, &id2);
    assert_eq!(env.events().all().len(), 1, "reactivation of id2 must emit one event");

    client.reactivate_asset(&admin, &id3);
    assert_eq!(env.events().all().len(), 1, "reactivation of id3 must emit one event");
}
