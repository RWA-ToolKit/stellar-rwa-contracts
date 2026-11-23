//! Tests for `mint_batch` atomicity semantics.
//!
//! These tests prove that `mint_batch` is atomic: if any recipient in the
//! batch fails a compliance check, the entire call aborts and no partial
//! state (total_supply or balances) is persisted.

use crate::{AssetToken, AssetTokenClient};
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

fn setup(env: &Env) -> (AssetTokenClient<'_>, ComplianceContractClient<'_>, Address) {
    let admin = Address::generate(env);
    let compliance_id = env.register_contract(None, ComplianceContract);
    let compliance_client = ComplianceContractClient::new(env, &compliance_id);

    let token_id = env.register_contract(None, AssetToken);
    let token_client = AssetTokenClient::new(env, &token_id);

    token_client.initialize(
        &admin,
        &7u32,
        &soroban_sdk::String::from_str(env, "Test Token"),
        &soroban_sdk::String::from_str(env, "TST"),
        &compliance_id,
    );

    (token_client, compliance_client, admin)
}

#[test]
fn test_mint_batch_atomic_rollback_on_mid_batch_failure() {
    let env = Env::default();
    env.mock_all_auths();

    let (token_client, compliance_client, admin) = setup(&env);

    let recipient_1 = Address::generate(&env);
    let recipient_2 = Address::generate(&env);
    let recipient_3 = Address::generate(&env);

    // Allowlist recipients 1 and 3, but NOT recipient 2.
    compliance_client.allow(&admin, &recipient_1);
    compliance_client.allow(&admin, &recipient_3);

    // Record pre-call state.
    let supply_before = token_client.total_supply();
    let bal_1_before = token_client.balance(&recipient_1);
    let bal_2_before = token_client.balance(&recipient_2);
    let bal_3_before = token_client.balance(&recipient_3);

    // Build the batch: recipient 2 is non-compliant.
    let mut recipients: Vec<Address> = Vec::new(&env);
    recipients.push_back(recipient_1.clone());
    recipients.push_back(recipient_2.clone());
    recipients.push_back(recipient_3.clone());

    let mut amounts: Vec<i128> = Vec::new(&env);
    amounts.push_back(100i128);
    amounts.push_back(200i128);
    amounts.push_back(300i128);

    // The call must fail because recipient 2 is not allowlisted.
    let result = token_client.try_mint_batch(&admin, &recipients, &amounts);
    assert!(result.is_err(), "mint_batch should fail on non-compliant recipient");

    // Assert no partial state was persisted.
    assert_eq!(
        token_client.total_supply(),
        supply_before,
        "total_supply must be unchanged after failed batch"
    );
    assert_eq!(
        token_client.balance(&recipient_1),
        bal_1_before,
        "recipient_1 balance must be unchanged after failed batch"
    );
    assert_eq!(
        token_client.balance(&recipient_2),
        bal_2_before,
        "recipient_2 balance must be unchanged after failed batch"
    );
    assert_eq!(
        token_client.balance(&recipient_3),
        bal_3_before,
        "recipient_3 balance must be unchanged after failed batch"
    );
}

#[test]
fn test_mint_batch_happy_path_mints_all_entries() {
    let env = Env::default();
    env.mock_all_auths();

    let (token_client, compliance_client, admin) = setup(&env);

    let recipient_1 = Address::generate(&env);
    let recipient_2 = Address::generate(&env);
    let recipient_3 = Address::generate(&env);

    // All recipients are compliant.
    compliance_client.allow(&admin, &recipient_1);
    compliance_client.allow(&admin, &recipient_2);
    compliance_client.allow(&admin, &recipient_3);

    let supply_before = token_client.total_supply();

    let mut recipients: Vec<Address> = Vec::new(&env);
    recipients.push_back(recipient_1.clone());
    recipients.push_back(recipient_2.clone());
    recipients.push_back(recipient_3.clone());

    let mut amounts: Vec<i128> = Vec::new(&env);
    amounts.push_back(100i128);
    amounts.push_back(200i128);
    amounts.push_back(300i128);

    token_client.mint_batch(&admin, &recipients, &amounts);

    // Each recipient received exactly its amount.
    assert_eq!(token_client.balance(&recipient_1), 100);
    assert_eq!(token_client.balance(&recipient_2), 200);
    assert_eq!(token_client.balance(&recipient_3), 300);

    // Total supply increased by the sum of all amounts.
    assert_eq!(token_client.total_supply(), supply_before + 600);
}