#![cfg(test)]
//! Emergency-stop coverage for the registry contract (issue #731),
//! mirroring the asset-token guardian/pause tests.

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
    valuation: i128,
) -> u64 {
    client.register_asset(
        issuer,
        &Address::generate(env),
        &String::from_str(env, "Asset"),
        &String::from_str(env, "real_estate"),
        &valuation,
    )
}

#[test]
fn test_pause_unpause_and_guardian_lifecycle() {
    let (env, client, admin) = setup();
    let guardian = Address::generate(&env);
    let issuer = Address::generate(&env);

    assert!(!client.is_paused());
    assert_eq!(client.get_guardian(), None);

    client.set_guardian(&admin, &Some(guardian.clone()));
    assert_eq!(client.get_guardian(), Some(guardian.clone()));

    client.pause(&guardian);
    assert!(client.is_paused());
    client.set_guardian(&admin, &None);
    assert_eq!(client.get_guardian(), None);
    client.unpause(&admin);
    assert!(!client.is_paused());

    let id = register(&env, &client, &issuer, 10_000);
    client.pause(&admin);
    assert!(client.is_paused());
    // Read-only views keep working while the registry is paused.
    assert_eq!(client.get_asset(&id).valuation, 10_000);
    assert_eq!(client.total_value_locked(), 10_000);
    client.unpause(&admin);
    assert!(!client.is_paused());
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_register_asset_blocked_while_paused() {
    let (env, client, admin) = setup();
    client.pause(&admin);
    register(&env, &client, &Address::generate(&env), 10_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_deactivate_asset_blocked_while_paused() {
    let (env, client, admin) = setup();
    let issuer = Address::generate(&env);
    let id = register(&env, &client, &issuer, 10_000);
    client.pause(&admin);
    client.deactivate_asset(&admin, &id);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_stranger_cannot_pause() {
    let (env, client, _admin) = setup();
    client.pause(&Address::generate(&env));
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_guardian_cannot_unpause() {
    let (env, client, admin) = setup();
    let guardian = Address::generate(&env);
    client.set_guardian(&admin, &Some(guardian.clone()));
    client.pause(&guardian);
    client.unpause(&guardian);
}
