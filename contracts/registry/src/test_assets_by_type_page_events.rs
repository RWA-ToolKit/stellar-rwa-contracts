#![cfg(test)]
//! #558 — `get_assets_by_type_page`: confirm event-emission behaviour.
//!
//! `get_assets_by_type_page` is a **read-only** query: it does not write to
//! storage and does not change any contract state.  Because Soroban events are
//! emitted to signal state changes for indexers, a pure read has no state to
//! signal and therefore emits **no events**.
//!
//! These tests confirm that invariant explicitly so that any future refactor
//! that accidentally adds an event (or accidentally turns this into a
//! state-changing call) is immediately caught.

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

/// Calling `get_assets_by_type_page` on a non-empty result set emits no events.
///
/// Rationale: the function is a pure read over a pre-built index; it changes
/// no persistent or instance storage, so there is no state change to signal to
/// the indexing API.
#[test]
fn get_assets_by_type_page_emits_no_events_on_hit() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "real_estate", 1_000);
    register(&env, &client, &issuer, "real_estate", 2_000);
    register(&env, &client, &issuer, "invoice", 500);

    // Drain any events produced by the setup calls.
    let _ = env.events().all();

    // Now perform the read — must not produce any new events.
    let results = client.get_assets_by_type_page(
        &String::from_str(&env, "real_estate"),
        &0u32,
        &10u32,
    );
    assert_eq!(results.len(), 2, "expected two real_estate assets");
    assert_eq!(
        env.events().all().len(),
        0,
        "get_assets_by_type_page must not emit events (it is a read-only query)"
    );
}

/// Calling `get_assets_by_type_page` on a type with no registered assets
/// also emits no events.
#[test]
fn get_assets_by_type_page_emits_no_events_on_empty_result() {
    let (env, client, _admin) = setup();

    let _ = env.events().all();

    let results = client.get_assets_by_type_page(
        &String::from_str(&env, "commodity"),
        &0u32,
        &10u32,
    );
    assert_eq!(results.len(), 0, "no commodity assets registered");
    assert_eq!(
        env.events().all().len(),
        0,
        "get_assets_by_type_page must not emit events even when the result is empty"
    );
}

/// Paginating across multiple pages produces no events on any page.
#[test]
fn get_assets_by_type_page_emits_no_events_across_multiple_pages() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    for _ in 0..5 {
        register(&env, &client, &issuer, "bond", 100);
    }

    let _ = env.events().all();

    // Page 0 and page 1.
    let p0 = client.get_assets_by_type_page(&String::from_str(&env, "bond"), &0u32, &3u32);
    assert_eq!(p0.len(), 3);
    assert_eq!(
        env.events().all().len(),
        0,
        "page 0 must emit no events"
    );

    let p1 = client.get_assets_by_type_page(&String::from_str(&env, "bond"), &1u32, &3u32);
    assert_eq!(p1.len(), 2);
    assert_eq!(
        env.events().all().len(),
        0,
        "page 1 must emit no events"
    );
}

/// Calling `get_assets_by_type_page` with an out-of-range page offset also
/// emits no events.
#[test]
fn get_assets_by_type_page_emits_no_events_out_of_range_page() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "equity", 500);

    let _ = env.events().all();

    let results = client.get_assets_by_type_page(
        &String::from_str(&env, "equity"),
        &99u32, // far beyond the single registered asset
        &10u32,
    );
    assert_eq!(results.len(), 0);
    assert_eq!(
        env.events().all().len(),
        0,
        "out-of-range page must emit no events"
    );
}
