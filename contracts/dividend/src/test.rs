#![cfg(test)]
use super::*;
use asset_token::{AssetTokenContract, AssetTokenContractClient};
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction},
    token, Address, Env, String, Symbol,
};

struct Ctx {
    env: Env,
    dividend: DividendContractClient<'static>,
    asset_id: Address,
    pay_id: Address,
    admin: Address,
    h1: Address,
    h2: Address,
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
    }
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
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000);
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
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000);
    // h1 holds 300/1000 -> 300; h2 holds 200/1000 -> 200.
    assert_eq!(ctx.dividend.claimable(&id, &ctx.h1), 300);
    assert_eq!(ctx.dividend.claimable(&id, &ctx.h2), 200);
    assert_eq!(ctx.dividend.claimable(&id, &ctx.admin), 500);

    ctx.dividend.claim(&id, &ctx.h1);
    assert_eq!(pay_balance(&ctx, &ctx.h1), 300);
    assert_eq!(ctx.dividend.claimable(&id, &ctx.h1), 0);
    assert_eq!(ctx.dividend.get_distribution(&id).distributed, 300);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_double_claim_rejected() {
    let ctx = setup();
    let id = ctx
        .dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000);
    ctx.dividend.claim(&id, &ctx.h1);
    ctx.dividend.claim(&id, &ctx.h1);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_nonholder_nothing_to_claim() {
    let ctx = setup();
    let id = ctx
        .dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000);
    let stranger = Address::generate(&ctx.env);
    ctx.dividend.claim(&id, &stranger);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_create_requires_admin() {
    let ctx = setup();
    let impostor = Address::generate(&ctx.env);
    ctx.dividend
        .create_distribution(&impostor, &ctx.asset_id, &ctx.pay_id, &1000);
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
    dividend.create_distribution(&admin, &asset_id, &pay_id, &big);

    // total_amount(2e19) * balance(2e19) overflows i128 -> ArithmeticOverflow (#10)
    let _ = dividend.claimable(&1, &h1);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_zero_amount_rejected() {
    let ctx = setup();
    ctx.dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &0);
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
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000);
    ctx.dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &500);
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

#[test]
fn test_full_distribution_completes() {
    let ctx = setup();
    let id = ctx
        .dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000);
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
        .create_distribution(&ctx.admin, &zero_asset_id, &ctx.pay_id, &1000);
}

// ---- issue #120: cross-contract auth propagation ----

#[test]
fn test_create_distribution_auth_tree() {
    let ctx = setup();
    ctx.dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000);

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
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000);

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
