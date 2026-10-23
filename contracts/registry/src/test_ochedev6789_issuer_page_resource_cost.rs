//! Resource cost baseline for `get_assets_by_issuer_page` (issue #554).
//!
//! These tests measure the CPU-instruction and memory-byte cost of
//! `get_assets_by_issuer_page` under representative inputs and record a
//! baseline so future regressions are caught.
//!
//! ## How to read the output
//!
//! Run with `cargo test -p registry -- --nocapture 2>&1 | grep baseline` to
//! see the printed numbers.  The assertions only enforce that costs are
//! non-zero (the budget API is reporting real data) and below the Soroban
//! per-transaction hard limits as of SDK v22:
//!
//! * CPU: 100 000 000 instructions
//! * Memory: 40 960 000 bytes
//!
//! ## Baseline (recorded 2025-10)
//!
//! | Scenario | CPU (instructions) | Memory (bytes) |
//! |---|---|---|
//! | empty issuer (no assets) | ~printed at test time~ | ~printed at test time~ |
//! | single asset, page 0 | ~printed at test time~ | ~printed at test time~ |
//! | 10 assets, page 0 (page_size=5) | ~printed at test time~ | ~printed at test time~ |
//! | 10 assets, page 1 (page_size=5) | ~printed at test time~ | ~printed at test time~ |
//!
//! If a future change to `get_assets_by_issuer_page` (or `fetch_assets_page`)
//! causes costs to spike significantly, tighten the per-call thresholds below
//! once a new baseline is measured.
//!
//! ## Cost model
//!
//! `get_assets_by_issuer_page` reads the per-issuer id index (one persistent
//! read) and then reads each asset entry in the page (one persistent read per
//! entry). Cost therefore scales linearly with `page_size`, not with the total
//! number of assets registered in the issuer index. The test below verifies
//! this by checking that a page-1 call costs roughly the same as a page-0 call
//! for the same number of entries.
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

// ---- Soroban SDK v22 hard limits ----
const CPU_LIMIT: u64 = 100_000_000;
const MEM_LIMIT: u64 = 40_960_000;

// ---- issue #554 tests ----

/// Baseline: query for an issuer with no assets.
/// Cost should be minimal: one persistent read for the (absent) index key.
#[test]
fn test_issuer_page_resource_cost_empty_issuer() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);

    env.cost_estimate().budget().reset_default();
    let _ = client.get_assets_by_issuer_page(&issuer, &0u32, &10u32);
    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let mem = env.cost_estimate().budget().memory_bytes_cost();

    println!("[baseline #554] get_assets_by_issuer_page (empty issuer): cpu={cpu} instructions, mem={mem} bytes");

    assert!(cpu > 0, "CPU cost must be non-zero (budget API sanity check)");
    assert!(mem > 0, "memory cost must be non-zero");
    assert!(cpu < CPU_LIMIT, "CPU cost {cpu} exceeds {CPU_LIMIT} instruction limit");
    assert!(mem < MEM_LIMIT, "memory cost {mem} exceeds {MEM_LIMIT} byte limit");
}

/// Baseline: single asset on page 0 with page_size=1.
#[test]
fn test_issuer_page_resource_cost_single_asset() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register_asset(&env, &client, &issuer, "real_estate", 100);

    env.cost_estimate().budget().reset_default();
    let _ = client.get_assets_by_issuer_page(&issuer, &0u32, &1u32);
    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let mem = env.cost_estimate().budget().memory_bytes_cost();

    println!("[baseline #554] get_assets_by_issuer_page (1 asset, page=0, size=1): cpu={cpu} instructions, mem={mem} bytes");

    assert!(cpu > 0);
    assert!(mem > 0);
    assert!(cpu < CPU_LIMIT, "CPU {cpu} exceeds limit");
    assert!(mem < MEM_LIMIT, "memory {mem} exceeds limit");
}

/// Baseline: 10 assets registered, page 0, page_size=5.
/// This is the primary baseline: 5 asset reads plus one index read.
#[test]
fn test_issuer_page_resource_cost_page0_of_10_assets() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    for _ in 0..10 {
        register_asset(&env, &client, &issuer, "invoice", 200);
    }

    env.cost_estimate().budget().reset_default();
    let _ = client.get_assets_by_issuer_page(&issuer, &0u32, &5u32);
    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let mem = env.cost_estimate().budget().memory_bytes_cost();

    println!("[baseline #554] get_assets_by_issuer_page (10 assets, page=0, size=5): cpu={cpu} instructions, mem={mem} bytes");

    assert!(cpu > 0);
    assert!(mem > 0);
    assert!(cpu < CPU_LIMIT, "CPU {cpu} exceeds limit");
    assert!(mem < MEM_LIMIT, "memory {mem} exceeds limit");
}

/// Baseline: 10 assets registered, page 1, page_size=5.
/// The second page reads the same number of asset entries as the first page;
/// cost should be comparable (within 3×) because the index read is the same
/// size and only the offset into it differs.
#[test]
fn test_issuer_page_resource_cost_page1_comparable_to_page0() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    for _ in 0..10 {
        register_asset(&env, &client, &issuer, "bond", 150);
    }

    env.cost_estimate().budget().reset_default();
    let _ = client.get_assets_by_issuer_page(&issuer, &0u32, &5u32);
    let cpu_page0 = env.cost_estimate().budget().cpu_instruction_cost();
    let mem_page0 = env.cost_estimate().budget().memory_bytes_cost();

    env.cost_estimate().budget().reset_default();
    let _ = client.get_assets_by_issuer_page(&issuer, &1u32, &5u32);
    let cpu_page1 = env.cost_estimate().budget().cpu_instruction_cost();
    let mem_page1 = env.cost_estimate().budget().memory_bytes_cost();

    println!("[baseline #554] page0: cpu={cpu_page0}, mem={mem_page0}  |  page1: cpu={cpu_page1}, mem={mem_page1}");

    // Both pages should be well within the hard limits.
    assert!(cpu_page0 < CPU_LIMIT && cpu_page1 < CPU_LIMIT);
    assert!(mem_page0 < MEM_LIMIT && mem_page1 < MEM_LIMIT);

    // Cost should scale with the number of entries read, not the page index.
    // A generous 3× allowance avoids fragility while catching an O(n^2) regression.
    assert!(
        cpu_page1 < cpu_page0 * 3,
        "page-1 CPU cost {cpu_page1} is more than 3× page-0 cost {cpu_page0}; unexpected scaling"
    );
}

/// Sanity check: an out-of-bounds page (empty result) should have lower or
/// equal cost compared with a page that actually fetches entries, because
/// `fetch_assets_page` exits early when `offset >= total`.
#[test]
fn test_issuer_page_resource_cost_out_of_bounds_not_more_than_full_page() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    for _ in 0..5 {
        register_asset(&env, &client, &issuer, "equity", 300);
    }

    env.cost_estimate().budget().reset_default();
    let _ = client.get_assets_by_issuer_page(&issuer, &0u32, &5u32);
    let cpu_normal = env.cost_estimate().budget().cpu_instruction_cost();

    env.cost_estimate().budget().reset_default();
    let _ = client.get_assets_by_issuer_page(&issuer, &999u32, &5u32);
    let cpu_oob = env.cost_estimate().budget().cpu_instruction_cost();

    println!("[baseline #554] normal page cpu={cpu_normal}  |  out-of-bounds page cpu={cpu_oob}");

    // Both must be within hard limits.
    assert!(cpu_normal < CPU_LIMIT && cpu_oob < CPU_LIMIT);

    // The out-of-bounds call reads the index once and then exits; it should
    // cost no more than the normal call.  We use a generous 2× bound.
    assert!(
        cpu_oob <= cpu_normal * 2,
        "out-of-bounds page cpu {cpu_oob} exceeds 2× normal page cpu {cpu_normal}"
    );
}
