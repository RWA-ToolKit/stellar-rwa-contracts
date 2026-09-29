#![cfg(test)]
use super::*;
use compliance::{ComplianceContract, ComplianceContractClient};
use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, Events, Ledger},
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
fn test_approve_then_transfer_from() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    let carol = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    approve(&s.env, &s.compliance, &s.admin, &carol);

    let expiration = s.env.ledger().sequence() + 1_000;
    s.token.approve(&s.admin, &bob, &300, &expiration);
    assert_eq!(s.token.allowance(&s.admin, &bob), 300);

    s.token.transfer_from(&bob, &s.admin, &carol, &200);
    assert_eq!(s.token.balance(&s.admin), 800);
    assert_eq!(s.token.balance(&carol), 200);
    assert_eq!(s.token.allowance(&s.admin, &bob), 100);
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn test_transfer_from_over_allowance_rejected() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    let carol = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    approve(&s.env, &s.compliance, &s.admin, &carol);

    let expiration = s.env.ledger().sequence() + 1_000;
    s.token.approve(&s.admin, &bob, &100, &expiration);
    s.token.transfer_from(&bob, &s.admin, &carol, &200);
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn test_transfer_from_after_expiry_rejected() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    let carol = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    approve(&s.env, &s.compliance, &s.admin, &carol);

    let expiration = s.env.ledger().sequence() + 5;
    s.token.approve(&s.admin, &bob, &200, &expiration);
    s.env
        .ledger()
        .with_mut(|li| li.sequence_number = expiration + 1);
    assert_eq!(s.token.allowance(&s.admin, &bob), 0);
    s.token.transfer_from(&bob, &s.admin, &carol, &50);
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

        let mut expected = std::vec![0i128; holders.len()];
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
                    assert_eq!(res, Err(Ok(Error::Paused.into())));
                }
                1 => {
                    let res = s.token.try_mint(&s.admin, &holders[subject], &amount);
                    assert_eq!(res, Err(Ok(Error::Paused.into())));
                }
                2 => {
                    let mut recipients = Vec::new(&s.env);
                    for offset in 0..2 {
                        let target = ((subject + offset + other) % holders.len()) % holders.len();
                        let payout = amount + offset as i128;
                        recipients.push_back((holders[target].clone(), payout));
                    }
                    let res = s.token.try_mint_batch(&s.admin, &recipients);
                    assert_eq!(res, Err(Ok(Error::Paused.into())));
                }
                _ => {
                    let res = s.token.try_burn(&holders[subject], &amount);
                    assert_eq!(res, Err(Ok(Error::Paused.into())));
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
#[should_panic(expected = "Error(Contract, #6)")]
fn test_burn_blocked_when_paused() {
    let s = setup(1_000);
    s.token.pause(&s.admin);
    s.token.burn(&s.admin, &100);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_approve_blocked_when_paused() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    s.token.pause(&s.admin);
    s.token
        .approve(&s.admin, &bob, &100, &(s.env.ledger().sequence() + 100));
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_transfer_from_blocked_when_paused() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    let carol = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    approve(&s.env, &s.compliance, &s.admin, &carol);
    let expiration = s.env.ledger().sequence() + 100;
    s.token.approve(&s.admin, &bob, &100, &expiration);
    s.token.pause(&s.admin);
    s.token.transfer_from(&bob, &s.admin, &carol, &50);
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
fn test_guardian_absent_by_default() {
    let s = setup(1_000);
    assert_eq!(s.token.get_metadata().guardian, None);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_pause_by_stranger_reverts() {
    let s = setup(1_000);
    let stranger = Address::generate(&s.env);
    s.token.pause(&stranger);
}

#[test]
fn test_guardian_can_pause() {
    let s = setup(1_000);
    let guardian = Address::generate(&s.env);
    s.token.set_guardian(&s.admin, &Some(guardian.clone()));
    assert_eq!(s.token.get_metadata().guardian, Some(guardian.clone()));
    s.token.pause(&guardian);
    assert!(s.token.get_metadata().paused);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_guardian_cannot_unpause() {
    let s = setup(1_000);
    let guardian = Address::generate(&s.env);
    s.token.set_guardian(&s.admin, &Some(guardian.clone()));
    s.token.pause(&guardian);
    s.token.unpause(&guardian);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_guardian_cannot_mint() {
    let s = setup(1_000);
    let guardian = Address::generate(&s.env);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.set_guardian(&s.admin, &Some(guardian.clone()));
    s.token.mint(&guardian, &bob, &100);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_set_guardian_by_non_admin_reverts() {
    let s = setup(1_000);
    let impostor = Address::generate(&s.env);
    let guardian = Address::generate(&s.env);
    s.token.set_guardian(&impostor, &Some(guardian));
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_cleared_guardian_loses_pause_rights() {
    let s = setup(1_000);
    let guardian = Address::generate(&s.env);
    s.token.set_guardian(&s.admin, &Some(guardian.clone()));
    s.token.set_guardian(&s.admin, &None);
    assert_eq!(s.token.get_metadata().guardian, None);
    s.token.pause(&guardian);
}

#[test]
fn test_admin_still_pauses_with_guardian_set() {
    let s = setup(1_000);
    let guardian = Address::generate(&s.env);
    s.token.set_guardian(&s.admin, &Some(guardian));
    s.token.pause(&s.admin);
    assert!(s.token.get_metadata().paused);
}

#[test]
fn test_update_valuation() {
    let s = setup(1_000);
    // Valuation is local token metadata; a separately registered valuation is
    // not synchronized by this call.
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
#[should_panic(expected = "Error(Contract, #14)")]
fn test_update_valuation_oversized_change_rejected() {
    let s = setup(1_000);
    // Initial valuation is 50_000_000; more than a 50% jump must be rejected.
    s.token.update_valuation(&s.admin, &200_000_000);
}

#[test]
fn test_update_valuation_emits_event() {
    let s = setup(1_000);
    s.token.update_valuation(&s.admin, &60_000_000);
    // `events().all()` reflects the most recent contract invocation only, so
    // read it immediately after the call under test.
    assert_eq!(s.env.events().all().events().len(), 1);
    assert_eq!(s.token.get_metadata().valuation, 60_000_000);
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
fn test_set_compliance_emits_old_and_new_addresses() {
    let s = setup(1_000);
    let comp2_id = env_register_empty_compliance(&s.env, &s.admin);
    let comp2 = ComplianceContractClient::new(&s.env, &comp2_id);
    approve(&s.env, &comp2, &s.admin, &s.admin);
    let old_compliance = s.token.get_metadata().compliance_contract;
    s.token.set_compliance(&s.admin, &comp2_id);
    // Read events right after the call: `events().all()` only reflects the
    // most recent contract invocation, and a later view call would clear it.
    let all_events = s.env.events().all();
    assert!(!all_events.events().is_empty());
    assert_eq!(old_compliance, s.compliance_id);
    assert_eq!(s.token.get_metadata().compliance_contract, comp2_id);
}

#[test]
#[should_panic]
fn test_set_compliance_rejects_non_conforming_target() {
    let s = setup(1_000);
    // A plain account address does not implement `is_allowed`; probing it as
    // a compliance target must fail rather than being silently accepted.
    let not_a_compliance_contract = Address::generate(&s.env);
    s.token.set_compliance(&s.admin, &not_a_compliance_contract);
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

/// Pins the deliberate policy documented on `AssetTokenContract::burn`: a
/// holder who is *suspended* (as opposed to fully removed) is still
/// compliance-gated and may not burn their tokens.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_burn_blocked_when_holder_suspended() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.transfer(&s.admin, &bob, &200);
    // Bob holds tokens; suspend (not remove) his approval.
    s.compliance.suspend(&s.admin, &bob);
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
    assert_eq!(res, Err(Ok(Error::InsufficientBalance.into())));
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
    assert_eq!(res, Err(Ok(Error::SenderNotCompliant.into())));
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
    assert_eq!(res, Err(Ok(Error::RecipientNotCompliant.into())));

    // The whole batch must revert: bob's balance and total_supply are untouched.
    assert_eq!(s.token.balance(&bob), 0);
    assert_eq!(s.token.total_supply(), supply_before);
}

#[test]
fn test_mint_batch_partial_failure_does_not_touch_prior_balance() {
    // Stronger atomicity proof than a fresh-recipient check: bob already
    // holds a balance before the batch runs, so a naive "credit as you go"
    // implementation could still leave his balance bumped even though the
    // whole call is supposed to revert. This asserts it is left exactly as
    // it was.
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    let carol = Address::generate(&s.env);
    let eve = Address::generate(&s.env); // never approved
    approve(&s.env, &s.compliance, &s.admin, &bob);
    approve(&s.env, &s.compliance, &s.admin, &carol);

    s.token.mint(&s.admin, &bob, &500);
    let bob_balance_before = s.token.balance(&bob);
    let supply_before = s.token.total_supply();

    let mut recipients = Vec::new(&s.env);
    recipients.push_back((bob.clone(), 100i128));
    recipients.push_back((carol.clone(), 25i128));
    recipients.push_back((eve, 50i128));

    let res = s.token.try_mint_batch(&s.admin, &recipients);
    assert_eq!(res, Err(Ok(Error::RecipientNotCompliant.into())));

    assert_eq!(s.token.balance(&bob), bob_balance_before);
    assert_eq!(s.token.balance(&carol), 0);
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
    let events_before = s.env.events().all().events().len();
    let recipients: Vec<(Address, i128)> = Vec::new(&s.env);
    s.token.mint_batch(&s.admin, &recipients);
    assert_eq!(s.token.total_supply(), supply_before);
    assert_eq!(s.env.events().all().events().len(), events_before);
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

// ---- issue #310: get_metadata must reflect every mutated field simultaneously ----

/// Calls update_valuation, pause, unpause, set_compliance, and mint in sequence,
/// then asserts every AssetMetadata field in a single get_metadata call.
/// This catches a setter that accidentally overwrites an unrelated field —
/// something that per-field tests cannot detect because they read metadata
/// in isolation immediately after their own setter.
#[test]
fn test_get_metadata_reflects_all_mutations() {
    let s = setup(1_000);

    // ── Step 1: update_valuation (within the 50% per-update guard) ───────────
    s.token.update_valuation(&s.admin, &70_000_000);

    // ── Step 2: pause then unpause (paused must end up false) ────────────────
    s.token.pause(&s.admin);
    s.token.unpause(&s.admin);

    // ── Step 3: swap compliance contract ────────────────────────────────────
    let comp2_id = env_register_empty_compliance(&s.env, &s.admin);
    let comp2 = ComplianceContractClient::new(&s.env, &comp2_id);
    // Admin must be approved under the new contract or set_compliance will
    // panic InvalidCompliance.
    approve(&s.env, &comp2, &s.admin, &s.admin);
    s.token.set_compliance(&s.admin, &comp2_id);

    // ── Step 4: mint to a compliant recipient (increases total_supply) ───────
    let bob = Address::generate(&s.env);
    approve(&s.env, &comp2, &s.admin, &bob);
    s.token.mint(&s.admin, &bob, &500);

    // ── Single get_metadata snapshot: every field checked together ───────────
    let meta = s.token.get_metadata();

    // Fields touched by the setters above.
    assert_eq!(meta.valuation, 70_000_000, "valuation not updated");
    assert!(!meta.paused, "paused flag should be false after unpause");
    assert_eq!(
        meta.compliance_contract, comp2_id,
        "compliance_contract not switched"
    );
    assert_eq!(
        meta.total_supply, 1_500,
        "total_supply not updated after mint"
    );

    // Fields that must be unchanged — another setter silently clobbering one
    // of these would be caught here but not by the individual setter tests.
    assert_eq!(
        meta.name,
        String::from_str(&s.env, "Manhattan Loft"),
        "name was clobbered"
    );
    assert_eq!(
        meta.symbol,
        String::from_str(&s.env, "MLOFT"),
        "symbol was clobbered"
    );
    assert_eq!(
        meta.asset_type,
        String::from_str(&s.env, "real_estate"),
        "asset_type was clobbered"
    );
    assert_eq!(meta.decimals, 2u32, "decimals was clobbered");
    assert_eq!(meta.admin, s.admin, "admin was clobbered");
    assert_eq!(
        meta.asset_description,
        String::from_str(&s.env, "A tokenized NYC loft"),
        "asset_description was clobbered",
    );
}

// ---- issue #309: total_supply() is its own public ABI entry point ----

/// Directly exercises `AssetTokenContract::total_supply` (a separate public
/// entry point with its own ABI surface, independent of `get_metadata`) and
/// asserts it tracks the initial supply together with mint, mint_batch, and
/// burn in a single flow. Supply changes always land on the same underlying
/// ledger key as the metadata-reported supply, so both read paths must agree.
#[test]
fn test_total_supply_tracks_mint_burn_mint_batch() {
    let s = setup(1_000);

    // Constructor: total_supply mirrors the initial supply.
    assert_eq!(s.token.total_supply(), 1_000);
    assert_eq!(s.token.get_metadata().total_supply, 1_000);

    // mint: 1,000 + 250 = 1,250.
    let bob = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &bob);
    s.token.mint(&s.admin, &bob, &250);
    assert_eq!(s.token.balance(&bob), 250);
    assert_eq!(s.token.total_supply(), 1_250);

    // mint_batch: 1,250 + (100 + 50) = 1,400 — repeated recipient is cumulative.
    let mut recipients = Vec::new(&s.env);
    recipients.push_back((bob.clone(), 100));
    recipients.push_back((bob.clone(), 50));
    s.token.mint_batch(&s.admin, &recipients);
    assert_eq!(s.token.balance(&bob), 400);
    assert_eq!(s.token.total_supply(), 1_400);

    // burn (from admin's balance): 1,400 - 200 = 1,200.
    s.token.burn(&s.admin, &200);
    assert_eq!(s.token.balance(&s.admin), 800);
    assert_eq!(s.token.total_supply(), 1_200);

    // The metadata-reported supply stays in lockstep with the direct ABI read.
    assert_eq!(s.token.get_metadata().total_supply, s.token.total_supply());
}

// ---- issue #3: compliance admin and asset-token admin may diverge ----

/// `scripts/deploy.sh` initializes both contracts with the same address as a
/// convenience default, but nothing in either contract ties the two admins
/// together. This proves a real deployment can use two distinct addresses —
/// a dedicated compliance officer and a separate asset-token admin — with
/// each administering only their own contract.
#[test]
fn test_compliance_admin_diverges_from_asset_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let asset_admin = Address::generate(&env);
    let compliance_officer = Address::generate(&env);
    assert_ne!(asset_admin, compliance_officer);

    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    compliance.initialize(&compliance_officer);

    // The compliance officer — not the asset admin — approves the asset
    // admin to hold the initial supply.
    approve(&env, &compliance, &compliance_officer, &asset_admin);

    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);
    token.initialize(
        &asset_admin,
        &String::from_str(&env, "Manhattan Loft"),
        &String::from_str(&env, "MLOFT"),
        &String::from_str(&env, "real_estate"),
        &1_000i128,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );

    // The two admins are recorded independently and are not equal.
    assert_eq!(token.get_metadata().admin, asset_admin);
    assert_eq!(compliance.get_admin(), compliance_officer);

    // The compliance officer administers KYC entirely on their own: approve,
    // suspend, and block a jurisdiction, none of which involves asset_admin.
    let bob = Address::generate(&env);
    approve(&env, &compliance, &compliance_officer, &bob);
    token.transfer(&asset_admin, &bob, &100);
    assert_eq!(token.balance(&bob), 100);

    compliance.suspend(&compliance_officer, &bob);
    let res = token.try_transfer(&bob, &asset_admin, &10);
    assert_eq!(res, Err(Ok(Error::SenderNotCompliant.into())));

    // The asset admin independently retains full control of the token (mint
    // still requires only asset_admin, never the compliance officer).
    let carol = Address::generate(&env);
    approve(&env, &compliance, &compliance_officer, &carol);
    token.mint(&asset_admin, &carol, &50);
    assert_eq!(token.balance(&carol), 50);

    // Neither admin has authority over the other's contract.
    let dave = Address::generate(&env);
    let res =
        compliance.try_add_to_allowlist(&asset_admin, &dave, &String::from_str(&env, "US"), &0);
    assert_eq!(res, Err(Ok(compliance::Error::Unauthorized.into())));

    let res = token.try_mint(&compliance_officer, &carol, &10);
    assert_eq!(res, Err(Ok(Error::Unauthorized.into())));
}

// ---- admin handover (issue #4) ----

#[test]
fn test_propose_accept_admin_moves_role_only_on_acceptance() {
    let s = setup(1_000);
    let successor = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &successor);

    s.token.propose_admin(&s.admin, &successor);
    // Role must not move until accepted.
    assert_eq!(s.token.get_metadata().admin, s.admin);
    assert_eq!(s.token.get_pending_admin(), Some(successor.clone()));

    s.token.accept_admin(&successor);
    assert_eq!(s.token.get_metadata().admin, successor);
    assert_eq!(s.token.get_pending_admin(), None);

    // The old admin has lost its privileges.
    let res = s.token.try_mint(&s.admin, &successor, &10);
    assert_eq!(res, Err(Ok(Error::Unauthorized.into())));
    // The new admin can act.
    s.token.mint(&successor, &successor, &10);
}

#[test]
fn test_cancel_admin_proposal_by_current_admin() {
    let s = setup(1_000);
    let successor = Address::generate(&s.env);

    s.token.propose_admin(&s.admin, &successor);
    s.token.cancel_admin_proposal(&s.admin);
    assert_eq!(s.token.get_pending_admin(), None);

    // The cancelled successor can no longer accept.
    let res = s.token.try_accept_admin(&successor);
    assert_eq!(res, Err(Ok(Error::NoPendingAdmin.into())));
    // The admin is unchanged.
    assert_eq!(s.token.get_metadata().admin, s.admin);
}

#[test]
fn test_cancel_admin_proposal_with_nothing_pending_fails() {
    let s = setup(1_000);
    let res = s.token.try_cancel_admin_proposal(&s.admin);
    assert_eq!(res, Err(Ok(Error::NoPendingAdmin.into())));
}

#[test]
fn test_non_admin_cannot_propose_admin() {
    let s = setup(1_000);
    let non_admin = Address::generate(&s.env);
    let successor = Address::generate(&s.env);
    let res = s.token.try_propose_admin(&non_admin, &successor);
    assert_eq!(res, Err(Ok(Error::Unauthorized.into())));
}

#[test]
fn test_non_admin_cannot_cancel_admin_proposal() {
    let s = setup(1_000);
    let non_admin = Address::generate(&s.env);
    let successor = Address::generate(&s.env);
    s.token.propose_admin(&s.admin, &successor);
    let res = s.token.try_cancel_admin_proposal(&non_admin);
    assert_eq!(res, Err(Ok(Error::Unauthorized.into())));
}

#[test]
fn test_only_proposed_successor_can_accept() {
    let s = setup(1_000);
    let successor = Address::generate(&s.env);
    let impostor = Address::generate(&s.env);

    s.token.propose_admin(&s.admin, &successor);
    let res = s.token.try_accept_admin(&impostor);
    assert_eq!(res, Err(Ok(Error::Unauthorized.into())));
    // Role is unaffected by the failed attempt.
    assert_eq!(s.token.get_metadata().admin, s.admin);
}

#[test]
fn test_accept_admin_with_no_pending_proposal_fails() {
    let s = setup(1_000);
    let stranger = Address::generate(&s.env);
    let res = s.token.try_accept_admin(&stranger);
    assert_eq!(res, Err(Ok(Error::NoPendingAdmin.into())));
}

#[test]
fn test_admin_handover_does_not_affect_guardian() {
    // Issue #4: propose/accept must be independent of the guardian (issue #1).
    let s = setup(1_000);
    let guardian = Address::generate(&s.env);
    let successor = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &successor);

    s.token.set_guardian(&s.admin, &Some(guardian.clone()));
    s.token.propose_admin(&s.admin, &successor);
    s.token.accept_admin(&successor);

    assert_eq!(s.token.get_metadata().admin, successor);
    assert_eq!(s.token.get_metadata().guardian, Some(guardian));
}

// Issue #370: Test that token metadata survives a TTL boundary.
#[test]
fn test_metadata_survives_ttl_boundary() {
    let s = setup(1_000);

    // Verify metadata is readable
    let metadata = s.token.get_metadata();
    assert_eq!(metadata.name, String::from_str(&s.env, "Manhattan Loft"));
    assert_eq!(metadata.symbol, String::from_str(&s.env, "MLOFT"));

    // Advance ledger past TTL threshold
    s.env.ledger().set_sequence_number(500_000);

    // Verify metadata still exists after ledger advance
    let metadata_after = s.token.get_metadata();
    assert_eq!(
        metadata_after.name, metadata.name,
        "Metadata must survive TTL boundary"
    );
    assert_eq!(metadata_after.symbol, metadata.symbol);
}

// Issue #371: Test that account balances survive a TTL boundary.
#[test]
fn test_balances_survive_ttl_boundary() {
    let s = setup(1_000);
    let user = Address::generate(&s.env);
    approve(&s.env, &s.compliance, &s.admin, &user);

    // Initial state: admin has 1000, user has 0
    assert_eq!(s.token.balance(&s.admin), 1_000);
    assert_eq!(s.token.balance(&user), 0);

    // Transfer some tokens
    s.token.transfer(&s.admin, &user, &300);
    assert_eq!(s.token.balance(&s.admin), 700);
    assert_eq!(s.token.balance(&user), 300);

    // Advance ledger past TTL threshold
    s.env.ledger().set_sequence_number(500_000);

    // Verify balances still exist after ledger advance
    assert_eq!(
        s.token.balance(&s.admin),
        700,
        "Admin balance must survive TTL boundary"
    );
    assert_eq!(
        s.token.balance(&user),
        300,
        "User balance must survive TTL boundary"
    );
}

// ---- extreme `decimals` documentation test ----
//
// `initialize` accepts any `u32` for `decimals` with no upper-bound
// validation. `decimals` is stored as opaque metadata by this contract and is
// never used in on-chain arithmetic here (balances/`total_supply` are raw
// `i128` units, independent of `decimals`), so a huge `decimals` value does
// not by itself overflow anything inside asset-token. The risk is entirely
// downstream: a consumer (e.g. a UI, or another contract computing
// `total_amount * basis` scaled by `10^decimals`, as `dividend` effectively
// does when interpreting amounts) that treats `decimals` as bounded (e.g.
// `<= 18`, matching typical token conventions) could overflow or produce
// nonsensical results. This test documents *current* behavior: `initialize`
// happily accepts `decimals = 255` (u8::MAX, an extreme but valid `u32`)
// alongside a large `total_supply` and `valuation`, and all reads
// (`get_metadata`, `total_supply`, `balance`) remain internally consistent.
// If an upper bound is added later (closing this gap), this test's
// expectations should change from "accepted" to "rejected".
#[test]
fn test_extreme_decimals_accepted_with_large_supply_and_valuation() {
    let env = Env::default();
    env.mock_all_auths();

    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    let admin = Address::generate(&env);
    compliance.initialize(&admin);
    approve(&env, &compliance, &admin, &admin);

    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);

    // A supply and valuation near the top of what `i128` can represent,
    // combined with an extreme decimals value, to probe for overflow or
    // inconsistent reads in asset-token's own storage/arithmetic.
    let huge_supply: i128 = 170_141_183_460_469_231_731_687_303_715_884_105_727 / 2;
    let huge_valuation: i128 = huge_supply;
    let extreme_decimals: u32 = 255;

    token.initialize(
        &admin,
        &String::from_str(&env, "Extreme Decimals Asset"),
        &String::from_str(&env, "XTRM"),
        &String::from_str(&env, "real_estate"),
        &huge_supply,
        &extreme_decimals,
        &compliance_id,
        &String::from_str(&env, "Documents current no-upper-bound decimals behavior"),
        &huge_valuation,
    );

    // `initialize` did not panic or clamp `decimals`; it is stored verbatim.
    let meta = token.get_metadata();
    assert_eq!(meta.decimals, extreme_decimals);
    assert_eq!(meta.total_supply, huge_supply);
    assert_eq!(meta.valuation, huge_valuation);

    // Balance/supply bookkeeping stays internally consistent regardless of
    // the (unrelated, unused-in-arithmetic) decimals value.
    assert_eq!(token.balance(&admin), huge_supply);
    assert_eq!(token.total_supply(), huge_supply);

    // A subsequent mint still behaves normally: `decimals` plays no role in
    // asset-token's own overflow checks, only in how a downstream consumer
    // might choose to scale/interpret raw i128 amounts.
    let bob = Address::generate(&env);
    approve(&env, &compliance, &admin, &bob);
    token.mint(&admin, &bob, &1_000);
    assert_eq!(token.balance(&bob), 1_000);
    assert_eq!(token.total_supply(), huge_supply + 1_000);
}

// Issue #376: Property test for supply conservation
proptest! {
    #[test]
    fn prop_supply_conserved_after_operations(
        mint_amounts in prop::collection::vec(1i128..100_000i128, 0..5),
        burn_amount in 0i128..1_000_000i128,
    ) {
        let s = setup(1_000_000);
        let initial_supply = s.token.total_supply();

        let bob = Address::generate(&s.env);
        approve(&s.env, &s.compliance, &s.admin, &bob);

        for amount in mint_amounts {
            s.token.mint(&s.admin, &bob, &amount);
        }

        let supply_after_mints = s.token.total_supply();
        let bob_balance = s.token.balance(&bob);

        if burn_amount <= s.token.balance(&s.admin) {
            s.token.burn(&s.admin, &burn_amount);
        }

        let final_supply = s.token.total_supply();

        // Check: sum of all balances equals total supply
        let admin_balance = s.token.balance(&s.admin);
        let sum_of_balances = admin_balance.saturating_add(bob_balance);

        prop_assert_eq!(
            final_supply, sum_of_balances,
            "Supply conservation violated: total={}, sum_of_balances={}",
            final_supply, sum_of_balances
        );
    }
}

// Issue #375: Property test for compliance transfer gate
proptest! {
    #[test]
    fn prop_transfer_gate_enforced(
        sender_approved in prop::bool::ANY,
        recipient_approved in prop::bool::ANY,
    ) {
        let s = setup(1_000);
        let sender = Address::generate(&s.env);
        let recipient = Address::generate(&s.env);

        if sender_approved {
            approve(&s.env, &s.compliance, &s.admin, &sender);
        }
        if recipient_approved {
            approve(&s.env, &s.compliance, &s.admin, &recipient);
        }

        // Mint to sender
        if sender_approved {
            s.token.mint(&s.admin, &sender, &100);
        }

        // Transfer succeeds only if BOTH are approved
        let should_succeed = sender_approved && recipient_approved;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            s.token.transfer(&sender, &recipient, &50);
        }));

        if should_succeed {
            prop_assert!(result.is_ok(), "Transfer should succeed when both approved");
        } else {
            prop_assert!(result.is_err(), "Transfer should fail when either party not approved");
        }
    }
}

/// If the configured compliance contract address holds no contract at all,
/// the cross-contract call the gate depends on cannot execute and the host
/// traps. This is deliberate: a transfer must never silently succeed (or
/// silently fail open) when the compliance gate is unreachable — it fails
/// hard, aborting the whole transaction. Documented in docs/asset-token.md.
#[test]
#[should_panic]
fn test_gate_traps_when_compliance_address_has_no_contract() {
    let s = setup(1_000);
    let ghost = Address::generate(&s.env);
    // `set_compliance` itself calls into the new gate to sanity-check it
    // before switching, so pointing it at a non-contract address exercises
    // exactly the same `is_allowed` call path that `transfer`/`mint` use.
    s.token.set_compliance(&s.admin, &ghost);
}

/// Same failure mode, exercised directly against `transfer` rather than
/// `set_compliance`: an already-configured but unreachable compliance
/// contract (e.g. one that was valid at `set_compliance` time but has since
/// been removed, or any address with no deployed contract) traps the call
/// instead of allowing or silently blocking the transfer.
#[test]
#[should_panic]
fn test_transfer_traps_when_compliance_contract_is_unreachable() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    // A plain generated address with no contract registered at it.
    let ghost_compliance = Address::generate(&env);
    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);
    // `initialize` itself consults the compliance gate (to check the admin), so
    // pointing it at an address with no contract traps at the earliest possible
    // point — the same `is_allowed` call path `transfer`/`mint` use.
    token.initialize(
        &admin,
        &String::from_str(&env, "Ghost Asset"),
        &String::from_str(&env, "GHOST"),
        &String::from_str(&env, "real_estate"),
        &1_000i128,
        &2u32,
        &ghost_compliance,
        &String::from_str(&env, "no contract at this address"),
        &1_000i128,
    );
}
