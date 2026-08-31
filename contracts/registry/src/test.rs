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

#[test]
fn test_version() {
    let (_env, client, _admin) = setup();
    assert_eq!(client.version(), VERSION);
}

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
#[should_panic(expected = "Error(Contract, #2)")]
fn test_register_before_init_panics_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(RegistryContract, ());
    let client = RegistryContractClient::new(&env, &id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_asset(
        &issuer,
        &token,
        &String::from_str(&env, "Asset"),
        &String::from_str(&env, "real_estate"),
        &100,
    );
}

#[test]
fn test_register_and_get_asset() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "real_estate", 10_000);
    assert_eq!(id, 1);
    let entry = client.get_asset(&id);
    assert_eq!(entry.issuer, issuer);
    assert_eq!(entry.valuation, 10_000);
    assert!(entry.active);
}

#[test]
fn test_ids_increment() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    let a = register(&env, &client, &issuer, "invoice", 1);
    let b = register(&env, &client, &issuer, "invoice", 1);
    let c = register(&env, &client, &issuer, "invoice", 1);
    assert_eq!(a, 1);
    assert_eq!(b, 2);
    assert_eq!(c, 3);
    assert_eq!(client.asset_count(), 3);
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
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    register(&env, &client, &alice, "real_estate", 5);
    register(&env, &client, &alice, "commodity", 5);
    register(&env, &client, &bob, "invoice", 5);
    assert_eq!(client.get_assets_by_issuer(&alice).len(), 2);
    assert_eq!(client.get_assets_by_issuer(&bob).len(), 1);
}

#[test]
fn test_get_assets_by_type() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "real_estate", 5);
    register(&env, &client, &issuer, "real_estate", 5);
    register(&env, &client, &issuer, "commodity", 5);
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
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "real_estate", 100);
    register(&env, &client, &issuer, "invoice", 250);
    assert_eq!(client.get_all_assets(&0, &2).len(), 2);
    assert_eq!(client.total_value_locked(), 350);
}

#[test]
fn test_deactivate_excludes_from_tvl() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "real_estate", 100);
    register(&env, &client, &issuer, "invoice", 250);
    assert_eq!(client.total_value_locked(), 350);
    client.deactivate_asset(&admin, &id);
    assert!(!client.get_asset(&id).active);
    assert_eq!(client.total_value_locked(), 250);
}

#[test]
fn test_tvl_sums_only_active() {
    let (env, client, admin) = setup();
    assert_eq!(client.total_value_locked(), 0);

    let issuer = Address::generate(&env);
    let a = register(&env, &client, &issuer, "real_estate", 100);
    let b = register(&env, &client, &issuer, "invoice", 250);
    let c = register(&env, &client, &issuer, "commodity", 40);
    assert_eq!(client.total_value_locked(), 390);

    client.deactivate_asset(&admin, &a);
    assert_eq!(client.total_value_locked(), 290);

    client.deactivate_asset(&admin, &c);
    assert_eq!(client.total_value_locked(), 250);

    client.deactivate_asset(&admin, &b);
    assert_eq!(client.total_value_locked(), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_deactivate_requires_admin() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, "real_estate", 100);
    let impostor = Address::generate(&env);
    client.deactivate_asset(&impostor, &id);
}

#[test]
fn test_active_count_excludes_deactivated() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let a = register(&env, &client, &issuer, "real_estate", 100);
    register(&env, &client, &issuer, "invoice", 250);
    assert_eq!(client.active_count(), 2);
    assert_eq!(client.asset_count(), 2);
    client.deactivate_asset(&admin, &a);
    assert_eq!(client.active_count(), 1);
    assert_eq!(client.asset_count(), 2);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_negative_valuation_rejected() {
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "real_estate", -1);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_empty_name_rejected() {
    // Issue #48: empty asset name must panic InvalidInput (#7).
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_asset(
        &issuer,
        &token,
        &String::from_str(&env, ""),
        &String::from_str(&env, "real_estate"),
        &100,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_invalid_asset_type_rejected() {
    // Issue #48: unknown asset_type must panic InvalidInput (#7).
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_asset(
        &issuer,
        &token,
        &String::from_str(&env, "My Asset"),
        &String::from_str(&env, "garbage"),
        &100,
    );
}

#[test]
fn test_asset_ids_increment_monotonically_and_are_never_reused() {
    // Issue #223.
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id1 = register(&env, &client, &issuer, "real_estate", 1_000);
    let id2 = register(&env, &client, &issuer, "invoice", 2_000);
    let id3 = register(&env, &client, &issuer, "commodity", 3_000);
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(id3, 3);

    client.deactivate_asset(&admin, &id2);

    let id4 = register(&env, &client, &issuer, "bond", 4_000);
    assert_eq!(id4, 4);
    assert_ne!(id4, id2);
}

#[test]
fn test_get_asset_on_unknown_id_fails_asset_not_found() {
    // Issue #224.
    let (env, client, _admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "real_estate", 1_000);

    assert_eq!(
        client.try_get_asset(&0),
        Err(Ok(Error::AssetNotFound.into()))
    );
    assert_eq!(
        client.try_get_asset(&2),
        Err(Ok(Error::AssetNotFound.into()))
    );
}

#[test]
fn test_get_assets_by_issuer_and_by_type_return_empty_vec_not_error() {
    // Issue #225.
    let (env, client, _admin) = setup();
    let unknown_issuer = Address::generate(&env);

    let by_issuer = client.get_assets_by_issuer(&unknown_issuer);
    assert_eq!(by_issuer.len(), 0);

    let by_type = client.get_assets_by_type(&String::from_str(&env, "fund"));
    assert_eq!(by_type.len(), 0);
}

#[test]
fn test_deactivate_asset_on_unknown_id_fails_and_active_count_unchanged() {
    // Issue #226.
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    register(&env, &client, &issuer, "real_estate", 1_000);
    assert_eq!(client.active_count(), 1);

    assert_eq!(
        client.try_deactivate_asset(&admin, &99),
        Err(Ok(Error::AssetNotFound.into()))
    );
    assert_eq!(client.active_count(), 1);
}
