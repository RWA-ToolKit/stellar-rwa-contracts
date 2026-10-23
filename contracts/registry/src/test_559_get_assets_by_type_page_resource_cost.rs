#![cfg(test)]
//! #559 — `get_assets_by_type_page`: resource-cost baseline.
//!
//! ## Baseline (recorded 2026-10-23, soroban-sdk 26)
//!
//! | scenario                           | cpu (instructions) | mem (bytes) |
//! |------------------------------------|-------------------:|------------:|
//! | empty type index                   |           < 50 000 |    < 50 000 |
//! | single-asset page                  |          < 500 000 |   < 200 000 |
//! | full page (10 assets, page_size=10)|        < 5 000 000 | < 1 000 000 |
//! | large page (50 assets, page_size=50)|      < 25 000 000 | < 5 000 000 |
//!
//! Run with `cargo test -p registry test_559 -- --nocapture` to print actual
//! observed values. Flag any result that approaches or exceeds a ceiling for
//! optimisation review.
//!
//! ## Why these ceilings?
//! The ceilings are generous multiples of the observed baselines so the suite
//! is not brittle across minor SDK upgrades. They are deliberately set to
//! catch order-of-magnitude regressions (e.g. accidentally O(n²) scans),
//! not micro-optimisation drift.

extern crate std;
use std::println;

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

/// Empty type index — lowest possible cost.
///
/// A miss on the TypeIndex persistent key returns an empty Vec immediately.
/// Even this "do-nothing" path must stay well under 50 000 cpu instructions.
#[test]
fn get_assets_by_type_page_cost_empty_index() {
    let (env, client, _admin) = setup();
    env.cost_estimate().budget().reset_default();

    let results = client.get_assets_by_type_page(
        &String::from_str(&env, "commodity"),
        &0u32,
        &10u32,
    );
    assert_eq!(results.len(), 0);

    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let mem = env.cost_estimate().budget().memory_bytes_cost();
    println!("[#559] empty-index: cpu={cpu} mem={mem}");

    assert!(
        cpu < 50_000,
        "empty-index cpu cost unexpectedly high: {cpu}"
    );
    assert!(
        mem < 50_000,
        "empty-index mem cost unexpectedly high: {mem}"
    );
}

/// Single-asset type index — baseline for the minimal realistic case.
#[test]
fn get_assets_by_type_page_cost_single_asset() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "invoice", 1_000);

    env.cost_estimate().budget().reset_default();
    let results = client.get_assets_by_type_page(
        &String::from_str(&env, "invoice"),
        &0u32,
        &10u32,
    );
    assert_eq!(results.len(), 1);

    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let mem = env.cost_estimate().budget().memory_bytes_cost();
    println!("[#559] single-asset: cpu={cpu} mem={mem}");

    assert!(
        cpu < 500_000,
        "single-asset cpu cost unexpectedly high: {cpu}"
    );
    assert!(
        mem < 200_000,
        "single-asset mem cost unexpectedly high: {mem}"
    );
}

/// Full page (10 assets) — scales with page_size, not registry size.
///
/// `get_assets_by_type_page` reads only `page_size` entries from the
/// TypeIndex Vec; it should not scan the whole registry.
#[test]
fn get_assets_by_type_page_cost_full_page_10() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    for _ in 0..10 {
        register(&env, &client, &issuer, "real_estate", 500);
    }
    // Register some other-type assets to ensure the function doesn't scan them.
    for _ in 0..10 {
        register(&env, &client, &issuer, "bond", 100);
    }

    env.cost_estimate().budget().reset_default();
    let results = client.get_assets_by_type_page(
        &String::from_str(&env, "real_estate"),
        &0u32,
        &10u32,
    );
    assert_eq!(results.len(), 10);

    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let mem = env.cost_estimate().budget().memory_bytes_cost();
    println!("[#559] full-page-10: cpu={cpu} mem={mem}");

    assert!(
        cpu < 5_000_000,
        "full-page-10 cpu cost unexpectedly high: {cpu}"
    );
    assert!(
        mem < 1_000_000,
        "full-page-10 mem cost unexpectedly high: {mem}"
    );
}

/// Larger page (50 assets) — verifies cost scales roughly linearly.
///
/// If the ceiling were breached here but not for page-10, it would indicate
/// a super-linear growth (e.g., repeated full-index deserialization) worth
/// investigating.
#[test]
fn get_assets_by_type_page_cost_full_page_50() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    for _ in 0..50 {
        register(&env, &client, &issuer, "equity", 200);
    }

    env.cost_estimate().budget().reset_default();
    let results = client.get_assets_by_type_page(
        &String::from_str(&env, "equity"),
        &0u32,
        &50u32,
    );
    assert_eq!(results.len(), 50);

    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let mem = env.cost_estimate().budget().memory_bytes_cost();
    println!("[#559] full-page-50: cpu={cpu} mem={mem}");

    assert!(
        cpu < 25_000_000,
        "full-page-50 cpu cost unexpectedly high: {cpu}"
    );
    assert!(
        mem < 5_000_000,
        "full-page-50 mem cost unexpectedly high: {mem}"
    );
}

/// Cost must not grow between a first-page and second-page call of equal size.
///
/// Both pages iterate exactly `page_size` entries from the TypeIndex;
/// the offset is computed arithmetically and does not incur an additional
/// storage scan, so the two costs should be within 2× of each other.
#[test]
fn get_assets_by_type_page_cost_page1_vs_page2_comparable() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    for _ in 0..20 {
        register(&env, &client, &issuer, "fund", 300);
    }

    env.cost_estimate().budget().reset_default();
    client.get_assets_by_type_page(&String::from_str(&env, "fund"), &0u32, &10u32);
    let cpu_page0 = env.cost_estimate().budget().cpu_instruction_cost();

    env.cost_estimate().budget().reset_default();
    client.get_assets_by_type_page(&String::from_str(&env, "fund"), &1u32, &10u32);
    let cpu_page1 = env.cost_estimate().budget().cpu_instruction_cost();

    println!("[#559] page0 cpu={cpu_page0}, page1 cpu={cpu_page1}");
    assert!(
        cpu_page1 <= cpu_page0 * 2,
        "page1 cost {cpu_page1} is more than 2× page0 cost {cpu_page0} — super-linear growth suspected"
    );
}
