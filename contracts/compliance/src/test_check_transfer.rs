//! `check_transfer` (#721): one call answering what `asset-token::transfer`
//! used to ask in four.
#![cfg(test)]
use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, String};

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
fn test_check_transfer_matches_the_individual_queries() {
    let (env, client, admin) = setup();
    let us1 = Address::generate(&env);
    let us2 = Address::generate(&env);
    let ke = Address::generate(&env);
    let unknown = Address::generate(&env);
    client.add_to_allowlist(&admin, &us1, &String::from_str(&env, "US"), &0);
    client.add_to_allowlist(&admin, &us2, &String::from_str(&env, "US"), &0);
    client.add_to_allowlist(&admin, &ke, &String::from_str(&env, "KE"), &0);

    assert_eq!(client.check_transfer(&us1, &us2), (true, true, true));
    assert_eq!(client.check_transfer(&us1, &ke), (true, true, false));
    assert_eq!(client.check_transfer(&us1, &unknown), (true, false, false));
    assert_eq!(client.check_transfer(&unknown, &us1), (false, true, false));
    client.suspend(&admin, &us2);
    assert_eq!(client.check_transfer(&us1, &us2).1, client.is_allowed(&us2));
}
