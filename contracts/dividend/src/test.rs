#![cfg(test)]
use super::*;
use asset_token::{AssetTokenContract, AssetTokenContractClient};
use compliance::{ComplianceContract, ComplianceContractClient};
use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction},
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
fn test_version() {
    let ctx = setup();
    assert_eq!(ctx.dividend.version(), VERSION);
}

#[test]
fn test_create_distribution_escrows_funds() {
    let ctx = setup();
    let div_addr = ctx.dividend.address.clone();
    let id = ctx
        .dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &eligible(&ctx));
    assert_eq!(id, 1);
    assert_eq!(pay_balance(&ctx, &div_addr), 1000);
    let d = ctx.dividend.get_distribution(&id);
    assert_eq!(d.total_amount, 1000);
    assert_eq!(d.distributed, 0);
    assert!(!d.completed);
}

#[test]
fn test_claim_is_proportional() {
    let ctx = setup();
    let id = ctx
        .dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &eligible(&ctx));
    // h1 holds 300/1000 -> 300; h2 holds 200/1000 -> 200.
    assert_eq!(ctx.dividend.claimable(&id, &ctx.h1), 300);
    assert_eq!(ctx.dividend.claimable(&id, &ctx.h2), 200);
    assert_eq!(ctx.dividend.claimable(&id, &ctx.admin), 500);

    ctx.dividend.claim(&id, &ctx.h1);
    assert_eq!(pay_balance(&ctx, &ctx.h1), 300);
    assert_eq!(ctx.dividend.claimable(&id, &ctx.h1), 0);
    assert_eq!(ctx.dividend.get_distribution(&id).distributed, 300);
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

        let mut received = vec![0i128; snapshot.len() as usize];
        for step in steps {
            let idx = (step as usize) % snapshot.len() as usize;
            let (holder, balance) = snapshot.get(idx as u32).unwrap();
            if received[idx] > 0 {
                continue;
            }
            let expected_share = total_amount.checked_mul(*balance).unwrap() / 1_000;
            let claimable = ctx.dividend.claimable(&id, holder);
            if claimable > 0 {
                ctx.dividend.claim(&id, holder);
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
    let id = ctx
        .dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &eligible(&ctx));
    ctx.dividend.claim(&id, &ctx.h1);
    ctx.dividend.claim(&id, &ctx.h1);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_nonholder_nothing_to_claim() {
    let ctx = setup();
    let id = ctx
        .dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &eligible(&ctx));
    let stranger = Address::generate(&ctx.env);
    ctx.dividend.claim(&id, &stranger);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_create_requires_admin() {
    let ctx = setup();
    let impostor = Address::generate(&ctx.env);
    ctx.dividend
        .create_distribution(&impostor, &ctx.asset_id, &ctx.pay_id, &1000, &Vec::new(&ctx.env));
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
    ctx.dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &0, &Vec::new(&ctx.env));
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
    ctx.dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &eligible(&ctx));
    ctx.dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &500, &eligible(&ctx));
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
    ctx.dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &eligible(&ctx));
    ctx.dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &500, &eligible(&ctx));
    ctx.dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &250, &eligible(&ctx));
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
    let id = ctx
        .dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &eligible(&ctx));
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

    ctx.dividend
        .create_distribution(&ctx.admin, &zero_asset_id, &ctx.pay_id, &1000, &Vec::new(&ctx.env));
}

// ---- issue #120: cross-contract auth propagation ----

#[test]
fn test_create_distribution_auth_tree() {
    let ctx = setup();
    ctx.dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &eligible(&ctx));

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
    let id = ctx
        .dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &eligible(&ctx));

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
    assert_eq!(above_counter, Err(Ok(Error::DistributionNotFound)));

    let zero_id = ctx.dividend.try_claim(&0, &ctx.h1);
    assert_eq!(zero_id, Err(Ok(Error::DistributionNotFound)));
}

// ---- issue #216: has_claimed flips from false to true exactly once ----

#[test]
fn test_has_claimed_flips_once_and_second_claim_moves_no_funds() {
    let ctx = setup();
    let id = ctx
        .dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &eligible(&ctx));

    assert!(!ctx.dividend.has_claimed(&id, &ctx.h1));

    ctx.dividend.claim(&id, &ctx.h1);
    assert!(ctx.dividend.has_claimed(&id, &ctx.h1));

    let balance_after_first_claim = pay_balance(&ctx, &ctx.h1);
    let div_addr = ctx.dividend.address.clone();
    let escrow_after_first_claim = pay_balance(&ctx, &div_addr);

    let result = ctx.dividend.try_claim(&id, &ctx.h1);
    assert_eq!(result, Err(Ok(Error::AlreadyClaimed)));

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
    assert_eq!(zero_result, Err(Ok(Error::InvalidAmount)));

    let negative_result = ctx.dividend.try_create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &-1,
        &eligible(&ctx),
    );
    assert_eq!(negative_result, Err(Ok(Error::InvalidAmount)));

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

    let main_id = ctx
        .dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &eligible(&ctx));
    let other_id =
        ctx.dividend
            .create_distribution(&ctx.admin, &other_asset, &ctx.pay_id, &100, &other_eligible);

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
