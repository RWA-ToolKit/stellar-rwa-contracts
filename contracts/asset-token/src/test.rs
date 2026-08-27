#![cfg(test)]
use super::*;
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction},
    Address, Env, String, Symbol,
};

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
    approve(&env, &compliance, &admin, &admin);

    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);
    token.initialize(
        &admin,
        &String::from_str(&env, "Manhattan Loft"),
        &String::from_str(&env, "MLOFT"),
        &String::from_str(&env, "real_estate"),
        &supply,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );

    Setup {
        env,
        token,
        compliance,
        compliance_id,
        admin,
    }
}

fn approve(env: &Env, compliance: &ComplianceContractClient, admin: &Address, who: &Address) {
    compliance.add_to_allowlist(admin, who, &String::from_str(env, "US"), &0);
}

#[test]
fn test_version() {
    let s = setup(1_000);
    assert_eq!(s.token.version(), VERSION);
}

#[test]
fn test_initialize_mints_full_supply_to_admin() {
    let s = setup(1_000);
    assert_eq!(s.token.balance(&s.admin), 1_000);
    assert_eq!(s.token.total_supply(), 1_000);
    let meta = s.token.get_metadata();
    assert_eq!(meta.symbol, String::from_str(&s.env, "MLOFT"));
    assert_eq!(meta.valuation, 50_000_000);
    assert!(!meta.paused);
}

#[test]
fn test_transfer_between_approved() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.transfer(&s.admin, &bob, &400);
    assert_eq!(s.token.balance(&s.admin), 600);
    assert_eq!(s.token.balance(&bob), 400);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_transfer_blocked_when_sender_not_compliant() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.transfer(&s.admin, &bob, &500);
    // Bob now holds tokens; revoke his approval.
    s.compliance.remove(&s.admin, &bob);
    let carol = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &carol);
    s.token.transfer(&bob, &carol, &100);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_transfer_blocked_when_recipient_not_compliant() {
    let s = setup(1_000);
    let carol = Address::generate(&s.env); // never approved
    s.token.transfer(&s.admin, &carol, &100);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_transfer_blocked_when_paused() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.pause(&s.admin);
    s.token.transfer(&s.admin, &bob, &100);
}

#[test]
fn test_mint_increases_supply() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.mint(&s.admin, &bob, &250);
    assert_eq!(s.token.balance(&bob), 250);
    assert_eq!(s.token.total_supply(), 1_250);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_mint_to_noncompliant_fails() {
    let s = setup(1_000);
    let carol = Address::generate(&s.env);
    s.token.mint(&s.admin, &carol, &100);
}

#[test]
fn test_burn_reduces_supply() {
    let s = setup(1_000);
    s.token.burn(&s.admin, &300);
    assert_eq!(s.token.balance(&s.admin), 700);
    assert_eq!(s.token.total_supply(), 700);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_insufficient_balance() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    // Bob has zero balance.
    s.token.transfer(&bob, &s.admin, &1);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_zero_amount_rejected() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.transfer(&s.admin, &bob, &0);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_negative_amount_rejected() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.transfer(&s.admin, &bob, &-1);
}

#[test]
fn test_self_transfer_no_inflation() {
    let s = setup(1_000);
    let supply_before = s.token.total_supply();
    let bal_before = s.token.balance(&s.admin);
    s.token.transfer(&s.admin, &s.admin, &100);
    assert_eq!(s.token.balance(&s.admin), bal_before);
    assert_eq!(s.token.total_supply(), supply_before);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_unauthorized_mint_rejected() {
    let s = setup(1_000);
    let impostor = Address::generate(&s.env);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.mint(&impostor, &bob, &100);
}

#[test]
fn test_pause_then_unpause_restores_transfer() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.pause(&s.admin);
    s.token.unpause(&s.admin);
    s.token.transfer(&s.admin, &bob, &100);
    assert_eq!(s.token.balance(&bob), 100);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_mint_blocked_when_paused() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.pause(&s.admin);
    s.token.mint(&s.admin, &bob, &100);
}

#[test]
fn test_mint_succeeds_after_unpause() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.pause(&s.admin);
    s.token.unpause(&s.admin);
    s.token.mint(&s.admin, &bob, &100);
    assert_eq!(s.token.balance(&bob), 100);
    assert_eq!(s.token.total_supply(), 1_100);
}

#[test]
fn test_update_valuation() {
    let s = setup(1_000);
    s.token.update_valuation(&s.admin, &75_000_000);
    assert_eq!(s.token.get_metadata().valuation, 75_000_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_update_valuation_by_non_admin_reverts() {
    let s = setup(1_000);
    let impostor = Address::generate(&s.env);
    s.token.update_valuation(&impostor, &75_000_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_update_valuation_negative_rejected() {
    let s = setup(1_000);
    s.token.update_valuation(&s.admin, &-1);
}

#[test]
fn test_set_compliance_switches_gate() {
    let s = setup(1_000);
    // A fresh compliance contract where the admin is approved.
    let comp2_id = env_register_empty_compliance(&s.env, &s.admin);
    let comp2 = ComplianceContractClient::new(&s.env, &comp2_id);
    approve(&s.env, &comp2, &s.admin, &s.admin);
    s.token.set_compliance(&s.admin, &comp2_id);
    assert_eq!(s.token.get_metadata().compliance_contract, comp2_id);
    // Sanity: original compliance still knows the admin.
    assert!(s.compliance.is_allowed(&s.admin));
    let _ = &s.compliance_id;
}

#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn test_set_compliance_rejects_contract_that_blocks_admin() {
    let s = setup(1_000);
    // A fresh compliance contract where nobody, including the admin, is approved.
    let comp2_id = env_register_empty_compliance(&s.env, &s.admin);
    s.token.set_compliance(&s.admin, &comp2_id);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_set_compliance_gate_change_blocks_previously_approved_holder() {
    let s = setup(1_000);
    // Bob is approved and holds a balance under the original compliance contract.
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.transfer(&s.admin, &bob, &200);

    // Switch to a fresh compliance contract that only approves the admin
    // (required for set_compliance to succeed) and rejects everyone else,
    // including bob.
    let comp2_id = env_register_empty_compliance(&s.env, &s.admin);
    let comp2 = ComplianceContractClient::new(&s.env, &comp2_id);
    approve(&s.env, &comp2, &s.admin, &s.admin);
    s.token.set_compliance(&s.admin, &comp2_id);

    // Bob was compliant under the old gate but is not recognized by the new
    // one, so the enforced gate must now reject his transfer.
    s.token.transfer(&bob, &s.admin, &50);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_burn_more_than_balance_fails() {
    let s = setup(1_000);
    s.token.burn(&s.admin, &2000);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_burn_blocked_when_holder_not_compliant() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.transfer(&s.admin, &bob, &200);
    // Bob now holds tokens; revoke his approval.
    s.compliance.remove(&s.admin, &bob);
    s.token.burn(&bob, &100);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_initialize_reverts_when_admin_not_compliant() {
    let env = Env::default();
    env.mock_all_auths();

    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    let admin = Address::generate(&env);
    compliance.initialize(&admin);

    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);
    token.initialize(
        &admin,
        &String::from_str(&env, "Manhattan Loft"),
        &String::from_str(&env, "MLOFT"),
        &String::from_str(&env, "real_estate"),
        &1_000i128,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_initialize_with_negative_total_supply_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    let admin = Address::generate(&env);
    compliance.initialize(&admin);

    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);
    token.initialize(
        &admin,
        &String::from_str(&env, "Manhattan Loft"),
        &String::from_str(&env, "MLOFT"),
        &String::from_str(&env, "real_estate"),
        &-100i128,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_initialize_with_negative_valuation_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    let admin = Address::generate(&env);
    compliance.initialize(&admin);

    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);
    token.initialize(
        &admin,
        &String::from_str(&env, "Manhattan Loft"),
        &String::from_str(&env, "MLOFT"),
        &String::from_str(&env, "real_estate"),
        &1_000i128,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &-50_000_000i128,
    );
}

fn env_register_empty_compliance(env: &Env, admin: &Address) -> Address {
    let id = env.register(ComplianceContract, ());
    let c = ComplianceContractClient::new(env, &id);
    c.initialize(admin);
    id
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_transfer_to_unapproved_recipient_panics_recipient_not_compliant() {
    let s = setup(1_000);
    // `eve` is never added to the allowlist — transfer must panic RecipientNotCompliant.
    let eve = Address::generate(&s.env);
    s.token.transfer(&s.admin, &eve, &100);
}

// ---- issue #120: cross-contract auth propagation ----

#[test]
fn test_transfer_requires_only_sender_auth() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);

    s.token.transfer(&s.admin, &bob, &400);

    let auths = s.env.auths();
    assert_eq!(auths.len(), 1);
    let (authorizer, invocation) = &auths[0];
    assert_eq!(*authorizer, s.admin);
    match &invocation.function {
        AuthorizedFunction::Contract((contract, fn_name, _)) => {
            assert_eq!(*contract, s.token.address);
            assert_eq!(*fn_name, Symbol::new(&s.env, "transfer"));
        }
        _ => panic!("expected a contract invocation"),
    }
    // Compliance gating only reads `is_allowed`, which requires no auth, so
    // there must be no sub-invocation beyond the sender's own transfer.
    assert_eq!(invocation.sub_invocations.len(), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_transfer_after_sender_suspended_post_mint_panics_sender_not_compliant() {
    let s = setup(1_000);

    // Mint to bob (he must be approved first).
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.mint(&s.admin, &bob, &500);
    assert_eq!(s.token.balance(&bob), 500);

    // Suspend bob via the compliance contract's dedicated suspend method.
    s.compliance.suspend(&s.admin, &bob);

    // carol is a valid recipient; the transfer should fail on the sender check.
    let carol = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &carol);
    s.token.transfer(&bob, &carol, &100);
}

// ---- issue #198: every entry point rejects calls before `initialize` ----

#[test]
fn test_entry_points_fail_before_initialize() {
    let env = Env::default();
    env.mock_all_auths();

    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    assert_eq!(
        token.try_transfer(&admin, &user, &100),
        Err(Ok(Error::NotInitialized))
    );
    assert_eq!(
        token.try_mint(&admin, &user, &100),
        Err(Ok(Error::NotInitialized))
    );
    assert_eq!(
        token.try_burn(&admin, &100),
        Err(Ok(Error::NotInitialized))
    );
    assert_eq!(token.try_total_supply(), Err(Ok(Error::NotInitialized)));
    assert_eq!(token.try_get_metadata(), Err(Ok(Error::NotInitialized)));
    assert_eq!(token.try_pause(&admin), Err(Ok(Error::NotInitialized)));
    assert_eq!(
        token.try_update_valuation(&admin, &100),
        Err(Ok(Error::NotInitialized))
    );
}

// ---- issue #197: every admin-only entry point rejects a non-admin caller ----

#[test]
fn test_admin_only_entry_points_reject_non_admin() {
    let s = setup(1_000);
    let impostor = Address::generate(&s.env);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);

    assert_eq!(
        s.token.try_mint(&impostor, &bob, &100),
        Err(Ok(Error::Unauthorized))
    );

    let recipients = Vec::from_array(&s.env, [(bob.clone(), 100i128)]);
    assert_eq!(
        s.token.try_mint_batch(&impostor, &recipients),
        Err(Ok(Error::Unauthorized))
    );

    assert_eq!(
        s.token.try_pause(&impostor),
        Err(Ok(Error::Unauthorized))
    );
    assert_eq!(
        s.token.try_unpause(&impostor),
        Err(Ok(Error::Unauthorized))
    );
    assert_eq!(
        s.token.try_update_valuation(&impostor, &75_000_000),
        Err(Ok(Error::Unauthorized))
    );
    assert_eq!(
        s.token.try_set_compliance(&impostor, &s.compliance_id),
        Err(Ok(Error::Unauthorized))
    );
}

// ---- issue #196: `update_valuation` rejects a negative valuation and keeps the old value ----

#[test]
fn test_update_valuation_negative_rejected_keeps_old_value() {
    let s = setup(1_000);
    let original = s.token.get_metadata().valuation;
    assert_eq!(
        s.token.try_update_valuation(&s.admin, &-1),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(s.token.get_metadata().valuation, original);
}

// ---- issue #195: `burn` reduces `total_supply` and rejects burning more than the balance ----

#[test]
fn test_burn_decrements_balance_and_supply_by_same_amount() {
    let s = setup(1_000);
    let balance_before = s.token.balance(&s.admin);
    let supply_before = s.token.total_supply();
    s.token.burn(&s.admin, &300);
    assert_eq!(balance_before - s.token.balance(&s.admin), 300);
    assert_eq!(supply_before - s.token.total_supply(), 300);
}

#[test]
fn test_burn_more_than_balance_fails_without_partial_mutation() {
    let s = setup(1_000);
    let balance_before = s.token.balance(&s.admin);
    let supply_before = s.token.total_supply();
    assert_eq!(
        s.token.try_burn(&s.admin, &(balance_before + 1)),
        Err(Ok(Error::InsufficientBalance))
    );
    assert_eq!(s.token.balance(&s.admin), balance_before);
    assert_eq!(s.token.total_supply(), supply_before);
}
