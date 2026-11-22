//! Workspace-level integration test exercising the full RWA lifecycle across
//! all four contracts: registry, compliance, asset-token, and dividend.
//!
//! Flow:
//!   1. Initialize the registry and register an asset.
//!   2. Initialize the compliance contract and allowlist/KYC-approve holders.
//!   3. Initialize the asset-token referencing the compliance contract and
//!      mint/transfer tokens between approved holders.
//!   4. Initialize the dividend contract, create a distribution over the
//!      holder snapshot, and have each holder claim.
//!
//! Assertions are made on balances, registry entries, compliance status, and
//! dividend claim state throughout.

use soroban_sdk::{testutils::Address as _, Address, Env, String};

// The four contracts under test. These are imported from their respective
// crates. Adjust the crate names / module paths to match the workspace layout.
use asset_token::{AssetToken, AssetTokenClient};
use compliance::{Compliance, ComplianceClient};
use dividend::{Dividend, DividendClient};
use registry::{Registry, RegistryClient};

/// Helper: deploy and initialize all four contracts, returning their clients
/// and the relevant addresses.
struct Fixture<'a> {
    env: Env,
    registry: RegistryClient<'a>,
    compliance: ComplianceClient<'a>,
    asset_token: AssetTokenClient<'a>,
    dividend: DividendClient<'a>,
    admin: Address,
    issuer: Address,
    holder_a: Address,
    holder_b: Address,
    holder_c: Address,
}

fn setup<'a>() -> Fixture<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let holder_a = Address::generate(&env);
    let holder_b = Address::generate(&env);
    let holder_c = Address::generate(&env);

    // --- Registry ---
    let registry_id = env.register_contract(None, Registry);
    let registry = RegistryClient::new(&env, &registry_id);
    registry.initialize(&admin);

    // --- Compliance ---
    let compliance_id = env.register_contract(None, Compliance);
    let compliance = ComplianceClient::new(&env, &compliance_id);
    compliance.initialize(&admin);

    // --- Asset Token (references compliance) ---
    let asset_token_id = env.register_contract(None, AssetToken);
    let asset_token = AssetTokenClient::new(&env, &asset_token_id);
    asset_token.initialize(
        &admin,
        &String::from_str(&env, "RWA Token"),
        &String::from_str(&env, "RWA"),
        &7u32,
        &compliance_id,
    );

    // --- Dividend ---
    let dividend_id = env.register_contract(None, Dividend);
    let dividend = DividendClient::new(&env, &dividend_id);
    dividend.initialize(&admin, &asset_token_id);

    Fixture {
        env,
        registry,
        compliance,
        asset_token,
        dividend,
        admin,
        issuer,
        holder_a,
        holder_b,
        holder_c,
    }
}

#[test]
fn full_rwa_lifecycle() {
    let f = setup();

    // ------------------------------------------------------------------
    // 1. Registry: register an asset.
    // ------------------------------------------------------------------
    let asset_id = f.registry.register_asset(
        &f.issuer,
        &String::from_str(&f.env, "Real Estate Fund I"),
        &String::from_str(&f.env, "REF-I"),
    );

    // Registry entry exists and is owned by the issuer.
    let entry = f.registry.get_asset(&asset_id);
    assert_eq!(entry.owner, f.issuer);
    assert_eq!(entry.symbol, String::from_str(&f.env, "REF-I"));
    assert!(f.registry.is_registered(&asset_id));

    // ------------------------------------------------------------------
    // 2. Compliance: allowlist / KYC-approve holders.
    // ------------------------------------------------------------------
    f.compliance.approve(&f.admin, &f.holder_a);
    f.compliance.approve(&f.admin, &f.holder_b);
    f.compliance.approve(&f.admin, &f.holder_c);

    assert!(f.compliance.is_approved(&f.holder_a));
    assert!(f.compliance.is_approved(&f.holder_b));
    assert!(f.compliance.is_approved(&f.holder_c));

    // An un-approved address should not pass compliance.
    let stranger = Address::generate(&f.env);
    assert!(!f.compliance.is_approved(&stranger));

    // ------------------------------------------------------------------
    // 3. Asset Token: mint to approved holders and transfer between them.
    // ------------------------------------------------------------------
    // Mint initial supply to holder_a (approved).
    f.asset_token.mint(&f.admin, &f.holder_a, &1_000i128);
    assert_eq!(f.asset_token.balance(&f.holder_a), 1_000i128);

    // Mint to holder_b.
    f.asset_token.mint(&f.admin, &f.holder_b, &500i128);
    assert_eq!(f.asset_token.balance(&f.holder_b), 500i128);

    // Transfer from holder_a to holder_c (both approved).
    f.asset_token
        .transfer(&f.holder_a, &f.holder_c, &250i128);
    assert_eq!(f.asset_token.balance(&f.holder_a), 750i128);
    assert_eq!(f.asset_token.balance(&f.holder_c), 250i128);

    // Total supply reflects all mints.
    assert_eq!(f.asset_token.total_supply(), 1_500i128);

    // ------------------------------------------------------------------
    // 4. Dividend: create a distribution and have holders claim.
    // ------------------------------------------------------------------
    // Snapshot holders and create a distribution of 1_500 tokens (1:1 with
    // total supply) so each holder receives a proportional share.
    let distribution_id = f.dividend.create_distribution(
        &f.admin,
        &1_500i128,
        &f.asset_token.total_supply(),
    );

    // Distribution is recorded.
    let dist = f.dividend.get_distribution(&distribution_id);
    assert_eq!(dist.total_amount, 1_500i128);
    assert_eq!(dist.snapshot_supply, 1_500i128);

    // holder_a holds 750 / 1500 => 750 dividend.
    let claimed_a = f.dividend.claim(&f.holder_a, &distribution_id);
    assert_eq!(claimed_a, 750i128);
    assert!(f.dividend.has_claimed(&f.holder_a, &distribution_id));

    // holder_b holds 500 / 1500 => 500 dividend.
    let claimed_b = f.dividend.claim(&f.holder_b, &distribution_id);
    assert_eq!(claimed_b, 500i128);
    assert!(f.dividend.has_claimed(&f.holder_b, &distribution_id));

    // holder_c holds 250 / 1500 => 250 dividend.
    let claimed_c = f.dividend.claim(&f.holder_c, &distribution_id);
    assert_eq!(claimed_c, 250i128);
    assert!(f.dividend.has_claimed(&f.holder_c, &distribution_id));

    // Total claimed equals the distribution amount.
    assert_eq!(claimed_a + claimed_b + claimed_c, 1_500i128);

    // ------------------------------------------------------------------
    // Final consistency checks across all four contracts.
    // ------------------------------------------------------------------
    // Registry still intact.
    assert!(f.registry.is_registered(&asset_id));

    // Compliance still reflects approved holders.
    assert!(f.compliance.is_approved(&f.holder_a));
    assert!(f.compliance.is_approved(&f.holder_b));
    assert!(f.compliance.is_approved(&f.holder_c));

    // Token balances unchanged by dividend claims.
    assert_eq!(f.asset_token.balance(&f.holder_a), 750i128);
    assert_eq!(f.asset_token.balance(&f.holder_b), 500i128);
    assert_eq!(f.asset_token.balance(&f.holder_c), 250i128);
}