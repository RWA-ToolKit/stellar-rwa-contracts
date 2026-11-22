//! Resource cost for `get_all_assets` (issue #716).
//!
//! `get_all_assets` already **silently clamps** `limit` to [`MAX_PAGE_SIZE`]
//! (100) rather than rejecting it; that behaviour is kept, because every other
//! paginated getter in this contract clamps the same way and callers rely on
//! "ask for a lot, get a page". What was missing is proof that the clamp
//! actually bounds cost: a caller passing `u32::MAX` must pay for one page of
//! `MAX_PAGE_SIZE` reads, not for an allocation or read loop sized by `limit`.
//!
//! Run `cargo test -p registry all_assets_resource -- --nocapture` to see the
//! measured numbers.
#![cfg(test)]

extern crate std;
use std::println;

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, String};

const CPU_LIMIT: u64 = 100_000_000;
const MEM_LIMIT: u64 = 40_960_000;

/// A registry holding `n` assets, registered with an unlimited budget so that
/// setup cost never leaks into what the tests measure.
fn setup_with_assets(n: u64) -> (Env, RegistryContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();
    let contract_id = env.register(RegistryContract, ());
    let client = RegistryContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let issuer = Address::generate(&env);
    for _ in 0..n {
        let token = Address::generate(&env);
        client.register_asset(
            &issuer,
            &token,
            &String::from_str(&env, "Asset"),
            &String::from_str(&env, "bond"),
            &1_000i128,
        );
    }
    (env, client)
}

fn measure(
    env: &Env,
    client: &RegistryContractClient,
    start_id: u64,
    limit: u32,
) -> (u32, u64, u64) {
    env.cost_estimate().budget().reset_default();
    let page = client.get_all_assets(&start_id, &limit);
    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let mem = env.cost_estimate().budget().memory_bytes_cost();
    (page.len(), cpu, mem)
}

/// An oversized `limit` is clamped to `MAX_PAGE_SIZE` and costs the same as
/// asking for exactly `MAX_PAGE_SIZE`: the cap bounds the work, not just the
/// response length.
#[test]
fn test_all_assets_oversized_limit_costs_one_capped_page() {
    let (env, client) = setup_with_assets(MAX_PAGE_SIZE as u64 + 50);

    let (len_cap, cpu_cap, mem_cap) = measure(&env, &client, 1, MAX_PAGE_SIZE);
    let (len_max, cpu_max, mem_max) = measure(&env, &client, 1, u32::MAX);
    println!(
        "[#716] get_all_assets limit={MAX_PAGE_SIZE}: {len_cap} rows, cpu={cpu_cap}, mem={mem_cap}"
    );
    println!("[#716] get_all_assets limit=u32::MAX: {len_max} rows, cpu={cpu_max}, mem={mem_max}");

    assert_eq!(len_cap, MAX_PAGE_SIZE);
    assert_eq!(len_max, MAX_PAGE_SIZE, "limit is clamped, not rejected");
    // Same work within 5%: nothing in the call scales with the requested limit.
    assert!(
        cpu_max <= cpu_cap + cpu_cap / 20,
        "cpu {cpu_max} vs capped {cpu_cap}"
    );
    assert!(
        mem_max <= mem_cap + mem_cap / 20,
        "mem {mem_max} vs capped {mem_cap}"
    );
    assert!(
        cpu_max < CPU_LIMIT && mem_max < MEM_LIMIT,
        "a full page stays within the transaction limits"
    );
}

/// Cost scales with the page actually returned, so a small page is cheaper
/// than a full one, and a page past the last id is close to free.
#[test]
fn test_all_assets_cost_scales_with_page_not_registry_size() {
    let (env, client) = setup_with_assets(MAX_PAGE_SIZE as u64 + 50);

    let (len_small, cpu_small, _) = measure(&env, &client, 1, 10);
    let (_, cpu_full, _) = measure(&env, &client, 1, MAX_PAGE_SIZE);
    let (len_past, cpu_past, _) = measure(&env, &client, 10_000, MAX_PAGE_SIZE);
    println!("[#716] get_all_assets limit=10: cpu={cpu_small}; limit={MAX_PAGE_SIZE}: cpu={cpu_full}; past end: cpu={cpu_past}");

    assert_eq!(len_small, 10);
    assert_eq!(len_past, 0);
    assert!(cpu_small < cpu_full);
    assert!(cpu_past < cpu_small);
}
