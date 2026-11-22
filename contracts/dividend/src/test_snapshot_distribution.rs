//! #728: distributions sized on an asset-token snapshot.
#![cfg(test)]
use super::*;
use asset_token::{AssetTokenContract, AssetTokenContractClient};
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{testutils::Address as _, token, Address, Env, String, Vec};

#[test]
fn test_distribution_from_snapshot_pays_holders_as_of_the_snapshot() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let seller = Address::generate(&env);
    let buyer = Address::generate(&env);
    let comp_id = env.register(ComplianceContract, ());
    let comp = ComplianceContractClient::new(&env, &comp_id);
    comp.initialize(&admin);
    for a in [&admin, &seller, &buyer] {
        comp.add_to_allowlist(&admin, a, &String::from_str(&env, "US"), &0);
    }
    let asset_id = env.register(AssetTokenContract, ());
    let asset = AssetTokenContractClient::new(&env, &asset_id);
    asset.initialize(
        &admin,
        &String::from_str(&env, "Loft"),
        &String::from_str(&env, "LFT"),
        &String::from_str(&env, "real_estate"),
        &1_000i128,
        &0u32,
        &comp_id,
        &String::from_str(&env, "desc"),
        &1_000i128,
    );
    asset.transfer(&admin, &seller, &400);
    let pay_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    token::StellarAssetClient::new(&env, &pay_id).mint(&admin, &10_000);
    let div_id = env.register(DividendContract, ());
    let dividend = DividendContractClient::new(&env, &div_id);
    dividend.initialize(&admin);

    // Record date, then the seller sells everything before the distribution.
    let snap = asset.snapshot(&admin);
    asset.transfer(&seller, &buyer, &400);

    let mut holders = Vec::new(&env);
    holders.push_back(seller.clone());
    holders.push_back(buyer.clone());
    holders.push_back(admin.clone());
    let id = dividend
        .create_distribution_snapshot(&admin, &asset_id, &pay_id, &1_000, &snap, &holders, &0);

    // Seller held 400 of 1000 at the snapshot; buyer held nothing.
    assert_eq!(dividend.claimable(&id, &seller), 400);
    assert_eq!(dividend.claimable(&id, &buyer), 0);
    assert_eq!(dividend.claimable(&id, &admin), 600);
    dividend.claim(&id, &seller);
    assert_eq!(token::TokenClient::new(&env, &pay_id).balance(&seller), 400);
}
