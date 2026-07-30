#![cfg(test)]
use super::*;
use asset_token::{AssetTokenContract, AssetTokenContractClient};
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, String};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Deploy compliance + asset-token.  Returns (token_address, issuer_address).
/// The `issuer` is the admin of both the compliance and asset-token contracts.
fn deploy_token(env: &Env) -> (Address, Address) {
    let issuer = Address::generate(env);

    // Stand-up a minimal compliance contract and approve the issuer.
    let comp_id = env.register(ComplianceContract, ());
    let comp = ComplianceContractClient::new(env, &comp_id);
    comp.initialize(&issuer);
    let us = String::from_str(env, "US");
    comp.add_to_allowlist(&issuer, &issuer, &us, &0);

    // Deploy asset-token with issuer as admin.
    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(env, &token_id);
    token.initialize(
        &issuer,
        &String::from_str(env, "My Asset"),
        &String::from_str(env, "MA"),
        &String::from_str(env, "real_estate"),
        &1_000i128,
        &0u32,
        &comp_id,
        &String::from_str(env, "desc"),
        &10_000i128,
    );

    (token_id, issuer)
}

/// Set up registry with admin.
fn setup() -> (Env, RegistryContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(RegistryContract, ());
    let client = RegistryContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

/// Register an asset through a real asset-token deployment (required because
/// register_asset now verifies issuer == token.admin cross-contract).
fn register(
    env: &Env,
    client: &RegistryContractClient,
    kind: &str,
    valuation: i128,
) -> (u64, Address, Address) {
    let (token_addr, issuer) = deploy_token(env);
    let id = client.register_asset(
        &issuer,
        &token_addr,
        &String::from_str(env, "Asset"),
        &String::from_str(env, kind),
        &valuation,
    );
    (id, token_addr, issuer)
}

// ---------------------------------------------------------------------------
// Existing tests (updated to use real token deployments)
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_admin() {
    let (_env, client, admin) = setup();
    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.asset_count(), 0);
    assert_eq!(client.total_value_locked(), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_double_init() {
    let (env, client, _admin) = setup();
    client.initialize(&Address::generate(&env));
}

#[test]
fn test_register_and_get_asset() {
    let (env, client, _admin) = setup();
    let (id, _token, issuer) = register(&env, &client, "real_estate", 10_000);
    assert_eq!(id, 1);
    let entry = client.get_asset(&id);
    assert_eq!(entry.issuer, issuer);
    assert_eq!(entry.valuation, 10_000);
    assert!(entry.active);
}

#[test]
fn test_ids_increment() {
    let (env, client, _admin) = setup();
    let (a, _, _) = register(&env, &client, "invoice", 1);
    let (b, _, _) = register(&env, &client, "invoice", 1);
    assert_eq!(a, 1);
    assert_eq!(b, 2);
    assert_eq!(client.asset_count(), 2);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_get_missing_asset() {
    let (_env, client, _admin) = setup();
    client.get_asset(&999);
}

#[test]
fn test_get_assets_by_issuer() {
    let (env, client, _admin) = setup();
    // Two assets under alice, one under bob — each with their own token contract.
    let (_, _, alice) = register(&env, &client, "real_estate", 5);
    // second asset for alice: we must deploy a fresh token whose admin is alice
    {
        let comp_id = env.register(ComplianceContract, ());
        let comp = ComplianceContractClient::new(&env, &comp_id);
        comp.initialize(&alice);
        comp.add_to_allowlist(&alice, &alice, &String::from_str(&env, "US"), &0);
        let token_id = env.register(AssetTokenContract, ());
        let token = AssetTokenContractClient::new(&env, &token_id);
        token.initialize(
            &alice,
            &String::from_str(&env, "Asset2"),
            &String::from_str(&env, "A2"),
            &String::from_str(&env, "commodity"),
            &1_000i128,
            &0u32,
            &comp_id,
            &String::from_str(&env, "d"),
            &5i128,
        );
        client.register_asset(
            &alice,
            &token_id,
            &String::from_str(&env, "Asset2"),
            &String::from_str(&env, "commodity"),
            &5,
        );
    }
    let (_, _, _bob) = register(&env, &client, "invoice", 5);
    assert_eq!(client.get_assets_by_issuer(&alice).len(), 2);
}

#[test]
fn test_get_assets_by_type() {
    let (env, client, _admin) = setup();
    register(&env, &client, "real_estate", 5);
    register(&env, &client, "real_estate", 5);
    register(&env, &client, "commodity", 5);
    assert_eq!(
        client
            .get_assets_by_type(&String::from_str(&env, "real_estate"))
            .len(),
        2
    );
    assert_eq!(
        client
            .get_assets_by_type(&String::from_str(&env, "commodity"))
            .len(),
        1
    );
}

#[test]
fn test_get_all_and_tvl() {
    let (env, client, _admin) = setup();
    register(&env, &client, "real_estate", 100);
    register(&env, &client, "invoice", 250);
    assert_eq!(client.get_all_assets().len(), 2);
    assert_eq!(client.total_value_locked(), 350);
}

#[test]
fn test_deactivate_excludes_from_tvl() {
    let (env, client, admin) = setup();
    let (id, _, _) = register(&env, &client, "real_estate", 100);
    register(&env, &client, "invoice", 250);
    assert_eq!(client.total_value_locked(), 350);
    client.deactivate_asset(&admin, &id);
    assert!(!client.get_asset(&id).active);
    assert_eq!(client.total_value_locked(), 250);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_deactivate_requires_admin() {
    let (env, client, _admin) = setup();
    let (id, _, _) = register(&env, &client, "real_estate", 100);
    let impostor = Address::generate(&env);
    client.deactivate_asset(&impostor, &id);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_negative_valuation_rejected() {
    let (env, client, _admin) = setup();
    let (token_addr, issuer) = deploy_token(&env);
    client.register_asset(
        &issuer,
        &token_addr,
        &String::from_str(&env, "Asset"),
        &String::from_str(&env, "real_estate"),
        &-1,
    );
}

// ---------------------------------------------------------------------------
// New security tests
// ---------------------------------------------------------------------------

/// #10 – a caller who is NOT the admin of the token_contract must be rejected.
#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_register_unauthorized_issuer_rejected() {
    let (env, client, _admin) = setup();
    let (token_addr, _real_issuer) = deploy_token(&env);
    // attacker tries to register a token they don't control
    let attacker = Address::generate(&env);
    client.register_asset(
        &attacker,
        &token_addr,
        &String::from_str(&env, "Fake"),
        &String::from_str(&env, "real_estate"),
        &999_999_999,
    );
}

/// #11 – registering the same token_contract twice must be rejected.
#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_duplicate_token_rejected() {
    let (env, client, _admin) = setup();
    let (token_addr, issuer) = deploy_token(&env);
    // First registration succeeds.
    client.register_asset(
        &issuer,
        &token_addr,
        &String::from_str(&env, "Asset"),
        &String::from_str(&env, "real_estate"),
        &10_000,
    );
    // Second registration of the exact same token_contract must fail.
    client.register_asset(
        &issuer,
        &token_addr,
        &String::from_str(&env, "Asset Dup"),
        &String::from_str(&env, "real_estate"),
        &10_000,
    );
}

/// #12 – issuer can update the registry valuation; TVL reflects the new value.
#[test]
fn test_update_asset_valuation_by_issuer() {
    let (env, client, _admin) = setup();
    let (id, _, issuer) = register(&env, &client, "real_estate", 100_000);
    assert_eq!(client.total_value_locked(), 100_000);

    client.update_asset_valuation(&issuer, &id, &200_000);

    assert_eq!(client.get_asset(&id).valuation, 200_000);
    assert_eq!(client.total_value_locked(), 200_000);
}

/// #12 – admin can also update the registry valuation.
#[test]
fn test_update_asset_valuation_by_admin() {
    let (env, client, admin) = setup();
    let (id, _, _issuer) = register(&env, &client, "real_estate", 100_000);

    client.update_asset_valuation(&admin, &id, &50_000);

    assert_eq!(client.get_asset(&id).valuation, 50_000);
    assert_eq!(client.total_value_locked(), 50_000);
}

/// #12 – a random address must not be able to update the valuation.
#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_update_asset_valuation_unauthorized_rejected() {
    let (env, client, _admin) = setup();
    let (id, _, _issuer) = register(&env, &client, "real_estate", 100_000);
    let stranger = Address::generate(&env);
    client.update_asset_valuation(&stranger, &id, &1);
}

/// #12 – negative valuation must be rejected on update too.
#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_update_asset_valuation_negative_rejected() {
    let (env, client, _admin) = setup();
    let (id, _, issuer) = register(&env, &client, "real_estate", 100_000);
    client.update_asset_valuation(&issuer, &id, &-1);
}
