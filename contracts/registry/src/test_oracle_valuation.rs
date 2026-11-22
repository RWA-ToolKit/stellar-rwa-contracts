#![cfg(test)]
use super::*;
use soroban_sdk::{testutils::Address as _, testutils::Events as _, Address, Env, String};

fn setup() -> (Env, RegistryContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(RegistryContract, ());
    let client = RegistryContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin, oracle)
}

fn register(client: &RegistryContractClient, env: &Env, issuer: &Address) -> u64 {
    client.register_asset(
        issuer,
        &Address::generate(env),
        &String::from_str(env, "Asset"),
        &String::from_str(env, "real_estate"),
        &10_000,
    )
}

#[test]
fn test_oracle_can_update_valuation_and_tvl() {
    let (env, client, admin, oracle) = setup();
    client.set_oracle(&admin, &Some(oracle.clone()));
    assert_eq!(client.get_oracle(), Some(oracle.clone()));
    let id = register(&client, &env, &Address::generate(&env));

    client.update_valuation_from_oracle(&oracle, &id, &20_000);

    assert_eq!(client.get_asset(&id).valuation, 20_000);
    assert_eq!(client.total_value_locked(), 20_000);
}

#[test]
fn test_oracle_cannot_call_admin_only_functions() {
    let (env, client, admin, oracle) = setup();
    client.set_oracle(&admin, &Some(oracle.clone()));
    let id = register(&client, &env, &Address::generate(&env));

    let result = client.try_set_oracle(&oracle, &None);
    assert_eq!(result, Err(Ok(Error::Unauthorized.into())));
    let result = client.try_deactivate_asset(&oracle, &id);
    assert_eq!(result, Err(Ok(Error::Unauthorized.into())));
    let result = client.try_update_valuation(&oracle, &id, &99_999);
    assert_eq!(result, Err(Ok(Error::Unauthorized.into())));
}

#[test]
fn test_non_oracle_cannot_update_valuation() {
    let (env, client, admin, oracle) = setup();
    client.set_oracle(&admin, &Some(oracle));
    let id = register(&client, &env, &Address::generate(&env));
    let stranger = Address::generate(&env);

    let result = client.try_update_valuation_from_oracle(&stranger, &id, &42);
    assert_eq!(result, Err(Ok(Error::Unauthorized.into())));
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_update_from_oracle_without_oracle_configured_panics() {
    let (env, client, _admin, oracle) = setup();
    let id = register(&client, &env, &Address::generate(&env));
    client.update_valuation_from_oracle(&oracle, &id, &42);
}

#[test]
fn test_admin_can_clear_oracle() {
    let (env, client, admin, oracle) = setup();
    client.set_oracle(&admin, &Some(oracle.clone()));
    client.set_oracle(&admin, &None);
    assert_eq!(client.get_oracle(), None);
    let id = register(&client, &env, &Address::generate(&env));
    let result = client.try_update_valuation_from_oracle(&oracle, &id, &42);
    assert_eq!(result, Err(Ok(Error::NoOracle.into())));
}
