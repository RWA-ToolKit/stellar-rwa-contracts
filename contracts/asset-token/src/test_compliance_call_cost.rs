//! #721: what the compliance cross-contract calls cost on `transfer`.
//!
//! `cargo test -p asset-token compliance_call_cost -- --nocapture --test-threads=1`
#![cfg(test)]
extern crate std;
use std::println;

use super::*;
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, String};

fn measure(same_jurisdiction: bool) -> (u64, u64) {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();
    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    let admin = Address::generate(&env);
    compliance.initialize(&admin);
    compliance.add_to_allowlist(&admin, &admin, &String::from_str(&env, "US"), &0);
    let bob = Address::generate(&env);
    let jurisdiction = if same_jurisdiction { "US" } else { "KE" };
    compliance.add_to_allowlist(&admin, &bob, &String::from_str(&env, jurisdiction), &0);
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
    env.cost_estimate().budget().reset_unlimited();
    token.transfer(&admin, &bob, &10);
    (
        env.cost_estimate().budget().cpu_instruction_cost(),
        env.cost_estimate().budget().memory_bytes_cost(),
    )
}

#[test]
fn test_compliance_call_cost_transfer() {
    let (cpu_same, mem_same) = measure(true);
    let (cpu_cross, mem_cross) = measure(false);
    println!("[#721] transfer, same jurisdiction: cpu={cpu_same} mem={mem_same}");
    println!("[#721] transfer, cross jurisdiction: cpu={cpu_cross} mem={mem_cross}");
    assert!(cpu_same < 2_000_000 && cpu_cross < 3_000_000);
}
