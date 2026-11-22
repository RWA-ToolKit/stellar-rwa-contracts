#![cfg(test)]
use super::*;
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, String};

struct Setup {
    env: Env,
    token: AssetTokenContractClient<'static>,
    compliance: ComplianceContractClient<'static>,
    compliance_id: Address,
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
        &String::from_str(&env, "Frozen Asset"),
        &String::from_str(&env, "FRZ"),
        &String::from_str(&env, "real_estate"),
        &supply,
        &2,
        &compliance_id,
        &String::from_str(&env, "partial-freeze test"),
        &1_000,
    );
    Setup {
        env,
        token,
        compliance,
        compliance_id,
        admin,
    }
}

fn approve(s: &Setup, who: &Address) {
    s.compliance
        .add_to_allowlist(&s.admin, who, &String::from_str(&s.env, "GB"), &0);
}

#[test]
fn test_partial_freeze_allows_available_transfer_but_rejects_frozen_portion() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s, &bob);

    s.token.freeze_balance(&s.admin, &s.admin, &400);

    assert_eq!(s.token.balance(&s.admin), 1_000);
    assert_eq!(s.token.frozen_balance(&s.admin), 400);
    assert_eq!(s.token.transferable_balance(&s.admin), 600);
    // KYC/compliance state is unchanged by a freeze.
    assert!(s.compliance.is_allowed(&s.admin));

    s.token.transfer(&s.admin, &bob, &600);
    assert_eq!(s.token.balance(&s.admin), 400);
    assert_eq!(s.token.balance(&bob), 600);
    assert_eq!(s.token.frozen_balance(&s.admin), 400);

    let result = s.token.try_transfer(&s.admin, &bob, &1);
    assert_eq!(result, Err(Ok(Error::InsufficientBalance.into())));
}

#[test]
fn test_unfreeze_restores_transferable_balance() {
    let s = setup(100);
    let bob = Address::generate(&s.env);
    approve(&s, &bob);

    s.token.freeze_balance(&s.admin, &s.admin, &70);
    s.token.unfreeze_balance(&s.admin, &s.admin, &20);
    assert_eq!(s.token.frozen_balance(&s.admin), 50);
    assert_eq!(s.token.transferable_balance(&s.admin), 50);

    s.token.transfer(&s.admin, &bob, &50);
    assert_eq!(s.token.balance(&s.admin), 50);
    assert_eq!(s.token.balance(&bob), 50);
    assert_eq!(s.token.frozen_balance(&s.admin), 50);
}

#[test]
fn test_freeze_cannot_exceed_balance_and_unfreeze_cannot_exceed_frozen() {
    let s = setup(100);
    assert_eq!(
        s.token.try_freeze_balance(&s.admin, &s.admin, &101),
        Err(Ok(Error::InsufficientBalance.into()))
    );
    s.token.freeze_balance(&s.admin, &s.admin, &50);
    assert_eq!(
        s.token.try_unfreeze_balance(&s.admin, &s.admin, &51),
        Err(Ok(Error::InsufficientBalance.into()))
    );
}

#[test]
fn test_only_admin_can_freeze() {
    let s = setup(100);
    let stranger = Address::generate(&s.env);
    assert_eq!(
        s.token.try_freeze_balance(&stranger, &s.admin, &10),
        Err(Ok(Error::Unauthorized.into()))
    );
    assert_eq!(s.token.frozen_balance(&s.admin), 0);
}
