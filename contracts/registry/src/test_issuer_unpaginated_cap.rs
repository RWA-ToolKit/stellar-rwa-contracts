//! #722: `get_assets_by_issuer` (deprecated) is capped exactly like page 0 of
//! `get_assets_by_issuer_page`, in both rows and cost.
#![cfg(test)]
extern crate std;
use std::println;

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, String};

#[test]
fn test_unpaginated_issuer_query_is_page_zero() {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();
    let contract_id = env.register(RegistryContract, ());
    let client = RegistryContractClient::new(&env, &contract_id);
    client.initialize(&Address::generate(&env));
    let issuer = Address::generate(&env);
    for _ in 0..(MAX_PAGE_SIZE + 10) {
        client.register_asset(
            &issuer,
            &Address::generate(&env),
            &String::from_str(&env, "Asset"),
            &String::from_str(&env, "bond"),
            &1_000i128,
        );
    }

    env.cost_estimate().budget().reset_default();
    let all = client.get_assets_by_issuer(&issuer);
    let cpu_all = env.cost_estimate().budget().cpu_instruction_cost();
    env.cost_estimate().budget().reset_default();
    let page = client.get_assets_by_issuer_page(&issuer, &0, &MAX_PAGE_SIZE);
    let cpu_page = env.cost_estimate().budget().cpu_instruction_cost();
    println!(
        "[#722] get_assets_by_issuer: {} rows cpu={cpu_all}; page 0: {} rows cpu={cpu_page}",
        all.len(),
        page.len()
    );

    // Truncated to one page, not all MAX_PAGE_SIZE + 10, and the same rows.
    assert_eq!(all.len(), MAX_PAGE_SIZE);
    assert_eq!(all, page);
    assert!(cpu_all <= cpu_page + cpu_page / 20);
}
