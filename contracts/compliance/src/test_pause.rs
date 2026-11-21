#![cfg(test)]
//! Emergency-stop coverage for the compliance contract (issue #731),
//! mirroring the asset-token guardian/pause tests.

use super::*;
use soroban_sdk::{testutils::Address as _, testutils::Events, Address, Env, String};

fn setup() -> (Env, ComplianceContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ComplianceContract, ());
    let client = ComplianceContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

#[test]
fn test_pause_unpause_and_guardian_lifecycle() {
    let (env, client, admin) = setup();
    let guardian = Address::generate(&env);
    let stranger = Address::generate(&env);

    assert!(!client.is_paused());
    assert_eq!(client.get_guardian(), None);

    client.set_guardian(&admin, &Some(guardian.clone()));
    assert_eq!(client.get_guardian(), Some(guardian.clone()));

    client.pause(&guardian);
    assert!(client.is_paused());
    // Guardian changes remain available while paused (like asset-token).
    client.set_guardian(&admin, &None);
    assert_eq!(client.get_guardian(), None);

    client.unpause(&admin);
    assert!(!client.is_paused());

    client.pause(&admin);
    assert!(client.is_paused());
    client.unpause(&admin);
    assert!(!client.is_paused());

    // Read-only views and existing allowlist data are untouched by pause.
    let user = Address::generate(&env);
    client.add_to_allowlist(&admin, &user, &String::from_str(&env, "US"), &0);
    client.pause(&admin);
    assert!(client.get_record(&user).is_some());
    assert_eq!(client.get_allowlist_count(), 1);
    let _ = stranger;
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_add_to_allowlist_blocked_while_paused() {
    let (env, client, admin) = setup();
    client.pause(&admin);
    client.add_to_allowlist(
        &admin,
        &Address::generate(&env),
        &String::from_str(&env, "US"),
        &0,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_suspend_blocked_while_paused() {
    let (env, client, admin) = setup();
    let user = Address::generate(&env);
    client.add_to_allowlist(&admin, &user, &String::from_str(&env, "US"), &0);
    client.pause(&admin);
    client.suspend(&admin, &user);
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
