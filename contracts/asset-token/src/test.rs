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

// ---- issue #191 / #192: initialize string-length validation ----

/// Build an all-`a` ASCII `String` of exactly `len` bytes, for exercising
/// `check_str`'s min/max boundaries.
fn ascii_string(env: &Env, len: usize) -> String {
    const BUF: [u8; 300] = [b'a'; 300];
    let s = core::str::from_utf8(&BUF[..len]).unwrap();
    String::from_str(env, s)
}

/// A registered, un-initialized token plus a compliance contract that already
/// approves `admin`, for exercising `initialize` boundary conditions.
fn uninitialized_token() -> (Env, Address, Address, AssetTokenContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    let admin = Address::generate(&env);
    compliance.initialize(&admin);
    approve(&env, &compliance, &admin, &admin);

    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);
    (env, compliance_id, admin, token)
}

#[test]
fn test_initialize_rejects_name_over_max_len() {
    let (env, compliance_id, admin, token) = uninitialized_token();
    let result = token.try_initialize(
        &admin,
        &ascii_string(&env, MAX_NAME_LEN as usize + 1),
        &String::from_str(&env, "MLOFT"),
        &String::from_str(&env, "real_estate"),
        &1_000i128,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );
    assert_eq!(result, Err(Ok(Error::InvalidInput)));
}

#[test]
fn test_initialize_accepts_name_at_max_len() {
    let (env, compliance_id, admin, token) = uninitialized_token();
    let name = ascii_string(&env, MAX_NAME_LEN as usize);
    token.initialize(
        &admin,
        &name,
        &String::from_str(&env, "MLOFT"),
        &String::from_str(&env, "real_estate"),
        &1_000i128,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );
    assert_eq!(token.get_metadata().name, name);
}

#[test]
fn test_initialize_rejects_symbol_over_max_len() {
    let (env, compliance_id, admin, token) = uninitialized_token();
    let result = token.try_initialize(
        &admin,
        &String::from_str(&env, "Manhattan Loft"),
        &ascii_string(&env, MAX_SYMBOL_LEN as usize + 1),
        &String::from_str(&env, "real_estate"),
        &1_000i128,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );
    assert_eq!(result, Err(Ok(Error::InvalidInput)));
}

#[test]
fn test_initialize_accepts_symbol_at_max_len() {
    let (env, compliance_id, admin, token) = uninitialized_token();
    let symbol = ascii_string(&env, MAX_SYMBOL_LEN as usize);
    token.initialize(
        &admin,
        &String::from_str(&env, "Manhattan Loft"),
        &symbol,
        &String::from_str(&env, "real_estate"),
        &1_000i128,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );
    assert_eq!(token.get_metadata().symbol, symbol);
}

#[test]
fn test_initialize_rejects_asset_type_over_max_len() {
    let (env, compliance_id, admin, token) = uninitialized_token();
    let result = token.try_initialize(
        &admin,
        &String::from_str(&env, "Manhattan Loft"),
        &String::from_str(&env, "MLOFT"),
        &ascii_string(&env, MAX_ASSET_TYPE_LEN as usize + 1),
        &1_000i128,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );
    assert_eq!(result, Err(Ok(Error::InvalidInput)));
}

#[test]
fn test_initialize_accepts_asset_type_at_max_len() {
    let (env, compliance_id, admin, token) = uninitialized_token();
    let asset_type = ascii_string(&env, MAX_ASSET_TYPE_LEN as usize);
    token.initialize(
        &admin,
        &String::from_str(&env, "Manhattan Loft"),
        &String::from_str(&env, "MLOFT"),
        &asset_type,
        &1_000i128,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );
    assert_eq!(token.get_metadata().asset_type, asset_type);
}

#[test]
fn test_initialize_rejects_description_over_max_len() {
    let (env, compliance_id, admin, token) = uninitialized_token();
    let result = token.try_initialize(
        &admin,
        &String::from_str(&env, "Manhattan Loft"),
        &String::from_str(&env, "MLOFT"),
        &String::from_str(&env, "real_estate"),
        &1_000i128,
        &2u32,
        &compliance_id,
        &ascii_string(&env, MAX_DESC_LEN as usize + 1),
        &50_000_000i128,
    );
    assert_eq!(result, Err(Ok(Error::InvalidInput)));
}

#[test]
fn test_initialize_accepts_description_at_max_len() {
    let (env, compliance_id, admin, token) = uninitialized_token();
    let description = ascii_string(&env, MAX_DESC_LEN as usize);
    token.initialize(
        &admin,
        &String::from_str(&env, "Manhattan Loft"),
        &String::from_str(&env, "MLOFT"),
        &String::from_str(&env, "real_estate"),
        &1_000i128,
        &2u32,
        &compliance_id,
        &description,
        &50_000_000i128,
    );
    assert_eq!(token.get_metadata().asset_description, description);
}

#[test]
fn test_initialize_rejects_empty_name() {
    let (env, compliance_id, admin, token) = uninitialized_token();
    let result = token.try_initialize(
        &admin,
        &String::from_str(&env, ""),
        &String::from_str(&env, "MLOFT"),
        &String::from_str(&env, "real_estate"),
        &1_000i128,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );
    assert_eq!(result, Err(Ok(Error::InvalidInput)));
}

#[test]
fn test_initialize_rejects_empty_symbol() {
    let (env, compliance_id, admin, token) = uninitialized_token();
    let result = token.try_initialize(
        &admin,
        &String::from_str(&env, "Manhattan Loft"),
        &String::from_str(&env, ""),
        &String::from_str(&env, "real_estate"),
        &1_000i128,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );
    assert_eq!(result, Err(Ok(Error::InvalidInput)));
}

#[test]
fn test_initialize_rejects_empty_asset_type() {
    let (env, compliance_id, admin, token) = uninitialized_token();
    let result = token.try_initialize(
        &admin,
        &String::from_str(&env, "Manhattan Loft"),
        &String::from_str(&env, "MLOFT"),
        &String::from_str(&env, ""),
        &1_000i128,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );
    assert_eq!(result, Err(Ok(Error::InvalidInput)));
}

#[test]
fn test_initialize_rejects_empty_description() {
    let (env, compliance_id, admin, token) = uninitialized_token();
    let result = token.try_initialize(
        &admin,
        &String::from_str(&env, "Manhattan Loft"),
        &String::from_str(&env, "MLOFT"),
        &String::from_str(&env, "real_estate"),
        &1_000i128,
        &2u32,
        &compliance_id,
        &String::from_str(&env, ""),
        &50_000_000i128,
    );
    assert_eq!(result, Err(Ok(Error::InvalidInput)));
}

// ---- issue #193: initialize with zero total supply ----

#[test]
fn test_initialize_with_zero_total_supply_succeeds() {
    let s = setup(0);
    assert_eq!(s.token.total_supply(), 0);
    assert_eq!(s.token.balance(&s.admin), 0);
}

// ---- issue #194: set_compliance rejects a gate that would not approve the admin ----

#[test]
fn test_set_compliance_rejects_gate_not_approving_admin_and_keeps_previous_gate() {
    let s = setup(1_000);
    let comp2_id = env_register_empty_compliance(&s.env, &s.admin);
    // admin is never added to comp2's allowlist.
    let result = s.token.try_set_compliance(&s.admin, &comp2_id);
    assert_eq!(result, Err(Ok(Error::InvalidCompliance)));
    assert_eq!(s.token.get_metadata().compliance_contract, s.compliance_id);
}
