//! #728: point-in-time balances.
#![cfg(test)]
use super::*;
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, String};

fn setup() -> (
    Env,
    AssetTokenContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    let admin = Address::generate(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    compliance.initialize(&admin);
    for a in [&admin, &alice, &bob] {
        compliance.add_to_allowlist(&admin, a, &String::from_str(&env, "US"), &0);
    }
    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);
    token.initialize(
        &admin,
        &String::from_str(&env, "Loft"),
        &String::from_str(&env, "LFT"),
        &String::from_str(&env, "real_estate"),
        &1_000i128,
        &0u32,
        &compliance_id,
        &String::from_str(&env, "desc"),
        &1_000i128,
    );
    token.transfer(&admin, &alice, &300);
    (env, token, admin, alice, bob)
}

#[test]
fn test_balance_at_is_fixed_at_the_snapshot() {
    let (_env, token, admin, alice, bob) = setup();
    let s1 = token.snapshot(&admin);
    assert_eq!(s1, 1);

    // Alice sells after the snapshot; Bob buys.
    token.transfer(&alice, &bob, &100);
    token.transfer(&alice, &bob, &50); // a second change must not overwrite the snapshot value

    assert_eq!(token.balance_at(&s1, &alice), 300);
    assert_eq!(token.balance_at(&s1, &bob), 0);
    assert_eq!(
        token.balance_at(&s1, &admin),
        700,
        "unchanged since: the live balance"
    );
    assert_eq!(token.balance(&alice), 150);
    assert_eq!(token.total_supply_at(&s1), 1_000);
}

#[test]
fn test_snapshots_layer_correctly() {
    let (_env, token, admin, alice, bob) = setup();
    let s1 = token.snapshot(&admin);
    token.transfer(&alice, &bob, &100); // alice 200, bob 100
    let s2 = token.snapshot(&admin);
    let s3 = token.snapshot(&admin); // nothing moves between s2 and s3
    token.transfer(&bob, &alice, &40); // alice 240, bob 60
    token.mint(&admin, &bob, &500); // supply 1500

    assert_eq!(
        (token.balance_at(&s1, &alice), token.balance_at(&s1, &bob)),
        (300, 0)
    );
    assert_eq!(
        (token.balance_at(&s2, &alice), token.balance_at(&s2, &bob)),
        (200, 100)
    );
    assert_eq!(
        (token.balance_at(&s3, &alice), token.balance_at(&s3, &bob)),
        (200, 100)
    );
    assert_eq!((token.balance(&alice), token.balance(&bob)), (240, 560));
    assert_eq!(token.total_supply_at(&s1), 1_000);
    assert_eq!(token.total_supply_at(&s3), 1_000);
    assert_eq!(token.current_snapshot_id(), 3);
}

#[test]
fn test_unknown_snapshot_is_rejected() {
    let (_env, token, admin, alice, _bob) = setup();
    assert_eq!(
        token.try_balance_at(&1, &alice),
        Err(Ok(Error::SnapshotNotFound.into()))
    );
    token.snapshot(&admin);
    assert_eq!(
        token.try_balance_at(&0, &alice),
        Err(Ok(Error::SnapshotNotFound.into()))
    );
    assert_eq!(
        token.try_total_supply_at(&2),
        Err(Ok(Error::SnapshotNotFound.into()))
    );
}

#[test]
fn test_only_admin_can_snapshot() {
    let (_env, token, _admin, alice, _bob) = setup();
    assert_eq!(
        token.try_snapshot(&alice),
        Err(Ok(Error::Unauthorized.into()))
    );
}
