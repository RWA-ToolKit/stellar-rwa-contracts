//! Tests for `get_assets_page` with zero / empty-registry inputs (issue #567).
//!
//! # Covered behaviours
//!
//! | scenario | `start_index` | `page_size` | expected result |
//! |---|---|---|---|
//! | empty registry, zero start | 0 | 10 | empty `Vec` |
//! | empty registry, zero page_size | 0 | 0 | empty `Vec` |
//! | non-empty registry, zero page_size | 0 | 0 | up to `MAX_PAGE_SIZE` assets |
//! | non-empty registry, start_index = 0 | 0 | 1 | first asset |
//! | non-empty registry, both zero | 0 | 0 | up to `MAX_PAGE_SIZE` assets |
//!
//! These are deliberate, documented behaviours rather than accidental edge cases:
//!
//! * An empty registry always returns an empty `Vec` — there is nothing to page
//!   through.
//! * `page_size = 0` is treated as "use the cap" (`MAX_PAGE_SIZE`), not "return
//!   nothing".  This makes zero a safe default value for callers that have not yet
//!   decided on a page size.
//! * `start_index = 0` is the natural first page; the function converts it to the
//!   1-based id internally.

#![cfg(test)]

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

fn register_one(env: &Env, client: &RegistryContractClient) -> u64 {
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    client.register_asset(
        &issuer,
        &token,
        &String::from_str(env, "Asset"),
        &String::from_str(env, "real_estate"),
        &100,
    )
}

// ---------------------------------------------------------------------------
// Empty-registry cases
// ---------------------------------------------------------------------------

/// Calling `get_assets_page` on a registry that has no assets returns an empty
/// `Vec`, regardless of the requested `start_index` or `page_size`.
#[test]
fn test_get_assets_page_empty_registry_zero_start_returns_empty() {
    let (_env, client, _admin) = setup();
    let result = client.get_assets_page(&0u32, &10u32);
    assert_eq!(
        result.len(),
        0,
        "empty registry must return an empty vec for any valid start_index"
    );
}

/// `start_index = 0` and `page_size = 0` on an empty registry still returns an
/// empty `Vec`.  The `page_size = 0` clamping logic runs after the early-return
/// on an empty counter, so both code paths agree.
#[test]
fn test_get_assets_page_empty_registry_both_zero_returns_empty() {
    let (_env, client, _admin) = setup();
    let result = client.get_assets_page(&0u32, &0u32);
    assert_eq!(
        result.len(),
        0,
        "empty registry with zero page_size must still return an empty vec"
    );
}

/// `page_size = 0` on an empty registry returns an empty `Vec` (not a panic).
#[test]
fn test_get_assets_page_empty_registry_zero_page_size_returns_empty() {
    let (_env, client, _admin) = setup();
    let result = client.get_assets_page(&0u32, &0u32);
    assert_eq!(result.len(), 0);
}

// ---------------------------------------------------------------------------
// Non-empty registry — zero page_size clamping
// ---------------------------------------------------------------------------

/// When `page_size = 0` is supplied on a non-empty registry the function
/// clamps it to `MAX_PAGE_SIZE` internally and returns up to that many assets.
/// This is deliberate: zero is a safe sentinel meaning "use the default cap".
#[test]
fn test_get_assets_page_zero_page_size_clamped_to_max_non_empty() {
    let (env, client, _admin) = setup();
    register_one(&env, &client);
    // page_size = 0 → clamped to MAX_PAGE_SIZE; only 1 asset exists so 1 is returned
    let result = client.get_assets_page(&0u32, &0u32);
    assert_eq!(
        result.len(),
        1,
        "page_size=0 must clamp to MAX_PAGE_SIZE, not return zero items"
    );
}

/// Verifies that `page_size = 0` clamping never causes a panic and always
/// returns the correct assets when more assets exist.
#[test]
fn test_get_assets_page_zero_page_size_returns_multiple_assets() {
    let (env, client, _admin) = setup();
    for _ in 0..3 {
        register_one(&env, &client);
    }
    let result = client.get_assets_page(&0u32, &0u32);
    assert_eq!(
        result.len(),
        3,
        "page_size=0 with 3 assets must return all 3 (clamped to MAX_PAGE_SIZE)"
    );
}

// ---------------------------------------------------------------------------
// Non-empty registry — zero start_index
// ---------------------------------------------------------------------------

/// `start_index = 0` addresses the first registered asset (id 1).
/// The function must return that asset as the first element.
#[test]
fn test_get_assets_page_zero_start_index_returns_first_asset() {
    let (env, client, _admin) = setup();
    let id = register_one(&env, &client);
    let result = client.get_assets_page(&0u32, &1u32);
    assert_eq!(result.len(), 1);
    assert_eq!(
        result.get(0).unwrap().id,
        id,
        "start_index=0 must map to id=1, the first registered asset"
    );
}

// ---------------------------------------------------------------------------
// Non-empty registry — both inputs zero
// ---------------------------------------------------------------------------

/// Both `start_index` and `page_size` being zero is handled gracefully: zero
/// start maps to the first asset, zero page_size clamps to MAX_PAGE_SIZE, so
/// the call succeeds and returns existing assets.
#[test]
fn test_get_assets_page_both_zero_returns_assets() {
    let (env, client, _admin) = setup();
    register_one(&env, &client);
    register_one(&env, &client);
    let result = client.get_assets_page(&0u32, &0u32);
    assert_eq!(
        result.len(),
        2,
        "start=0 and page_size=0 must return all assets up to MAX_PAGE_SIZE"
    );
    // Results are in ascending id order.
    assert_eq!(result.get(0).unwrap().id, 1);
    assert_eq!(result.get(1).unwrap().id, 2);
}
