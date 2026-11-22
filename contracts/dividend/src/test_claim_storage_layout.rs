//! #723: storage layout of claimed state, and what one `claim` writes.
//!
//! `cargo test -p dividend claim_storage_layout -- --nocapture`
#![cfg(test)]
extern crate std;
use std::println;

use super::*;
use asset_token::{AssetTokenContract, AssetTokenContractClient};
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{testutils::Address as _, token, xdr::ToXdr, Address, Env, String, Vec};

#[test]
fn test_claim_storage_layout_writes_are_small() {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();
    let admin = Address::generate(&env);
    let comp_id = env.register(ComplianceContract, ());
    let comp = ComplianceContractClient::new(&env, &comp_id);
    comp.initialize(&admin);
    let us = String::from_str(&env, "US");
    let holder = Address::generate(&env);
    comp.add_to_allowlist(&admin, &admin, &us, &0);
    comp.add_to_allowlist(&admin, &holder, &us, &0);
    let asset_id = env.register(AssetTokenContract, ());
    let asset = AssetTokenContractClient::new(&env, &asset_id);
    asset.initialize(
        &admin,
        &String::from_str(&env, "Loft"),
        &String::from_str(&env, "LFT"),
        &String::from_str(&env, "real_estate"),
        &1000i128,
        &0u32,
        &comp_id,
        &String::from_str(&env, "desc"),
        &1000i128,
    );
    asset.transfer(&admin, &holder, &400);
    let pay_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    token::StellarAssetClient::new(&env, &pay_id).mint(&admin, &100_000);
    let div_id = env.register(DividendContract, ());
    let dividend = DividendContractClient::new(&env, &div_id);
    dividend.initialize(&admin);
    let mut eligible = Vec::new(&env);
    eligible.push_back((holder.clone(), 400i128));
    eligible.push_back((admin.clone(), 600i128));
    let id = dividend.create_distribution(&admin, &asset_id, &pay_id, &1000, &eligible);

    let dist_bytes = dividend.get_distribution(&id).to_xdr(&env).len();
    env.cost_estimate().budget().reset_unlimited();
    dividend.claim(&id, &holder);
    let res = env.cost_estimate().resources();
    println!(
        "[#723] Distribution entry: {dist_bytes} bytes; one claim writes {} entries, {} bytes in total",
        res.write_entries, res.write_bytes
    );
    // Claimed state is a per-(distribution, holder) key, not a list inside
    // Distribution, so the struct does not grow with the number of claims.
    assert!(dist_bytes < 400, "Distribution is {dist_bytes} bytes");
}
