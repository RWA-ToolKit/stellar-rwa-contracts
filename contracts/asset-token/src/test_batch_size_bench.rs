//! #726: per-recipient cost of `mint_batch` / `transfer_batch`, and the
//! largest batch that fits Soroban's per-transaction limits.
//!
//! `cargo test -p asset-token batch_size_bench -- --nocapture --test-threads=1`
#![cfg(test)]
extern crate std;
use std::println;

use super::*;
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, String, Vec};

fn setup() -> (
    Env,
    AssetTokenContractClient<'static>,
    ComplianceContractClient<'static>,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();
    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    let admin = Address::generate(&env);
    compliance.initialize(&admin);
    compliance.add_to_allowlist(&admin, &admin, &String::from_str(&env, "US"), &0);
    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);
    token.initialize(
        &admin,
        &String::from_str(&env, "Loft"),
        &String::from_str(&env, "LFT"),
        &String::from_str(&env, "real_estate"),
        &1_000_000i128,
        &0u32,
        &compliance_id,
        &String::from_str(&env, "desc"),
        &1_000_000i128,
    );
    (env, token, compliance, admin)
}

/// Run one batch of `n` under the real per-transaction limits.
/// Returns Some(cpu) if it fits, None if the host rejected it for resources.
fn run(n: u32, mint: bool) -> Option<u64> {
    let (env, token, compliance, admin) = setup();
    let mut batch = Vec::new(&env);
    for _ in 0..n {
        let a = Address::generate(&env);
        compliance.add_to_allowlist(&admin, &a, &String::from_str(&env, "US"), &0);
        batch.push_back((a, 10i128));
    }
    env.cost_estimate().budget().reset_default();
    let ok = if mint {
        token.try_mint_batch(&admin, &batch).is_ok()
    } else {
        token.try_transfer_batch(&admin, &batch).is_ok()
    };
    ok.then(|| env.cost_estimate().budget().cpu_instruction_cost())
}

/// Like `run`, but a batch so large the host aborts (it can exhaust the
/// budget while recording auth, before `try_` gets an error back) also
/// counts as "does not fit".
fn fits(n: u32, mint: bool) -> Option<u64> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(std::boxed::Box::new(|_| {}));
    let out =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(n, mint))).unwrap_or(None);
    std::panic::set_hook(hook);
    out
}

fn bench(mint: bool) -> u32 {
    let name = if mint { "mint_batch" } else { "transfer_batch" };
    let mut largest_ok = 0;
    for n in [1u32, 5, 10, 15, 20, 50] {
        match fits(n, mint) {
            Some(cpu) => {
                println!(
                    "[#726] {name} n={n}: cpu={cpu} ({} per recipient)",
                    cpu / n as u64
                );
                largest_ok = n;
            }
            None => println!("[#726] {name} n={n}: exceeds transaction resource limits"),
        }
    }
    // Pin the exact ceiling between the last size that fit and the next.
    let mut n = largest_ok + 1;
    while fits(n, mint).is_some() {
        largest_ok = n;
        n += 1;
    }
    println!("[#726] {name}: largest batch that fits = {largest_ok}");
    largest_ok
}

#[test]
fn test_batch_size_bench_mint_and_transfer() {
    let mint_max = bench(true);
    let transfer_max = bench(false);
    // docs/asset-token.md recommends at most 10; the ceilings must stay above it.
    assert!(mint_max >= 10, "mint_batch ceiling {mint_max}");
    assert!(transfer_max >= 10, "transfer_batch ceiling {transfer_max}");
}
