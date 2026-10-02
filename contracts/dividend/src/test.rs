#![cfg(test)]
use super::*;
use asset_token::{AssetTokenContract, AssetTokenContractClient};
use compliance::{ComplianceContract, ComplianceContractClient};
use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, Ledger},
    token, Address, Env, String, Symbol, Vec,
};

struct Ctx {
    env: Env,
    dividend: DividendContractClient<'static>,
    asset_id: Address,
    pay_id: Address,
    admin: Address,
    h1: Address,
    h2: Address,
    comp_id: Address,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);

    // Compliance with admin + two holders approved.
    let comp_id = env.register(ComplianceContract, ());
    let comp = ComplianceContractClient::new(&env, &comp_id);
    comp.initialize(&admin);
    let us = String::from_str(&env, "US");
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);
    comp.add_to_allowlist(&admin, &admin, &us, &0);
    comp.add_to_allowlist(&admin, &h1, &us, &0);
    comp.add_to_allowlist(&admin, &h2, &us, &0);

    // Asset token: supply 1000 -> admin, then h1=300, h2=200, admin=500.
    let asset_id = env.register(AssetTokenContract, ());
    let asset = AssetTokenContractClient::new(&env, &asset_id);
    asset.initialize(
        &admin,
        &String::from_str(&env, "Loft"),
        &String::from_str(&env, "LFT"),
        &String::from_str(&env, "real_estate"),
        &1000i128,
        &0u32,
        &comp_id,
        &String::from_str(&env, "desc"),
        &1000i128,
    );
    asset.transfer(&admin, &h1, &300);
    asset.transfer(&admin, &h2, &200);

    // Payment token (Stellar Asset Contract), mint 100_000 to admin.
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let pay_id = sac.address();
    token::StellarAssetClient::new(&env, &pay_id).mint(&admin, &100_000);

    let div_id = env.register(DividendContract, ());
    let dividend = DividendContractClient::new(&env, &div_id);
    dividend.initialize(&admin);

    Ctx {
        env,
        dividend,
        asset_id,
        pay_id,
        admin,
        h1,
        h2,
        comp_id,
    }
}

/// Eligible balances as of distribution creation: h1=300, h2=200, admin=500.
fn eligible(ctx: &Ctx) -> Vec<(Address, i128)> {
    let mut v = Vec::new(&ctx.env);
    v.push_back((ctx.h1.clone(), 300));
    v.push_back((ctx.h2.clone(), 200));
    v.push_back((ctx.admin.clone(), 500));
    v
}

fn pay_balance(ctx: &Ctx, who: &Address) -> i128 {
    token::TokenClient::new(&ctx.env, &ctx.pay_id).balance(who)
}

#[test]
fn test_initialize_admin() {
    let ctx = setup();
    assert_eq!(ctx.dividend.get_admin(), ctx.admin);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_get_admin_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let div_id = env.register(DividendContract, ());
    let dividend = DividendContractClient::new(&env, &div_id);
    dividend.get_admin();
}

#[test]
fn test_version() {
    let ctx = setup();
    assert_eq!(ctx.dividend.version(), VERSION);
}

#[test]
fn test_create_distribution_escrows_funds() {
    let ctx = setup();
    let div_addr = ctx.dividend.address.clone();
    let id = ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );
    assert_eq!(id, 1);
    assert_eq!(pay_balance(&ctx, &div_addr), 1000);
    let d = ctx.dividend.get_distribution(&id);
    assert_eq!(d.total_amount, 1000);
    assert_eq!(d.distributed, 0);
    assert!(!d.completed);
}

#[test]
fn test_asset_admin_can_create_distribution() {
    let ctx = setup();
    let asset_admin = AssetTokenContractClient::new(&ctx.env, &ctx.asset_id)
        .get_metadata()
        .admin;
    let id = ctx.dividend.create_distribution(
        &asset_admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &500,
        &eligible(&ctx),
    );
    assert_eq!(id, 1);
    assert_eq!(pay_balance(&ctx, &ctx.dividend.address), 500);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_unrelated_address_cannot_create_distribution() {
    let ctx = setup();
    let stranger = Address::generate(&ctx.env);
    ctx.dividend.create_distribution(
        &stranger,
        &ctx.asset_id,
        &ctx.pay_id,
        &500,
        &eligible(&ctx),
    );
}

#[test]
fn test_claim_is_proportional() {
    let ctx = setup();
    let id = ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );
    // h1 holds 300/1000 -> 300; h2 holds 200/1000 -> 200.
    assert_eq!(ctx.dividend.claimable(&id, &ctx.h1), 300);
    assert_eq!(ctx.dividend.claimable(&id, &ctx.h2), 200);
    assert_eq!(ctx.dividend.claimable(&id, &ctx.admin), 500);

    ctx.dividend.claim(&id, &ctx.h1);
    assert_eq!(pay_balance(&ctx, &ctx.h1), 300);
    assert_eq!(ctx.dividend.claimable(&id, &ctx.h1), 0);
    assert_eq!(ctx.dividend.get_distribution(&id).distributed, 300);
}

// A zero-decimal asset token has integer-only holdings. A payment smaller than
// the snapshot denominator can therefore produce a zero proportional claim.
#[test]
fn test_zero_decimal_asset_small_claim_rounds_to_zero() {
    let ctx = setup();
    assert_eq!(
        AssetTokenContractClient::new(&ctx.env, &ctx.asset_id)
            .get_metadata()
            .decimals,
        0
    );

    let snapshot = Vec::from_array(
        &ctx.env,
        [(ctx.h1.clone(), 1i128), (ctx.h2.clone(), 999i128)],
    );
    let id =
        ctx.dividend
            .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1, &snapshot);

    // 1 payment unit * 1 asset unit / 1000 snapshot units floors to zero.
    assert_eq!(ctx.dividend.claimable(&id, &ctx.h1), 0);
}

proptest! {
    #[test]
    fn prop_distribution_claims_never_exceed_proportional_share(
        steps in prop::collection::vec(any::<u8>(), 1..16),
    ) {
        let ctx = setup();
        let snapshot = Vec::from_array(
            &ctx.env,
            [
                (ctx.admin.clone(), 500i128),
                (ctx.h1.clone(), 300i128),
                (ctx.h2.clone(), 200i128),
            ],
        );
        let total_amount = 1_000i128;
        let id = ctx.dividend.create_distribution(
            &ctx.admin,
            &ctx.asset_id,
            &ctx.pay_id,
            &total_amount,
            &snapshot,
        );

        let mut received = std::vec![0i128; snapshot.len() as usize];
        for step in steps {
            let idx = (step as usize) % snapshot.len() as usize;
            let (holder, balance) = snapshot.get(idx as u32).unwrap();
            if received[idx] > 0 {
                continue;
            }
            let expected_share = total_amount.checked_mul(balance).unwrap() / 1_000;
            let claimable = ctx.dividend.claimable(&id, &holder);
            if claimable > 0 {
                ctx.dividend.claim(&id, &holder);
                received[idx] = claimable;
                assert!(received[idx] <= expected_share);
            }
        }
    }
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_double_claim_rejected() {
    let ctx = setup();
    let id = ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );
    ctx.dividend.claim(&id, &ctx.h1);
    ctx.dividend.claim(&id, &ctx.h1);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_nonholder_nothing_to_claim() {
    let ctx = setup();
    let id = ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );
    let stranger = Address::generate(&ctx.env);
    ctx.dividend.claim(&id, &stranger);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_create_requires_admin() {
    let ctx = setup();
    let impostor = Address::generate(&ctx.env);
    ctx.dividend.create_distribution(
        &impostor,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &Vec::new(&ctx.env),
    );
}

// ---- issue #165: `claimable` must guard `total_amount * balance` against
// i128 overflow instead of relying on the release profile's overflow-checks
// (which would abort the whole contract). ----
#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_claimable_overflow_guarded() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let comp_id = env.register(ComplianceContract, ());
    let comp = ComplianceContractClient::new(&env, &comp_id);
    comp.initialize(&admin);
    let us = String::from_str(&env, "US");
    let h1 = Address::generate(&env);
    comp.add_to_allowlist(&admin, &admin, &us, &0);
    comp.add_to_allowlist(&admin, &h1, &us, &0);

    // Use amounts large enough that total_amount * balance overflows i128,
    // but each individually fits and the supply is positive.
    let big: i128 = 20_000_000_000_000_000_000; // 2e19
    let asset_id = env.register(AssetTokenContract, ());
    let asset = AssetTokenContractClient::new(&env, &asset_id);
    asset.initialize(
        &admin,
        &String::from_str(&env, "Big"),
        &String::from_str(&env, "BIG"),
        &String::from_str(&env, "real_estate"),
        &big,
        &0u32,
        &comp_id,
        &String::from_str(&env, "desc"),
        &big,
    );
    asset.transfer(&admin, &h1, &big);

    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let pay_id = sac.address();
    token::StellarAssetClient::new(&env, &pay_id).mint(&admin, &big);

    let div_id = env.register(DividendContract, ());
    let dividend = DividendContractClient::new(&env, &div_id);
    dividend.initialize(&admin);
    let mut eligible = Vec::new(&env);
    eligible.push_back((h1.clone(), big));
    dividend.create_distribution(&admin, &asset_id, &pay_id, &big, &eligible);

    // total_amount(2e19) * balance(2e19) overflows i128 -> ArithmeticOverflow (#10)
    let _ = dividend.claimable(&1, &h1);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_zero_amount_rejected() {
    let ctx = setup();
    ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &0,
        &Vec::new(&ctx.env),
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_missing_distribution() {
    let ctx = setup();
    ctx.dividend.get_distribution(&99);
}

#[test]
fn test_get_distributions_for_asset() {
    let ctx = setup();
    ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );
    ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &500,
        &eligible(&ctx),
    );
    let other_asset = Address::generate(&ctx.env);
    assert_eq!(
        ctx.dividend
            .get_distributions_for_asset(&ctx.asset_id)
            .len(),
        2
    );
    assert_eq!(
        ctx.dividend.get_distributions_for_asset(&other_asset).len(),
        0
    );
}

// ---- issue #166: results must be scoped to the requested asset token, not
// derived from a global counter scan. ----
#[test]
fn test_get_distributions_for_asset_scoped_per_asset() {
    let ctx = setup();
    // Register a second, real asset token so distributions can be created for it.
    let other_asset = env_register_asset(&ctx, 1000);
    let mut other_eligible = Vec::new(&ctx.env);
    other_eligible.push_back((ctx.admin.clone(), 1000));
    // 3 distributions for the main asset, 2 for a different asset.
    ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );
    ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &500,
        &eligible(&ctx),
    );
    ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &250,
        &eligible(&ctx),
    );
    ctx.dividend
        .create_distribution(&ctx.admin, &other_asset, &ctx.pay_id, &100, &other_eligible);
    ctx.dividend
        .create_distribution(&ctx.admin, &other_asset, &ctx.pay_id, &100, &other_eligible);

    let main = ctx.dividend.get_distributions_for_asset(&ctx.asset_id);
    assert_eq!(main.len(), 3);
    let other = ctx.dividend.get_distributions_for_asset(&other_asset);
    assert_eq!(other.len(), 2);
    // Every returned distribution actually references the requested asset.
    for d in main.iter() {
        assert_eq!(d.asset_token, ctx.asset_id);
    }
    for d in other.iter() {
        assert_eq!(d.asset_token, other_asset);
    }
}

/// Register a fresh asset token (supply `supply`) under the test compliance
/// allowlist so distributions can be created against it.
fn env_register_asset(ctx: &Ctx, supply: i128) -> Address {
    let asset_id = ctx.env.register(AssetTokenContract, ());
    let asset = AssetTokenContractClient::new(&ctx.env, &asset_id);
    asset.initialize(
        &ctx.admin,
        &String::from_str(&ctx.env, "Oth"),
        &String::from_str(&ctx.env, "OTH"),
        &String::from_str(&ctx.env, "real_estate"),
        &supply,
        &0u32,
        &ctx.comp_id,
        &String::from_str(&ctx.env, "desc"),
        &supply,
    );
    asset_id
}

#[test]
fn test_full_distribution_completes() {
    let ctx = setup();
    let id = ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );
    ctx.dividend.claim(&id, &ctx.h1); // 300
    ctx.dividend.claim(&id, &ctx.h2); // 200
    ctx.dividend.claim(&id, &ctx.admin); // 500
    let d = ctx.dividend.get_distribution(&id);
    assert_eq!(d.distributed, 1000);
    assert!(d.completed);
    assert_eq!(pay_balance(&ctx, &ctx.h2), 200);
    assert_eq!(pay_balance(&ctx, &ctx.admin), 100_000 - 1000 + 500);
}

// ---- issue #49: reject zero-supply asset token ----

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_create_distribution_zero_supply_rejected() {
    let ctx = setup();
    // Register an asset token with supply=0; no holders can ever claim.
    let env = &ctx.env;
    let comp_id = env.register(ComplianceContract, ());
    let comp = ComplianceContractClient::new(env, &comp_id);
    comp.initialize(&ctx.admin);
    comp.add_to_allowlist(&ctx.admin, &ctx.admin, &String::from_str(env, "US"), &0);

    let zero_asset_id = env.register(AssetTokenContract, ());
    let zero_asset = AssetTokenContractClient::new(env, &zero_asset_id);
    zero_asset.initialize(
        &ctx.admin,
        &String::from_str(env, "Empty"),
        &String::from_str(env, "EMPT"),
        &String::from_str(env, "commodity"),
        &0i128, // zero supply
        &0u32,
        &comp_id,
        &String::from_str(env, "empty asset"),
        &0i128,
    );

    ctx.dividend.create_distribution(
        &ctx.admin,
        &zero_asset_id,
        &ctx.pay_id,
        &1000,
        &Vec::new(&ctx.env),
    );
}

// ---- issue #120: cross-contract auth propagation ----

#[test]
fn test_create_distribution_auth_tree() {
    let ctx = setup();
    ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );

    let auths = ctx.env.auths();
    assert_eq!(auths.len(), 1);
    let (authorizer, invocation) = &auths[0];
    assert_eq!(*authorizer, ctx.admin);
    match &invocation.function {
        AuthorizedFunction::Contract((contract, fn_name, _)) => {
            assert_eq!(*contract, ctx.dividend.address);
            assert_eq!(*fn_name, Symbol::new(&ctx.env, "create_distribution"));
        }
        _ => panic!("expected a contract invocation"),
    }
    // Escrowing the payment token is a cross-contract call made `from` the
    // admin, so it must appear as a sub-invocation authorized by the admin.
    assert_eq!(invocation.sub_invocations.len(), 1);
    match &invocation.sub_invocations[0].function {
        AuthorizedFunction::Contract((contract, fn_name, _)) => {
            assert_eq!(*contract, ctx.pay_id);
            assert_eq!(*fn_name, Symbol::new(&ctx.env, "transfer"));
        }
        _ => panic!("expected a contract invocation"),
    }
}

#[test]
fn test_claim_requires_only_holder_auth() {
    let ctx = setup();
    let id = ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );

    ctx.dividend.claim(&id, &ctx.h1);

    let auths = ctx.env.auths();
    assert_eq!(auths.len(), 1);
    let (authorizer, invocation) = &auths[0];
    assert_eq!(*authorizer, ctx.h1);
    match &invocation.function {
        AuthorizedFunction::Contract((contract, fn_name, _)) => {
            assert_eq!(*contract, ctx.dividend.address);
            assert_eq!(*fn_name, Symbol::new(&ctx.env, "claim"));
        }
        _ => panic!("expected a contract invocation"),
    }
    // The payout transfer moves funds `from` the dividend contract's own
    // escrow, so it is self-authorized and needs no separate sub-invocation.
    assert_eq!(invocation.sub_invocations.len(), 0);
}

// ---- issue #163: holders cannot claim twice by moving tokens to a new wallet ----

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_token_move_to_fresh_wallet_cannot_claim() {
    let ctx = setup();
    let id = ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );
    // h1 (snapshot 300) claims its share.
    ctx.dividend.claim(&id, &ctx.h1);
    assert_eq!(pay_balance(&ctx, &ctx.h1), 300);

    // A brand-new wallet receives 300 tokens from the admin. It is
    // compliance-approved so the transfer succeeds, and it now holds tokens, but
    // it is NOT in the distribution snapshot...
    let comp = ComplianceContractClient::new(&ctx.env, &ctx.comp_id);
    let w = Address::generate(&ctx.env);
    comp.add_to_allowlist(&ctx.admin, &w, &String::from_str(&ctx.env, "US"), &0);
    let asset = AssetTokenContractClient::new(&ctx.env, &ctx.asset_id);
    asset.transfer(&ctx.admin, &w, &300);

    // ...so its entitlement basis is 0 and it cannot claim (fixes #163).
    ctx.dividend.claim(&id, &w);
}

#[test]
fn test_snapshot_basis_is_immutable_after_transfer() {
    let ctx = setup();
    let id = ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );
    // h2 is snapshotted at 200. Even after receiving more tokens, the payout
    // stays at the frozen 200 (not the inflated live balance).
    let asset = AssetTokenContractClient::new(&ctx.env, &ctx.asset_id);
    asset.transfer(&ctx.admin, &ctx.h2, &300);
    assert_eq!(ctx.dividend.claimable(&id, &ctx.h2), 200);
    ctx.dividend.claim(&id, &ctx.h2);
    assert_eq!(pay_balance(&ctx, &ctx.h2), 200);
    assert_eq!(ctx.dividend.claimable(&id, &ctx.h2), 0);
}

// ---- issue #215: claim on a non-existent distribution id fails DistributionNotFound ----

#[test]
fn test_claim_on_nonexistent_distribution_fails() {
    let ctx = setup();
    // No distribution has been created yet, so the counter is still 0 —
    // both an id above the counter and id 0 itself must fail the same way.
    let above_counter = ctx.dividend.try_claim(&99, &ctx.h1);
    assert_eq!(above_counter, Err(Ok(Error::DistributionNotFound.into())));

    let zero_id = ctx.dividend.try_claim(&0, &ctx.h1);
    assert_eq!(zero_id, Err(Ok(Error::DistributionNotFound.into())));
}

// ---- issue #216: has_claimed flips from false to true exactly once ----

#[test]
fn test_has_claimed_flips_once_and_second_claim_moves_no_funds() {
    let ctx = setup();
    let id = ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );

    assert!(!ctx.dividend.has_claimed(&id, &ctx.h1));

    ctx.dividend.claim(&id, &ctx.h1);
    assert!(ctx.dividend.has_claimed(&id, &ctx.h1));

    let balance_after_first_claim = pay_balance(&ctx, &ctx.h1);
    let div_addr = ctx.dividend.address.clone();
    let escrow_after_first_claim = pay_balance(&ctx, &div_addr);

    let result = ctx.dividend.try_claim(&id, &ctx.h1);
    assert_eq!(result, Err(Ok(Error::AlreadyClaimed.into())));

    // Still claimed, and no additional funds moved on the rejected second claim.
    assert!(ctx.dividend.has_claimed(&id, &ctx.h1));
    assert_eq!(pay_balance(&ctx, &ctx.h1), balance_after_first_claim);
    assert_eq!(pay_balance(&ctx, &div_addr), escrow_after_first_claim);
}

// ---- issue #217: create_distribution rejects a zero or negative amount ----

#[test]
fn test_create_distribution_rejects_zero_and_negative_amount() {
    let ctx = setup();
    let admin_before = pay_balance(&ctx, &ctx.admin);
    let div_addr = ctx.dividend.address.clone();

    let zero_result = ctx.dividend.try_create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &0,
        &eligible(&ctx),
    );
    assert_eq!(zero_result, Err(Ok(Error::InvalidAmount.into())));

    let negative_result = ctx.dividend.try_create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &-1,
        &eligible(&ctx),
    );
    assert_eq!(negative_result, Err(Ok(Error::InvalidAmount.into())));

    // Neither rejected call should have moved any payment-token funds.
    assert_eq!(pay_balance(&ctx, &ctx.admin), admin_before);
    assert_eq!(pay_balance(&ctx, &div_addr), 0);
}

// ---- issue #218: distributions for one asset are not returned for another ----

#[test]
fn test_distributions_for_one_asset_excluded_from_another() {
    let ctx = setup();
    let other_asset = env_register_asset(&ctx, 1000);
    let mut other_eligible = Vec::new(&ctx.env);
    other_eligible.push_back((ctx.admin.clone(), 1000));

    let main_id = ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );
    let other_id = ctx.dividend.create_distribution(
        &ctx.admin,
        &other_asset,
        &ctx.pay_id,
        &100,
        &other_eligible,
    );

    let main_list = ctx.dividend.get_distributions_for_asset(&ctx.asset_id);
    let other_list = ctx.dividend.get_distributions_for_asset(&other_asset);

    assert_eq!(main_list.len(), 1);
    assert_eq!(main_list.get(0).unwrap().id, main_id);
    assert_eq!(other_list.len(), 1);
    assert_eq!(other_list.get(0).unwrap().id, other_id);

    // Cross-check: the other asset's distribution id never shows up in the
    // main asset's list, and vice versa.
    assert!(!main_list.iter().any(|d| d.id == other_id));
    assert!(!other_list.iter().any(|d| d.id == main_id));
}

// ---- issue #290: a duplicate address in `eligible` would be counted in the
// denominator but never be claimable, stranding that slice of the escrow.
// Creation must reject the list outright (Error #11, DuplicateHolder). ----

#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn test_create_distribution_rejects_duplicate_holder() {
    let ctx = setup();

    // h1 appears twice (300 + 100). Summed, snapshot_supply would be 600, but
    // `snapshot_balance` only ever returns the first h1 entry (300), so the
    // extra 100 is in the denominator yet unclaimable — stranded escrow.
    let mut dup = Vec::new(&ctx.env);
    dup.push_back((ctx.h1.clone(), 300i128));
    dup.push_back((ctx.h2.clone(), 200i128));
    dup.push_back((ctx.h1.clone(), 100i128));

    ctx.dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &dup);
}

// A duplicate that is not adjacent to its twin is still caught (the check is
// all-pairs, not just neighbours).
#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn test_create_distribution_rejects_non_adjacent_duplicate_holder() {
    let ctx = setup();
    let mut dup = Vec::new(&ctx.env);
    dup.push_back((ctx.h1.clone(), 300i128));
    dup.push_back((ctx.h2.clone(), 200i128));
    dup.push_back((ctx.admin.clone(), 500i128));
    dup.push_back((ctx.h1.clone(), 1i128));

    ctx.dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &dup);
}

// ---- issue #291: a negative entry in `eligible` lowers the shared denominator
// and silently inflates every other holder's payout. Reject it at creation
// (Error #5, InvalidAmount). ----

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_create_distribution_rejects_negative_balance() {
    let ctx = setup();

    let mut neg = Vec::new(&ctx.env);
    neg.push_back((ctx.h1.clone(), 300i128));
    neg.push_back((ctx.h2.clone(), -50i128));
    neg.push_back((ctx.admin.clone(), 500i128));

    ctx.dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &neg);
}

// A negative entry in the very first slot is rejected too (loop covers index 0).
#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_create_distribution_rejects_negative_balance_first_entry() {
    let ctx = setup();
    let mut neg = Vec::new(&ctx.env);
    neg.push_back((ctx.h1.clone(), -1i128));
    neg.push_back((ctx.h2.clone(), 200i128));

    ctx.dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &neg);
}

// A zero balance is a valid (if pointless) entry — it must not be rejected as
// "negative", and it contributes nothing to the denominator.
#[test]
fn test_create_distribution_allows_zero_balance_entry() {
    let ctx = setup();
    let mut v = Vec::new(&ctx.env);
    v.push_back((ctx.h1.clone(), 300i128));
    v.push_back((ctx.h2.clone(), 0i128));
    v.push_back((ctx.admin.clone(), 500i128));

    let id = ctx
        .dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &800, &v);
    // Denominator is 800 (300 + 0 + 500); h1 gets 300/800 * 800 = 300.
    assert_eq!(ctx.dividend.claimable(&id, &ctx.h1), 300);
    assert_eq!(ctx.dividend.claimable(&id, &ctx.h2), 0);
    assert_eq!(ctx.dividend.claimable(&id, &ctx.admin), 500);
}

// ---- admin handover (issue #4) ----

#[test]
fn test_propose_accept_admin_moves_role_only_on_acceptance() {
    let ctx = setup();
    let successor = Address::generate(&ctx.env);

    ctx.dividend.propose_admin(&ctx.admin, &successor);
    // Role must not move until accepted.
    assert_eq!(ctx.dividend.get_admin(), ctx.admin);
    assert_eq!(ctx.dividend.get_pending_admin(), Some(successor.clone()));

    ctx.dividend.accept_admin(&successor);
    assert_eq!(ctx.dividend.get_admin(), successor);
    assert_eq!(ctx.dividend.get_pending_admin(), None);

    // The old admin has lost its privileges.
    let mut v = Vec::new(&ctx.env);
    v.push_back((ctx.h1.clone(), 300i128));
    let res =
        ctx.dividend
            .try_create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &300, &v);
    assert_eq!(res, Err(Ok(Error::Unauthorized.into())));
}

#[test]
fn test_cancel_admin_proposal_by_current_admin() {
    let ctx = setup();
    let successor = Address::generate(&ctx.env);

    ctx.dividend.propose_admin(&ctx.admin, &successor);
    ctx.dividend.cancel_admin_proposal(&ctx.admin);
    assert_eq!(ctx.dividend.get_pending_admin(), None);

    // The cancelled successor can no longer accept.
    let res = ctx.dividend.try_accept_admin(&successor);
    assert_eq!(res, Err(Ok(Error::NoPendingAdmin.into())));
    // The admin is unchanged.
    assert_eq!(ctx.dividend.get_admin(), ctx.admin);
}

#[test]
fn test_cancel_admin_proposal_with_nothing_pending_fails() {
    let ctx = setup();
    let res = ctx.dividend.try_cancel_admin_proposal(&ctx.admin);
    assert_eq!(res, Err(Ok(Error::NoPendingAdmin.into())));
}

#[test]
fn test_non_admin_cannot_propose_admin() {
    let ctx = setup();
    let non_admin = Address::generate(&ctx.env);
    let successor = Address::generate(&ctx.env);
    let res = ctx.dividend.try_propose_admin(&non_admin, &successor);
    assert_eq!(res, Err(Ok(Error::Unauthorized.into())));
}

#[test]
fn test_non_admin_cannot_cancel_admin_proposal() {
    let ctx = setup();
    let non_admin = Address::generate(&ctx.env);
    let successor = Address::generate(&ctx.env);
    ctx.dividend.propose_admin(&ctx.admin, &successor);
    let res = ctx.dividend.try_cancel_admin_proposal(&non_admin);
    assert_eq!(res, Err(Ok(Error::Unauthorized.into())));
}

#[test]
fn test_only_proposed_successor_can_accept() {
    let ctx = setup();
    let successor = Address::generate(&ctx.env);
    let impostor = Address::generate(&ctx.env);

    ctx.dividend.propose_admin(&ctx.admin, &successor);
    let res = ctx.dividend.try_accept_admin(&impostor);
    assert_eq!(res, Err(Ok(Error::Unauthorized.into())));
    // Role is unaffected by the failed attempt.
    assert_eq!(ctx.dividend.get_admin(), ctx.admin);
}

#[test]
fn test_accept_admin_with_no_pending_proposal_fails() {
    let ctx = setup();
    let stranger = Address::generate(&ctx.env);
    let res = ctx.dividend.try_accept_admin(&stranger);
    assert_eq!(res, Err(Ok(Error::NoPendingAdmin.into())));
}

// Issue #377: Property test that claim totals never exceed the pool.
proptest! {
    #[test]
    fn prop_total_claims_never_exceed_pool(
        // Bounded by the 100_000 payment tokens `setup()` mints to admin —
        // create_distribution escrows `total_amount` from admin's balance, so
        // a larger value would fail on insufficient balance rather than
        // exercising the invariant under test.
        total_amount in 1i128..100_000i128,
        h1_balance in 0i128..1_000_000i128,
        h2_balance in 0i128..1_000_000i128,
        admin_balance in 0i128..1_000_000i128,
    ) {
        let ctx = setup();
        let supply = h1_balance + h2_balance + admin_balance;
        // Skip zero-supply distributions; there's nothing to assert about them.
        if supply != 0 {
            let eligible = Vec::from_array(
                &ctx.env,
                [
                    (ctx.admin.clone(), admin_balance),
                    (ctx.h1.clone(), h1_balance),
                    (ctx.h2.clone(), h2_balance),
                ],
            );

            let id = ctx.dividend.create_distribution(
                &ctx.admin,
                &ctx.asset_id,
                &ctx.pay_id,
                &total_amount,
                &eligible,
            );

            let mut total_claimed = 0i128;

            // Try to claim for each holder
            for (holder, _) in eligible.iter() {
                if let Ok(_) = std::panic::catch_unwind(
                    std::panic::AssertUnwindSafe(|| {
                        let claimable = ctx.dividend.claimable(&id, &holder);
                        if claimable > 0 {
                            ctx.dividend.claim(&id, &holder);
                            total_claimed = total_claimed.saturating_add(claimable);
                        }
                    })
                ) {}
            }

            prop_assert!(
                total_claimed <= total_amount,
                "Total claims {} must not exceed pool {}",
                total_claimed,
                total_amount
            );
        }
    }
}

// Issue #365: Reject distributions with an empty eligible list.
#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_create_distribution_rejects_empty_eligible_list() {
    let ctx = setup();
    let empty = Vec::new(&ctx.env);
    ctx.dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &empty);
}

// Issue #366: Allow cancelling a distribution before any claim is made.
#[test]
fn test_cancel_distribution_before_claims() {
    let ctx = setup();
    let div_addr = ctx.dividend.address.clone();
    let id = ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );
    assert_eq!(pay_balance(&ctx, &div_addr), 1000);

    // Cancel the distribution; funds should return to admin.
    ctx.dividend.cancel_distribution(&ctx.admin, &id);
    assert_eq!(pay_balance(&ctx, &div_addr), 0);
    assert_eq!(pay_balance(&ctx, &ctx.admin), 100_000);
}

// Issue #366: Reject cancellation after any claim is made.
#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_cancel_distribution_after_claim_fails() {
    let ctx = setup();
    let id = ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );
    ctx.dividend.claim(&id, &ctx.h1);

    // Attempt to cancel after a claim has been made; should fail.
    ctx.dividend.cancel_distribution(&ctx.admin, &id);
}

// Issue #367: Test claiming against a distribution whose payment token has
// unusual (different) decimals from the asset token.
#[test]
fn test_claim_with_unusual_payment_token_decimals() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);

    // Compliance with admin + two holders approved.
    let comp_id = env.register(ComplianceContract, ());
    let comp = ComplianceContractClient::new(&env, &comp_id);
    comp.initialize(&admin);
    let us = String::from_str(&env, "US");
    let h1 = Address::generate(&env);
    comp.add_to_allowlist(&admin, &admin, &us, &0);
    comp.add_to_allowlist(&admin, &h1, &us, &0);

    // Asset token: supply 1000 with 7 decimals.
    let asset_id = env.register(AssetTokenContract, ());
    let asset = AssetTokenContractClient::new(&env, &asset_id);
    asset.initialize(
        &admin,
        &String::from_str(&env, "Loft"),
        &String::from_str(&env, "LFT"),
        &String::from_str(&env, "real_estate"),
        &1000i128,
        &7u32,
        &comp_id,
        &String::from_str(&env, "desc"),
        &1000i128,
    );
    asset.transfer(&admin, &h1, &600);

    // Payment token (SAC) with 2 decimals (e.g., USDC variant).
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let pay_id = sac.address();
    token::StellarAssetClient::new(&env, &pay_id).mint(&admin, &10_000);

    let div_id = env.register(DividendContract, ());
    let dividend = DividendContractClient::new(&env, &div_id);
    dividend.initialize(&admin);

    // Create distribution: 10_000 payment tokens for snapshot: admin=400, h1=600.
    let mut eligible = Vec::new(&env);
    eligible.push_back((admin.clone(), 400i128));
    eligible.push_back((h1.clone(), 600i128));
    let dist_id = dividend.create_distribution(&admin, &asset_id, &pay_id, &10_000, &eligible);

    // h1 should get 600/1000 * 10_000 = 6_000 (even with decimal differences).
    assert_eq!(dividend.claimable(&dist_id, &h1), 6_000);
    dividend.claim(&dist_id, &h1);
    assert_eq!(token::TokenClient::new(&env, &pay_id).balance(&h1), 6_000);
}

fn set_ledger_sequence(env: &Env, seq: u32) {
    env.ledger().with_mut(|li| li.sequence_number = seq);
}

// Claiming after the deadline is rejected, even for a holder who never
// claimed before.
#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn test_claim_after_deadline_fails() {
    let ctx = setup();
    let deadline = ctx.env.ledger().sequence() + 100;
    let id = ctx.dividend.create_distribution_deadline(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
        &deadline,
    );
    set_ledger_sequence(&ctx.env, deadline + 1);
    ctx.dividend.claim(&id, &ctx.h1);
}

// Claiming before the deadline succeeds normally.
#[test]
fn test_claim_before_deadline_succeeds() {
    let ctx = setup();
    let deadline = ctx.env.ledger().sequence() + 100;
    let id = ctx.dividend.create_distribution_deadline(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
        &deadline,
    );
    ctx.dividend.claim(&id, &ctx.h1);
    assert_eq!(pay_balance(&ctx, &ctx.h1), 300);
}

// A second claim by the same holder must fail with AlreadyClaimed (#7), and
// must not move any additional funds or mutate `distributed` a second time.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_claim_twice_by_same_holder_fails() {
    let ctx = setup();
    let id = ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );
    ctx.dividend.claim(&id, &ctx.h1);
    assert!(ctx.dividend.has_claimed(&id, &ctx.h1));
    // Second claim by the same holder must be rejected by the guard.
    ctx.dividend.claim(&id, &ctx.h1);
}

// The guard is scoped per (distribution, holder): a holder claiming should
// never block or consume another holder's entitlement, whether claims happen
// interleaved (h1, h2, h1-again-fails, admin) or in any other order.
#[test]
fn test_interleaved_claims_by_different_holders_all_succeed() {
    let ctx = setup();
    let id = ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );

    // h1 claims first.
    ctx.dividend.claim(&id, &ctx.h1);
    assert_eq!(pay_balance(&ctx, &ctx.h1), 300);
    assert!(ctx.dividend.has_claimed(&id, &ctx.h1));
    assert!(!ctx.dividend.has_claimed(&id, &ctx.h2));

    // h2 claims next, interleaved with h1 already having claimed. h1's flag
    // and payout must be unaffected.
    ctx.dividend.claim(&id, &ctx.h2);
    assert_eq!(pay_balance(&ctx, &ctx.h2), 200);
    assert!(ctx.dividend.has_claimed(&id, &ctx.h2));

    // admin claims last; all three succeed independently and the running
    // `distributed` total reflects exactly the sum of the three payouts.
    // (admin started with 100_000 payment tokens and escrowed 1000.)
    ctx.dividend.claim(&id, &ctx.admin);
    assert_eq!(pay_balance(&ctx, &ctx.admin), 100_000 - 1000 + 500);

    let d = ctx.dividend.get_distribution(&id);
    assert_eq!(d.distributed, 1000);
    assert!(d.completed);

    // Every holder is now flagged claimed, and none can claim again.
    assert!(ctx.dividend.has_claimed(&id, &ctx.h1));
    assert!(ctx.dividend.has_claimed(&id, &ctx.h2));
    assert!(ctx.dividend.has_claimed(&id, &ctx.admin));
}

// Reclaiming before the deadline must fail.
#[test]
#[should_panic(expected = "Error(Contract, #14)")]
fn test_reclaim_before_deadline_fails() {
    let ctx = setup();
    let deadline = ctx.env.ledger().sequence() + 100;
    let id = ctx.dividend.create_distribution_deadline(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
        &deadline,
    );
    ctx.dividend.reclaim_unclaimed(&ctx.admin, &id);
}

// Once the deadline has passed, the admin can reclaim whatever was never
// claimed, and the distribution is marked completed.
#[test]
fn test_reclaim_unclaimed_after_deadline() {
    let ctx = setup();
    let deadline = ctx.env.ledger().sequence() + 100;
    let id = ctx.dividend.create_distribution_deadline(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
        &deadline,
    );
    // h1 claims their share before the deadline; h2 and admin never claim.
    ctx.dividend.claim(&id, &ctx.h1);

    set_ledger_sequence(&ctx.env, deadline + 1);
    let admin_before = pay_balance(&ctx, &ctx.admin);
    let reclaimed = ctx.dividend.reclaim_unclaimed(&ctx.admin, &id);
    assert_eq!(reclaimed, 700); // 1000 - 300 claimed by h1
    assert_eq!(pay_balance(&ctx, &ctx.admin), admin_before + 700);

    let d = ctx.dividend.get_distribution(&id);
    assert!(d.completed);
    assert_eq!(d.distributed, d.total_amount);
}

// A distribution with no deadline (the default) can never be reclaimed.
#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_reclaim_without_deadline_fails() {
    let ctx = setup();
    let id = ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &eligible(&ctx),
    );
    set_ledger_sequence(&ctx.env, ctx.env.ledger().sequence() + 1_000_000);
    ctx.dividend.reclaim_unclaimed(&ctx.admin, &id);
}

// A distribution that does not divide evenly among its 3 holders leaves
// dust permanently locked in escrow. Worst-case dust for N=3 holders is
// N-1 = 2 units (see the module doc on `claimable`); this picks amounts
// designed to hit that bound.
#[test]
fn test_uneven_distribution_leaves_dust() {
    let ctx = setup();
    let div_addr = ctx.dividend.address.clone();
    // supply = 1000 (300 + 200 + 500), total_amount = 1001 does not divide
    // evenly by any of the three balances.
    let total_amount = 1001i128;
    let id = ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &total_amount,
        &eligible(&ctx),
    );

    let c_h1 = ctx.dividend.claimable(&id, &ctx.h1); // floor(1001*300/1000) = 300
    let c_h2 = ctx.dividend.claimable(&id, &ctx.h2); // floor(1001*200/1000) = 200
    let c_admin = ctx.dividend.claimable(&id, &ctx.admin); // floor(1001*500/1000) = 500
    assert_eq!(c_h1, 300);
    assert_eq!(c_h2, 200);
    assert_eq!(c_admin, 500);

    ctx.dividend.claim(&id, &ctx.h1);
    ctx.dividend.claim(&id, &ctx.h2);
    ctx.dividend.claim(&id, &ctx.admin);

    let d = ctx.dividend.get_distribution(&id);
    let dust = total_amount - d.distributed;
    // dust must obey the documented bound: 0 <= dust <= N - 1 (N=3 holders).
    assert!(dust >= 0 && dust <= 2);
    assert_eq!(dust, 1); // 1001 - (300 + 200 + 500)

    // The dust remains stranded in the contract's escrow: nothing further
    // can be claimed (distribution is exhausted / no deadline to reclaim).
    assert_eq!(pay_balance(&ctx, &div_addr), dust);
    assert!(!d.completed); // distributed (1000) < total_amount (1001)
}
