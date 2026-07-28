#![cfg(test)]
use super::*;
use asset_token::{AssetTokenContract, AssetTokenContractClient};
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{
    testutils::{storage::Persistent as _, Address as _, Ledger as _},
    token, vec, Address, Env, String,
};

struct Ctx {
    env: Env,
    dividend: DividendContractClient<'static>,
    asset: AssetTokenContractClient<'static>,
    comp: ComplianceContractClient<'static>,
    asset_id: Address,
    pay_id: Address,
    admin: Address,
    h1: Address,
    h2: Address,
}

impl Ctx {
    /// A fresh, compliance-approved address holding nothing.
    fn new_holder(&self) -> Address {
        let who = Address::generate(&self.env);
        self.comp
            .add_to_allowlist(&self.admin, &who, &String::from_str(&self.env, "US"), &0);
        who
    }
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
        asset,
        comp,
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

/// The holder snapshot matching `setup`: h1 300, h2 200, admin 500.
fn full_snapshot(ctx: &Ctx) -> Vec<SnapshotEntry> {
    vec![
        &ctx.env,
        SnapshotEntry {
            holder: ctx.h1.clone(),
            balance: 300,
        },
        SnapshotEntry {
            holder: ctx.h2.clone(),
            balance: 200,
        },
        SnapshotEntry {
            holder: ctx.admin.clone(),
            balance: 500,
        },
    ]
}

fn create(ctx: &Ctx, total_amount: i128) -> u64 {
    ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &total_amount,
        &full_snapshot(ctx),
    )
}

fn advance_ledgers(ctx: &Ctx, n: u32) {
    ctx.env.ledger().with_mut(|li| li.sequence_number += n);
}

#[test]
fn test_initialize_admin() {
    let ctx = setup();
    assert_eq!(ctx.dividend.get_admin(), ctx.admin);
}

#[test]
fn test_create_distribution_escrows_funds() {
    let ctx = setup();
    let div_addr = ctx.dividend.address.clone();
    let id = create(&ctx, 1000);
    assert_eq!(id, 1);
    assert_eq!(pay_balance(&ctx, &div_addr), 1000);
    let d = ctx.dividend.get_distribution(&id);
    assert_eq!(d.total_amount, 1000);
    assert_eq!(d.distributed, 0);
    assert_eq!(d.snapshot_supply, 1000);
    assert_eq!(d.allocated, 1000);
    assert!(!d.completed);
}

#[test]
fn test_claim_is_proportional() {
    let ctx = setup();
    let id = create(&ctx, 1000);
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
    let id = create(&ctx, 1000);
    ctx.dividend.claim(&id, &ctx.h1);
    ctx.dividend.claim(&id, &ctx.h1);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_nonholder_nothing_to_claim() {
    let ctx = setup();
    let id = create(&ctx, 1000);
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
        &full_snapshot(&ctx),
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_zero_amount_rejected() {
    let ctx = setup();
    create(&ctx, 0);
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
    create(&ctx, 1000);
    create(&ctx, 500);
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
    let id = create(&ctx, 1000);
    ctx.dividend.claim(&id, &ctx.h1); // 300
    ctx.dividend.claim(&id, &ctx.h2); // 200
    ctx.dividend.claim(&id, &ctx.admin); // 500
    let d = ctx.dividend.get_distribution(&id);
    assert_eq!(d.distributed, 1000);
    assert!(d.completed);
    assert_eq!(pay_balance(&ctx, &ctx.h2), 200);
    assert_eq!(pay_balance(&ctx, &ctx.admin), 100_000 - 1000 + 500);
}

// ---- #5: shares come from the snapshot, not the live balance ----

#[test]
fn test_share_follows_snapshot_not_live_balance() {
    let ctx = setup();
    let id = create(&ctx, 1000);

    // h1 claims its snapshotted 300, then moves the whole position to a fresh
    // compliant address. The snapshot is what pays, so the tokens arriving at
    // h3 buy no new claim and the escrow is only drawn down once.
    ctx.dividend.claim(&id, &ctx.h1);
    let h3 = ctx.new_holder();
    ctx.asset.transfer(&ctx.h1, &h3, &300);

    assert_eq!(ctx.asset.balance(&h3), 300);
    assert_eq!(ctx.dividend.snapshot_balance(&id, &h3), 0);
    assert_eq!(ctx.dividend.claimable(&id, &h3), 0);
    assert_eq!(ctx.dividend.claimable(&id, &ctx.h1), 0);
    assert_eq!(pay_balance(&ctx, &ctx.dividend.address), 700);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_recipient_of_moved_tokens_cannot_claim() {
    let ctx = setup();
    let id = create(&ctx, 1000);
    ctx.dividend.claim(&id, &ctx.h1);
    let h3 = ctx.new_holder();
    ctx.asset.transfer(&ctx.h1, &h3, &300);
    ctx.dividend.claim(&id, &h3);
}

#[test]
fn test_buying_tokens_after_snapshot_does_not_raise_share() {
    let ctx = setup();
    let id = create(&ctx, 1000);
    // admin sends h2 another 400 after the snapshot was taken.
    ctx.asset.transfer(&ctx.admin, &ctx.h2, &400);
    assert_eq!(ctx.asset.balance(&ctx.h2), 600);
    // h2 is still paid on its snapshotted 200.
    assert_eq!(ctx.dividend.claimable(&id, &ctx.h2), 200);
    ctx.dividend.claim(&id, &ctx.h2);
    assert_eq!(pay_balance(&ctx, &ctx.h2), 200);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_empty_snapshot_rejected() {
    let ctx = setup();
    ctx.dividend.create_distribution(
        &ctx.admin,
        &ctx.asset_id,
        &ctx.pay_id,
        &1000,
        &vec![&ctx.env],
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_duplicate_snapshot_holder_rejected() {
    let ctx = setup();
    let dup = vec![
        &ctx.env,
        SnapshotEntry {
            holder: ctx.h1.clone(),
            balance: 300,
        },
        SnapshotEntry {
            holder: ctx.h1.clone(),
            balance: 300,
        },
    ];
    ctx.dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &dup);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_snapshot_exceeding_supply_rejected() {
    let ctx = setup();
    let too_much = vec![
        &ctx.env,
        SnapshotEntry {
            holder: ctx.h1.clone(),
            balance: 5000,
        },
    ];
    ctx.dividend
        .create_distribution(&ctx.admin, &ctx.asset_id, &ctx.pay_id, &1000, &too_much);
}

// ---- #6: rounding dust is reclaimable and distributions can close ----

#[test]
fn test_dust_is_tracked_and_reclaimable() {
    let ctx = setup();
    // 999 over 300/200/500 floors to 299 + 199 + 499 = 997, leaving 2 of dust.
    let id = create(&ctx, 999);
    let d = ctx.dividend.get_distribution(&id);
    assert_eq!(d.allocated, 997);
    assert_eq!(d.total_amount - d.allocated, 2);

    ctx.dividend.claim(&id, &ctx.h1);
    ctx.dividend.claim(&id, &ctx.h2);
    ctx.dividend.claim(&id, &ctx.admin);

    // Every allocated share is claimed, so the distribution completes even
    // though `distributed` never reaches `total_amount`.
    let d = ctx.dividend.get_distribution(&id);
    assert_eq!(d.distributed, 997);
    assert!(d.completed);
    assert_eq!(pay_balance(&ctx, &ctx.dividend.address), 2);

    // The dust is swept immediately: no need to wait out the claim window.
    let swept = ctx.dividend.reclaim_unclaimed(&ctx.admin, &id);
    assert_eq!(swept, 2);
    assert_eq!(pay_balance(&ctx, &ctx.dividend.address), 0);
    assert_eq!(pay_balance(&ctx, &ctx.admin), 100_000 - 999 + 499 + 2);
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_reclaim_blocked_while_claim_window_open() {
    let ctx = setup();
    let id = create(&ctx, 1000);
    ctx.dividend.claim(&id, &ctx.h1);
    ctx.dividend.reclaim_unclaimed(&ctx.admin, &id);
}

#[test]
fn test_reclaim_after_claim_window() {
    let ctx = setup();
    let id = create(&ctx, 1000);
    ctx.dividend.claim(&id, &ctx.h1); // 300 out, 700 unclaimed

    advance_ledgers(&ctx, CLAIM_WINDOW_LEDGERS + 1);

    let swept = ctx.dividend.reclaim_unclaimed(&ctx.admin, &id);
    assert_eq!(swept, 700);
    assert_eq!(pay_balance(&ctx, &ctx.dividend.address), 0);
    let d = ctx.dividend.get_distribution(&id);
    assert!(d.completed);
    assert_eq!(d.distributed, d.total_amount);
}

#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn test_claim_after_reclaim_rejected() {
    let ctx = setup();
    let id = create(&ctx, 1000);
    advance_ledgers(&ctx, CLAIM_WINDOW_LEDGERS + 1);
    ctx.dividend.reclaim_unclaimed(&ctx.admin, &id);
    ctx.dividend.claim(&id, &ctx.h1);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_reclaim_twice_rejected() {
    let ctx = setup();
    let id = create(&ctx, 1000);
    advance_ledgers(&ctx, CLAIM_WINDOW_LEDGERS + 1);
    ctx.dividend.reclaim_unclaimed(&ctx.admin, &id);
    ctx.dividend.reclaim_unclaimed(&ctx.admin, &id);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_reclaim_requires_admin() {
    let ctx = setup();
    let id = create(&ctx, 1000);
    advance_ledgers(&ctx, CLAIM_WINDOW_LEDGERS + 1);
    let impostor = Address::generate(&ctx.env);
    ctx.dividend.reclaim_unclaimed(&impostor, &id);
}

// ---- #7: claim markers are TTL-bumped so they cannot be archived early ----

/// TTL of a persistent entry of the dividend contract, in ledgers remaining.
fn ttl_of(ctx: &Ctx, key: DataKey) -> u32 {
    ctx.env.as_contract(&ctx.dividend.address, || {
        ctx.env.storage().persistent().get_ttl(&key)
    })
}

#[test]
fn test_claim_marker_is_written_and_ttl_extended() {
    let ctx = setup();
    let id = create(&ctx, 1000);
    ctx.dividend.claim(&id, &ctx.h1);
    assert!(ctx.dividend.has_claimed(&id, &ctx.h1));

    // Without an explicit bump the marker would only carry the network minimum
    // (4096 ledgers by default), be archived long before the distribution, and
    // let `has_claimed` read false again.
    let marker_ttl = ttl_of(&ctx, DataKey::Claimed(id, ctx.h1.clone()));
    assert_eq!(marker_ttl, PERSISTENT_BUMP_AMOUNT);
    assert!(marker_ttl > CLAIM_WINDOW_LEDGERS);

    // It must also outlive the distribution entry it guards, and the snapshot.
    assert!(marker_ttl >= ttl_of(&ctx, DataKey::Dist(id)));
    assert!(marker_ttl >= ttl_of(&ctx, DataKey::Snapshot(id, ctx.h1.clone())));
}

#[test]
fn test_distribution_and_snapshot_ttl_extended_at_creation() {
    let ctx = setup();
    let id = create(&ctx, 1000);
    assert_eq!(ttl_of(&ctx, DataKey::Dist(id)), PERSISTENT_BUMP_AMOUNT);
    assert_eq!(
        ttl_of(&ctx, DataKey::Snapshot(id, ctx.h1.clone())),
        PERSISTENT_BUMP_AMOUNT
    );
}

#[test]
fn test_claim_marker_outlives_the_claim_window() {
    let ctx = setup();
    let id = create(&ctx, 1000);
    ctx.dividend.claim(&id, &ctx.h1);

    // Walk right past the claim window: the marker is still live, so a second
    // claim is still refused and the escrow is only ever drawn down once.
    advance_ledgers(&ctx, CLAIM_WINDOW_LEDGERS + 1);
    assert!(ttl_of(&ctx, DataKey::Claimed(id, ctx.h1.clone())) > 0);
    assert!(ctx.dividend.has_claimed(&id, &ctx.h1));
    assert_eq!(ctx.dividend.claimable(&id, &ctx.h1), 0);
}
