#![cfg(test)]
use super::*;
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, String};

struct Setup {
    env: Env,
    token: AssetTokenContractClient<'static>,
    compliance: ComplianceContractClient<'static>,
    admin: Address,
}

fn setup(supply: i128) -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    let admin = Address::generate(&env);
    compliance.initialize(&admin);
    compliance.add_to_allowlist(&admin, &admin, &String::from_str(&env, "US"), &0);
    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);
    token.initialize(
        &admin,
        &String::from_str(&env, "Capped Asset"),
        &String::from_str(&env, "CAP"),
        &String::from_str(&env, "real_estate"),
        &supply,
        &2,
        &compliance_id,
        &String::from_str(&env, "cap test"),
        &1_000,
    );
    Setup {
        env,
        token,
        compliance,
        admin,
    }
}

fn approve(s: &Setup, who: &Address, jurisdiction: &str) {
    s.compliance
        .add_to_allowlist(&s.admin, who, &String::from_str(&s.env, jurisdiction), &0);
}

#[test]
fn test_cap_limits_cross_jurisdiction_transfers() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s, &bob, "GB");
    s.compliance
        .set_jurisdiction_cap(&s.admin, &String::from_str(&s.env, "GB"), &Some(2_000));

    s.token.transfer(&s.admin, &bob, &200);
    assert_eq!(s.token.balance(&bob), 200);
    assert_eq!(
        s.token.try_transfer(&s.admin, &bob, &1),
        Err(Ok(Error::JurisdictionCapExceeded.into()))
    );
    assert_eq!(s.token.balance(&bob), 200);
}

#[test]
fn test_intra_jurisdiction_transfer_is_exempt() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s, &bob, "US");
    s.compliance
        .set_jurisdiction_cap(&s.admin, &String::from_str(&s.env, "US"), &Some(1_000));

    s.token.transfer(&s.admin, &bob, &500);
    assert_eq!(s.token.balance(&bob), 500);
    assert_eq!(s.token.balance(&s.admin), 500);
}

#[test]
fn test_clearing_cap_allows_more_transfers() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s, &bob, "DE");
    s.compliance
        .set_jurisdiction_cap(&s.admin, &String::from_str(&s.env, "DE"), &Some(500));
    assert_eq!(
        s.token.try_transfer(&s.admin, &bob, &100),
        Err(Ok(Error::JurisdictionCapExceeded.into()))
    );
    s.compliance
        .set_jurisdiction_cap(&s.admin, &String::from_str(&s.env, "DE"), &None);
    s.token.transfer(&s.admin, &bob, &100);
    assert_eq!(s.token.balance(&bob), 100);
}
