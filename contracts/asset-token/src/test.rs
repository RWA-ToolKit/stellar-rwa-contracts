#![cfg(test)]
use super::*;
use compliance::{ComplianceContract, ComplianceContractClient};
use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, Events},
    Address, Env, String, Symbol, Vec,
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

proptest! {
    #[test]
    fn prop_balances_sum_to_total_supply(
        ops in prop::collection::vec((any::<u8>(), any::<u8>(), any::<u8>(), any::<u8>()), 1..20),
    ) {
        let s = setup(1_000);
        let holders = [
            s.admin.clone(),
            Address::generate(&s.env),
            Address::generate(&s.env),
            Address::generate(&s.env),
            Address::generate(&s.env),
        ];
        for holder in &holders[1..] {
            approve(&s.env, &s.compliance, &s.admin, holder);
        }

        let mut expected = vec![0i128; holders.len()];
        expected[0] = 1_000;

        for op in ops {
            let action = op.0 % 4;
            let subject = (op.1 as usize) % holders.len();
            let other = (op.2 as usize) % holders.len();
            let amount = (op.3 as i128 % 50) + 1;

            match action {
                0 => {
                    let amt = if expected[subject] == 0 {
                        0
                    } else {
                        1 + (amount % expected[subject].max(1))
                    };
                    if amt > 0 {
                        if subject == other {
                            s.token.transfer(&holders[subject], &holders[other], &amt);
                        } else {
                            s.token.transfer(&holders[subject], &holders[other], &amt);
                            expected[subject] -= amt;
                            expected[other] += amt;
                        }
                    }
                }
                1 => {
                    let amt = amount;
                    s.token.mint(&s.admin, &holders[subject], &amt);
                    expected[subject] += amt;
                }
                2 => {
                    let mut recipients = Vec::new(&s.env);
                    for offset in 0..3 {
                        let target = ((subject + offset + other) % holders.len()) % holders.len();
                        let payout = (amount + offset as i128) % 50 + 1;
                        recipients.push_back((holders[target].clone(), payout));
                        expected[target] += payout;
                    }
                    s.token.mint_batch(&s.admin, &recipients);
                }
                _ => {
                    let amt = if expected[subject] == 0 {
                        0
                    } else {
                        1 + (amount % expected[subject].max(1))
                    };
                    if amt > 0 {
                        s.token.burn(&holders[subject], &amt);
                        expected[subject] -= amt;
                    }
                }
            }

            let mut sum = 0i128;
            for holder in &holders {
                sum += s.token.balance(holder);
            }
            let mut tracking = 0i128;
            for bal in expected.iter() {
                tracking += *bal;
            }
            assert_eq!(sum, s.token.total_supply());
            assert_eq!(sum, tracking);
        }
    }
}

proptest! {
    #[test]
    fn prop_paused_token_permits_no_balance_movement(
        ops in prop::collection::vec((any::<u8>(), any::<u8>(), any::<u8>(), any::<u8>()), 1..20),
    ) {
        let s = setup(1_000);
        let holders = [
            s.admin.clone(),
            Address::generate(&s.env),
            Address::generate(&s.env),
            Address::generate(&s.env),
            Address::generate(&s.env),
        ];
        for holder in &holders[1..] {
            approve(&s.env, &s.compliance, &s.admin, holder);
        }
        s.token.pause(&s.admin);

        let before_supply = s.token.total_supply();
        let before_balances: std::vec::Vec<i128> = holders
            .iter()
            .map(|holder| s.token.balance(holder))
            .collect();

        for op in ops {
            let action = op.0 % 4;
            let subject = (op.1 as usize) % holders.len();
            let other = (op.2 as usize) % holders.len();
            let amount = (op.3 as i128 % 50) + 1;
            match action {
                0 => {
                    let res = s.token.try_transfer(&holders[subject], &holders[other], &amount);
                    assert!(matches!(res, Err(Ok(Error::Paused))));
                }
                1 => {
                    let res = s.token.try_mint(&s.admin, &holders[subject], &amount);
                    assert!(matches!(res, Err(Ok(Error::Paused))));
                }
                2 => {
                    let mut recipients = Vec::new(&s.env);
                    for offset in 0..2 {
                        let target = ((subject + offset + other) % holders.len()) % holders.len();
                        let payout = amount + offset as i128;
                        recipients.push_back((holders[target].clone(), payout));
                    }
                    let res = s.token.try_mint_batch(&s.admin, &recipients);
                    assert!(matches!(res, Err(Ok(Error::Paused))));
                }
                _ => {
                    let res = s.token.try_burn(&holders[subject], &amount);
                    assert!(matches!(res, Err(Ok(Error::Paused))));
                }
            }

            for (idx, holder) in holders.iter().enumerate() {
                assert_eq!(s.token.balance(holder), before_balances[idx]);
            }
            assert_eq!(s.token.total_supply(), before_supply);
        }
    }
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

// ---- issue #185: self-transfer still enforces balance and compliance checks ----

#[test]
fn test_self_transfer_exceeding_balance_fails() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    // Bob has zero balance; a self-transfer must still hit the balance check
    // before the self-transfer short-circuit.
    let res = s.token.try_transfer(&bob, &bob, &1);
    assert_eq!(res, Err(Ok(Error::InsufficientBalance)));
}

#[test]
fn test_self_transfer_by_suspended_holder_fails() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.mint(&s.admin, &bob, &500);
    s.compliance.suspend(&s.admin, &bob);
    // The sender-compliance check must still run before the self-transfer
    // short-circuit.
    let res = s.token.try_transfer(&bob, &bob, &100);
    assert_eq!(res, Err(Ok(Error::SenderNotCompliant)));
}

// ---- issue #186: mint_batch reverts entirely when one recipient fails compliance ----

#[test]
fn test_mint_batch_reverts_entirely_on_noncompliant_recipient() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    let eve = Address::generate(&s.env); // never approved
    approve(&s.env, &s.compliance, &s.admin, &bob);

    let supply_before = s.token.total_supply();
    let mut recipients = Vec::new(&s.env);
    recipients.push_back((bob.clone(), 100i128));
    recipients.push_back((eve, 50i128));

    let res = s.token.try_mint_batch(&s.admin, &recipients);
    assert_eq!(res, Err(Ok(Error::RecipientNotCompliant)));

    // The whole batch must revert: bob's balance and total_supply are untouched.
    assert_eq!(s.token.balance(&bob), 0);
    assert_eq!(s.token.total_supply(), supply_before);
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

// ---- mint_batch ----

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_mint_batch_blocked_when_paused() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.pause(&s.admin);
    let mut recipients = Vec::new(&s.env);
    recipients.push_back((bob, 100));
    s.token.mint_batch(&s.admin, &recipients);
}

#[test]
fn test_mint_batch_succeeds_after_unpause() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.pause(&s.admin);
    s.token.unpause(&s.admin);
    let mut recipients = Vec::new(&s.env);
    recipients.push_back((bob.clone(), 100));
    s.token.mint_batch(&s.admin, &recipients);
    assert_eq!(s.token.balance(&bob), 100);
    assert_eq!(s.token.total_supply(), 1_100);
}

#[test]
fn test_mint_batch_empty_is_noop() {
    let s = setup(1_000);
    let supply_before = s.token.total_supply();
    let events_before = s.env.events().all().len();
    let recipients: Vec<(Address, i128)> = Vec::new(&s.env);
    s.token.mint_batch(&s.admin, &recipients);
    assert_eq!(s.token.total_supply(), supply_before);
    assert_eq!(s.env.events().all().len(), events_before);
}

#[test]
fn test_mint_batch_credits_repeated_recipient_cumulatively() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    let supply_before = s.token.total_supply();
    let mut recipients = Vec::new(&s.env);
    recipients.push_back((bob.clone(), 100));
    recipients.push_back((bob.clone(), 50));
    s.token.mint_batch(&s.admin, &recipients);
    assert_eq!(s.token.balance(&bob), 150);
    assert_eq!(s.token.total_supply(), supply_before + 150);
}
