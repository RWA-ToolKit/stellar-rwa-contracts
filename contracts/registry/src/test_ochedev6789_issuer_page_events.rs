//! Tests for `get_assets_by_issuer_page` event emission (issue #553).
//!
//! `get_assets_by_issuer_page` is a **read-only** query: it never mutates
//! contract state and therefore never emits any events. This is deliberate —
//! read functions that emit events would create spurious index entries for the
//! API to process, increasing noise and cost with no benefit.
//!
//! ## Documented behaviour
//!
//! * `get_assets_by_issuer_page` emits **no events** on any call, regardless
//!   of input.  The function delegates entirely to `index_ids` →
//!   `fetch_assets_page`, neither of which calls `env.events().publish(…)`.
//!
//! * Only state-mutating entry points (`register_asset`, `deactivate_asset`,
//!   `reactivate_asset`, `update_valuation`, `initialize`, `propose_admin`,
//!   `cancel_admin_proposal`, `accept_admin`) emit events; the API can
//!   reconstruct all registry activity from those events alone without any
//!   read-path events.
//!
//! These tests verify the no-event contract so that if someone inadvertently
//! adds an `env.events().publish(…)` call to the read path, CI catches it.
#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, testutils::Events as _, Address, Env, String};

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

// ---- issue #553 tests ----

/// A normal paged query emits no events.
///
/// `events().all()` reflects only the most recent contract invocation, so
/// reading it immediately after `get_assets_by_issuer_page` is authoritative.
#[test]
fn test_issuer_page_emits_no_events_with_results() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register_asset(&env, &client, &issuer, "real_estate", 500);
    register_asset(&env, &client, &issuer, "invoice", 300);

    // Consume the last registration event by calling the read function after.
    let _result = client.get_assets_by_issuer_page(&issuer, &0u32, &10u32);
    assert_eq!(
        env.events().all().events().len(),
        0,
        "get_assets_by_issuer_page must not emit any events"
    );
}

/// A query for an issuer with no assets (empty result) also emits no events.
#[test]
fn test_issuer_page_emits_no_events_on_empty_result() {
    let (env, client, _admin) = setup();
    let stranger = Address::generate(&env);

    let _result = client.get_assets_by_issuer_page(&stranger, &0u32, &10u32);
    assert_eq!(
        env.events().all().events().len(),
        0,
        "read-only query on unknown issuer must emit no events"
    );
}

/// Calling with `page_size = 0` (early-exit path) still emits no events.
#[test]
fn test_issuer_page_emits_no_events_when_page_size_zero() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register_asset(&env, &client, &issuer, "commodity", 100);

    let _result = client.get_assets_by_issuer_page(&issuer, &0u32, &0u32);
    assert_eq!(
        env.events().all().events().len(),
        0,
        "zero page_size early-exit must not emit events"
    );
}

/// Calling with `page` beyond the index still emits no events.
#[test]
fn test_issuer_page_emits_no_events_when_page_out_of_bounds() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register_asset(&env, &client, &issuer, "bond", 200);

    // page=999 is well past the single-asset index.
    let _result = client.get_assets_by_issuer_page(&issuer, &999u32, &10u32);
    assert_eq!(
        env.events().all().events().len(),
        0,
        "out-of-bounds page must not emit events"
    );
}

/// Successive paged calls each emit no events; the event log remains
/// empty across the full paginated walk.
#[test]
fn test_issuer_page_no_events_across_multiple_pages() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    for _ in 0..5 {
        register_asset(&env, &client, &issuer, "equity", 100);
    }

    // Walk all pages in page_size=2 steps.
    for page in 0u32..3 {
        let _result = client.get_assets_by_issuer_page(&issuer, &page, &2u32);
        assert_eq!(
            env.events().all().events().len(),
            0,
            "page {page} must not emit events"
        );
    }
}

/// State-mutating calls (`register_asset`, `deactivate_asset`) do emit events,
/// confirming the event machinery works and that the absence of events on the
/// read path is not a testing artifact.
#[test]
fn test_mutating_calls_emit_events_but_read_path_does_not() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id = register_asset(&env, &client, &issuer, "fund", 999);

    // register_asset emits a "register" event.
    assert_eq!(
        env.events().all().events().len(),
        1,
        "register_asset must emit exactly one event"
    );

    // deactivate_asset emits a "deactvate" event.
    client.deactivate_asset(&admin, &id);
    assert_eq!(
        env.events().all().events().len(),
        1,
        "deactivate_asset must emit exactly one event"
    );

    // The subsequent read must emit nothing.
    let _result = client.get_assets_by_issuer_page(&issuer, &0u32, &10u32);
    assert_eq!(
        env.events().all().events().len(),
        0,
        "read after mutations must still emit no events"
    );
}
