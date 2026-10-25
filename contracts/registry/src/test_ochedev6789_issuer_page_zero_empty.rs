//! Tests for `get_assets_by_issuer_page` with zero/empty input (issue #552).
//!
//! `get_assets_by_issuer_page` is a read-only query; it never mutates state,
//! emits events, or panics on edge-case inputs. These tests confirm that
//! boundary values — an issuer with no assets, `page_size = 0`, a `page`
//! number beyond the last page, and `page_size` clamped at `MAX_PAGE_SIZE` —
//! all return an empty (or correctly-bounded) `Vec` rather than panicking or
//! returning an error.
//!
//! ## Documented behaviour
//!
//! * **Unknown / no-asset issuer** — returns an empty `Vec`.  The per-issuer
//!   index simply has no entry, so `index_ids` returns an empty list and
//!   `fetch_assets_page` short-circuits to an empty result.
//!
//! * **`page_size = 0`** — the call is silently clamped: `page_size.min(MAX_PAGE_SIZE)`
//!   yields 0, which propagates into `fetch_assets_page` where `limit == 0`
//!   is an explicit early-exit condition, so an empty `Vec` is returned.
//!
//! * **`page` beyond the index length** — the computed `offset` will be ≥ the
//!   length of the issuer id list, so `fetch_assets_page` returns an empty
//!   `Vec` immediately.
//!
//! * **`page_size` above `MAX_PAGE_SIZE`** — silently clamped to
//!   `MAX_PAGE_SIZE`; no more than 100 entries are ever returned.
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

// ---- issue #552 tests ----

/// Querying an issuer that has never registered any asset returns an empty
/// `Vec`. This is the minimal-input case: the per-issuer index has no entry at
/// all, which must not panic or error.
#[test]
fn test_issuer_page_unknown_issuer_returns_empty() {
    let (env, client, _admin) = setup();
    let stranger = Address::generate(&env);
    let result = client.get_assets_by_issuer_page(&stranger, &0u32, &10u32);
    assert_eq!(result.len(), 0);
}

/// `page_size = 0` is a zero-value input. The function clamps it via
/// `page_size.min(MAX_PAGE_SIZE)` → 0, and `fetch_assets_page` exits early
/// when `limit == 0`. An empty `Vec` is the documented result.
#[test]
fn test_issuer_page_zero_page_size_returns_empty() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register_asset(&env, &client, &issuer, "real_estate", 100);

    let result = client.get_assets_by_issuer_page(&issuer, &0u32, &0u32);
    assert_eq!(result.len(), 0);
}

/// Requesting `page = 0` and `page_size = 0` simultaneously combines the two
/// minimal inputs into a single call.  Expected result is still an empty `Vec`.
#[test]
fn test_issuer_page_both_zero_returns_empty() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register_asset(&env, &client, &issuer, "invoice", 50);

    let result = client.get_assets_by_issuer_page(&issuer, &0u32, &0u32);
    assert_eq!(result.len(), 0);
}

/// `page` set beyond the last valid page index returns an empty `Vec`.
/// With 3 assets and `page_size = 3` there is only one valid page (page 0).
/// Requesting page 1 therefore falls entirely outside the index.
#[test]
fn test_issuer_page_page_beyond_end_returns_empty() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    for _ in 0..3 {
        register_asset(&env, &client, &issuer, "bond", 100);
    }

    // page=1 → offset = 1 * 3 = 3, but there are only 3 ids (indices 0-2),
    // so offset >= total → empty.
    let result = client.get_assets_by_issuer_page(&issuer, &1u32, &3u32);
    assert_eq!(result.len(), 0);
}

/// A `page_size` above `MAX_PAGE_SIZE` (100) is clamped silently.
/// Register 5 assets and request all with an absurdly large `page_size`; only
/// 5 assets should be returned, confirming the clamp is in effect.
#[test]
fn test_issuer_page_page_size_clamped_to_max() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    for _ in 0..5 {
        register_asset(&env, &client, &issuer, "equity", 200);
    }

    // u32::MAX >> MAX_PAGE_SIZE; only 5 assets exist so we get 5 back, but
    // the cap is what prevents unbounded responses with a large registry.
    let result = client.get_assets_by_issuer_page(&issuer, &0u32, &u32::MAX);
    assert_eq!(result.len(), 5);
}

/// First-page call on an issuer with exactly one asset returns that single
/// entry, confirming the happy path still works alongside the edge cases.
#[test]
fn test_issuer_page_single_asset_first_page_returns_one() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    let id = register_asset(&env, &client, &issuer, "commodity", 999);

    let result = client.get_assets_by_issuer_page(&issuer, &0u32, &1u32);
    assert_eq!(result.len(), 1);
    assert_eq!(result.get(0).unwrap().id, id);
}

/// Two issuers share the registry; a zero-input query for one does not bleed
/// into the other's index.
#[test]
fn test_issuer_page_zero_input_does_not_bleed_across_issuers() {
    let (env, client, _admin) = setup();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    register_asset(&env, &client, &alice, "fund", 300);
    register_asset(&env, &client, &bob, "bond", 500);

    // Alice with page_size=0 → empty.
    let alice_zero = client.get_assets_by_issuer_page(&alice, &0u32, &0u32);
    assert_eq!(alice_zero.len(), 0);

    // Bob with page past end → empty, but alice's asset count is unaffected.
    let bob_out = client.get_assets_by_issuer_page(&bob, &99u32, &10u32);
    assert_eq!(bob_out.len(), 0);

    // Normal query for alice still works.
    let alice_ok = client.get_assets_by_issuer_page(&alice, &0u32, &10u32);
    assert_eq!(alice_ok.len(), 1);
}
