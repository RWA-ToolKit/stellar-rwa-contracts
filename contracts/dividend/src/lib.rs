#![no_std]
//! # Dividend Contract
//!
//! Distributes yield/dividends to asset-token holders in proportion to their
//! holdings. An issuer funds a distribution with a payment token; each holder
//! then claims `total_amount * balance / total_supply`, paid from the escrow
//! this contract holds. Each holder can claim a given distribution once.
//!
//! Balances are frozen in a snapshot at distribution creation time; each holder's
//! entitlement is sized against this snapshot rather than live balances, preventing
//! post-creation transfers from inflating or diluting any holder's claim.
//! `created_at` records the ledger at which the distribution was created for reference.
//!
//! Entitlements use integer token units and integer division. If the asset token
//! has zero decimals, it cannot represent fractional holdings, and a small
//! proportional payment may round down to zero and be unclaimable.
//!
//! # Claim deadline & reclaim policy (issue #2)
//!
//! A distribution may optionally carry a `deadline` (a ledger sequence number).
//! This is a **policy decision**, documented here before the implementation
//! below:
//!
//! 1. `deadline == 0` means "no deadline" — the distribution behaves exactly
//!    as before and can be claimed at any time; funds are never reclaimable.
//! 2. When `deadline != 0`, holders may claim normally up to and including
//!    ledger `deadline`. Once `env.ledger().sequence() > deadline`, `claim`
//!    is rejected with `DeadlinePassed` — holders permanently lose the
//!    ability to claim after the deadline.
//! 3. Once the deadline has passed, the contract **admin** — and only the
//!    admin, not the original issuer or any other role — may call
//!    `reclaim_unclaimed` to sweep whatever remains unclaimed
//!    (`total_amount - distributed`) out of escrow to themselves. This
//!    exists so an issuer-controlled admin can recover dust/unclaimed funds
//!    rather than have them locked in the contract forever; it is
//!    intentionally restricted to admin because the admin is the only party
//!    that funded the escrow in the first place.
//! 4. Reclaiming marks the distribution `completed` and clears its snapshot,
//!    exactly like a distribution that was fully claimed. Reclaim before the
//!    deadline, or by a non-admin, is rejected.

#[cfg(test)]
extern crate std;

use asset_token::AssetMetadata;
use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, symbol_short, Address,
    Env, Map, Vec,
};

/// Read-only view of the asset token needed to size a holder's share and to
/// authorize issuer-owned distributions.
#[contractclient(name = "AssetClient")]
pub trait AssetInterface {
    fn total_supply(env: Env) -> i128;
    fn get_metadata(env: Env) -> AssetMetadata;
}

/// Minimal payment-token interface used to move escrowed funds.
#[contractclient(name = "TokenClient")]
pub trait TokenInterface {
    fn transfer(env: Env, from: Address, to: Address, amount: i128);
}

/// A single dividend distribution.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Distribution {
    pub id: u64,
    pub asset_token: Address,
    pub payment_token: Address,
    pub total_amount: i128,
    pub distributed: i128,
    pub created_at: u32,
    pub completed: bool,
    /// Ledger sequence after which claims are rejected and the admin may
    /// reclaim unclaimed funds (issue #2). `0` means no deadline.
    pub deadline: u32,
    /// Set to `true` by `cancel_distribution` (issue #428).
    ///
    /// A cancelled distribution is distinguished from a fully-paid one by
    /// this flag: both set `completed = true`, but only a cancelled
    /// distribution also sets `cancelled = true`. This makes the two
    /// terminal states distinguishable to any client or indexer that reads
    /// the stored entry, not just to the cancel event.
    pub cancelled: bool,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    /// Address nominated by the current admin via `propose_admin`, pending
    /// acceptance via `accept_admin` (issue #4). Absent when there is no
    /// proposal in flight.
    PendingAdmin,
    Counter,
    Ids,
    Dist(u64),
    Claimed(u64, Address),
    /// Distribution ids created for a given asset token, so
    /// `get_distributions_for_asset` only walks that asset's distributions
    /// instead of scanning the global counter (issue #166).
    AssetIds(Address),
    /// Sum of the snapshot balances captured at creation for a distribution
    /// (issue #163) — the denominator used to size every holder's share.
    Supply(u64),
    /// The `eligible` list passed to `create_distribution`, frozen at
    /// creation time so a holder's entitlement can't be inflated (or
    /// diluted) by balance changes after the fact (issue #163).
    Snapshot(u64),
}

#[contracterror]
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    DistributionNotFound = 4,
    InvalidAmount = 5,
    NothingToClaim = 6,
    AlreadyClaimed = 7,
    /// `asset_token` has zero total supply; no holder can ever claim (issue #49).
    ZeroSupply = 8,
    /// Total distributed would exceed the distribution's `total_amount` (issue #164).
    OverDistributed = 9,
    /// `total_amount * balance` would overflow i128 (issue #165).
    ArithmeticOverflow = 10,
    /// The `eligible` list contains more than one entry for the same address
    /// (issue #290). `snapshot_balance` only ever returns the first match, so a
    /// duplicate would inflate the denominator while its amount stays
    /// unclaimable — permanently stranding that slice of the escrow.
    DuplicateHolder = 11,
    /// `accept_admin` or `cancel_admin_proposal` called with no pending
    /// admin proposal on file (issue #4).
    NoPendingAdmin = 12,
    /// A claim was attempted after the distribution's `deadline` (issue #2).
    DeadlinePassed = 13,
    /// `reclaim_unclaimed` was called before the deadline (issue #2).
    DeadlineNotReached = 14,
    /// `reclaim_unclaimed` was called on a distribution with no deadline set
    /// (`deadline == 0`), i.e. one whose policy never permits reclaiming.
    NoDeadline = 15,
}

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Contract ABI/behavior version. Bump on any change to storage layout or
/// externally observable behavior so clients and the indexer can detect it.
pub const VERSION: u32 = 4;

#[contract]
pub struct DividendContract;

#[contractimpl]
impl DividendContract {
    /// Current contract version.
    pub fn version(_env: Env) -> u32 {
        VERSION
    }

    /// Initialize with an admin. Callable once.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_err(&env, Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Counter, &0u64);
        bump(&env);
        env.events().publish((symbol_short!("init"),), admin);
    }

    /// Create and fund a distribution. Pulls `total_amount` of `payment_token`
    /// from the admin into this contract's escrow. Admin only.
    ///
    /// `asset_token` must expose `total_supply()` and `balance()` (the
    /// asset-token interface defined in this crate). `payment_token` must
    /// implement the standard SAC / SEP-41 token interface; in particular its
    /// `transfer` must not trap on the outbound leg when holders claim (issue #50).
    ///
    /// # Trust assumption on `eligible` (issue #293)
    ///
    /// `eligible` is an admin-supplied `(Address, balance)` list that is frozen
    /// verbatim as the entitlement snapshot; every holder's share is sized
    /// against it. This contract does **not** call `AssetClient::balance` to
    /// cross-check any entry against the asset token's real holdings, so a
    /// malicious or buggy admin can hand shares to arbitrary addresses in
    /// arbitrary amounts. Producing an `eligible` list that faithfully mirrors
    /// the asset token's holders at creation time is the caller's
    /// responsibility. The contract only enforces structural sanity: entries
    /// must be non-negative (issue #291) and each address may appear at most
    /// once (issue #290).
    pub fn create_distribution(
        env: Env,
        admin: Address,
        asset_token: Address,
        payment_token: Address,
        total_amount: i128,
        eligible: Vec<(Address, i128)>,
    ) -> u64 {
        Self::create_distribution_deadline(
            env,
            admin,
            asset_token,
            payment_token,
            total_amount,
            eligible,
            0,
        )
    }

    /// Same as `create_distribution`, but sets a claim `deadline` (a ledger
    /// sequence number). See the module-level "Claim deadline & reclaim
    /// policy" doc for the rules a non-zero deadline enables (issue #2).
    /// `deadline == 0` is equivalent to `create_distribution` (no deadline).
    pub fn create_distribution_deadline(
        env: Env,
        admin: Address,
        asset_token: Address,
        payment_token: Address,
        total_amount: i128,
        eligible: Vec<(Address, i128)>,
        deadline: u32,
    ) -> u64 {
        Self::require_admin_or_asset_admin(&env, &admin, &asset_token);
        if total_amount <= 0 {
            panic_err(&env, Error::InvalidAmount);
        }
        // Reject distributions where no holder can ever claim (issue #49).
        let supply = AssetClient::new(&env, &asset_token).total_supply();
        if supply <= 0 {
            panic_err(&env, Error::ZeroSupply);
        }
        // Reject distributions with empty eligible set (issue #365).
        if eligible.len() == 0 {
            panic_err(&env, Error::InvalidAmount);
        }
        // Validate the eligible list and total its balances *before* pulling any
        // funds, so a rejected list never leaves escrow moved. This total is
        // frozen as the distribution's denominator (issue #163): every holder's
        // share is sized against this list, not the asset token's live balances,
        // so a post-creation transfer can neither inflate nor dilute anyone.
        let mut snapshot_supply: i128 = 0;
        let mut seen: Map<Address, ()> = Map::new(&env);
        for (addr, balance) in eligible.iter() {
            // Reject negative entries (issue #291): a negative balance shrinks
            // the denominator shared by every other holder, silently inflating
            // their payouts while the negative holder's own share floors at 0.
            if balance < 0 {
                panic_err(&env, Error::InvalidAmount);
            }
            // Reject duplicate holders (issue #290): `snapshot_balance` returns
            // the first matching entry only, so a second entry for the same
            // address would be counted in `snapshot_supply` but never be
            // claimable by anyone — that slice of the escrow would be stranded.
            if seen.contains_key(addr.clone()) {
                panic_err(&env, Error::DuplicateHolder);
            }
            seen.set(addr, ());
            snapshot_supply = snapshot_supply
                .checked_add(balance)
                .unwrap_or_else(|| panic_err(&env, Error::ArithmeticOverflow));
        }

        // Escrow the funds in this contract.
        let this = env.current_contract_address();
        TokenClient::new(&env, &payment_token).transfer(&admin, &this, &total_amount);

        let id: u64 = env.storage().instance().get(&DataKey::Counter).unwrap_or(0) + 1;

        env.storage()
            .persistent()
            .set(&DataKey::Snapshot(id), &eligible);
        env.storage().persistent().extend_ttl(
            &DataKey::Snapshot(id),
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
        env.storage()
            .persistent()
            .set(&DataKey::Supply(id), &snapshot_supply);
        env.storage().persistent().extend_ttl(
            &DataKey::Supply(id),
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
        let dist = Distribution {
            id,
            asset_token,
            payment_token,
            total_amount,
            distributed: 0,
            created_at: env.ledger().sequence(),
            completed: false,
            deadline,
            cancelled: false,
        };
        env.storage().persistent().set(&DataKey::Dist(id), &dist);
        env.storage().persistent().extend_ttl(
            &DataKey::Dist(id),
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
        // Index this distribution under its asset token so lookups are O(n_asset)
        // rather than O(global counter) (issue #166).
        let mut asset_ids = env
            .storage()
            .persistent()
            .get::<DataKey, Vec<u64>>(&DataKey::AssetIds(dist.asset_token.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        asset_ids.push_back(id);
        env.storage()
            .persistent()
            .set(&DataKey::AssetIds(dist.asset_token.clone()), &asset_ids);
        env.storage().persistent().extend_ttl(
            &DataKey::AssetIds(dist.asset_token.clone()),
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
        env.storage().instance().set(&DataKey::Counter, &id);
        bump(&env);
        env.events()
            .publish((symbol_short!("created"), admin), (id, total_amount));
        id
    }

    /// Amount a holder can still claim from a distribution (0 if already
    /// claimed, holds nothing, or the distribution is empty).
    ///
    /// # Rounding behaviour & dust (issue #4)
    ///
    /// Each claim is `floor(total_amount * balance_i / supply)`: integer
    /// division truncates toward zero, so a holder's actual payout can be up
    /// to (but never more than) `1` unit of `payment_token` less than their
    /// exact proportional share. Because every holder's share is computed
    /// independently, these per-holder rounding losses do **not** cancel out
    /// — they only ever accumulate.
    ///
    /// **Worst-case dust, quantified:** let `N` be the number of entries in
    /// the `eligible` snapshot. Writing each exact share as
    /// `a_i = total_amount * balance_i / supply` (a real number), we have
    /// `sum(a_i) = total_amount` exactly (since `sum(balance_i) = supply`).
    /// The permanently-unclaimable dust is `total_amount - sum(floor(a_i))
    /// = sum(frac(a_i))`, a sum of `N` fractional terms each in `[0, 1)`.
    /// That sum is therefore strictly less than `N` and, being an integer
    /// (both `total_amount` and the floored sum are integers), satisfies
    /// `0 <= dust <= N - 1`. In other words: **at most one unit of
    /// `payment_token` per eligible holder can be left stranded in escrow**,
    /// and this bound is tight (achievable when every holder's remainder is
    /// `supply - 1`). This dust is never reclaimed by `claim`/`claimable`;
    /// see `reclaim_unclaimed` (issue #2) for the only way an admin can
    /// recover it, and once a deadline is set.
    ///
    /// This is documented, expected behaviour, not a bug: exact proportional
    /// division is generally impossible over integers, and the alternative
    /// (rounding some holders up) would let claims collectively exceed
    /// `total_amount`, violating the escrow invariant enforced by
    /// `OverDistributed`. See `test::test_uneven_distribution_leaves_dust` in
    /// `test.rs` for a worked example.
    pub fn claimable(env: Env, distribution_id: u64, holder: Address) -> i128 {
        let dist = Self::load(&env, distribution_id);
        if Self::has_claimed(env.clone(), distribution_id, holder.clone()) {
            return 0;
        }
        let supply = Self::load_supply(&env, distribution_id);
        if supply <= 0 {
            return 0;
        }
        let basis = Self::snapshot_balance(&env, distribution_id, &holder);
        if basis <= 0 {
            return 0;
        }
        // Proportional share, floored by integer division. For zero-decimal
        // asset tokens this can make small claims equal to zero. Guard the
        // multiplication against i128 overflow (issue #165).
        dist.total_amount
            .checked_mul(basis)
            .unwrap_or_else(|| panic_err(&env, Error::ArithmeticOverflow))
            / supply
    }

    /// Claim a holder's proportional share, paid from escrow. Holder-authorized.
    ///
    /// # Double-claim guard (issue #1)
    ///
    /// Each `(distribution_id, holder)` pair may be claimed at most once. This
    /// is enforced by the `DataKey::Claimed(distribution_id, holder)` flag:
    /// `claim` checks the flag first and panics with `AlreadyClaimed (#7)` if
    /// it is already set, then sets it to `true` **before** the outbound
    /// token transfer. Setting the flag before the transfer (rather than
    /// after) matters because Soroban aborts and rolls back all storage
    /// writes if any step in the function traps — so even if the transfer
    /// itself were to panic, there is no window where the flag is set but the
    /// funds were not sent, nor a window where funds could be sent twice by
    /// re-entering before the flag is persisted. Because the flag is keyed
    /// per-holder, claims from different holders are fully independent: they
    /// touch disjoint storage keys and interleaving them (in any order, or
    /// concurrently across separate transactions) can never cause one
    /// holder's claim to block or double-pay another's. See
    /// `test::claim_twice_by_same_holder_fails` and
    /// `test::interleaved_claims_by_different_holders_all_succeed` in
    /// `test.rs` for the properties this guard is expected to uphold.
    pub fn claim(env: Env, distribution_id: u64, holder: Address) {
        holder.require_auth();
        let mut dist = Self::load(&env, distribution_id);
        // Policy (issue #2): once past the deadline, claims are rejected —
        // only `reclaim_unclaimed` (admin-only) may move funds after this
        // point.
        if dist.deadline != 0 && env.ledger().sequence() > dist.deadline {
            panic_err(&env, Error::DeadlinePassed);
        }
        if Self::has_claimed(env.clone(), distribution_id, holder.clone()) {
            panic_err(&env, Error::AlreadyClaimed);
        }
        let amount = Self::claimable(env.clone(), distribution_id, holder.clone());
        if amount <= 0 {
            panic_err(&env, Error::NothingToClaim);
        }
        env.storage()
            .persistent()
            .set(&DataKey::Claimed(distribution_id, holder.clone()), &true);

        let this = env.current_contract_address();
        TokenClient::new(&env, &dist.payment_token).transfer(&this, &holder, &amount);

        dist.distributed = dist
            .distributed
            .checked_add(amount)
            // Overflow of the running total is an arithmetic fault, not bad
            // input — report it as such so callers can tell the two apart, and
            // to match `claimable`'s use of the same variant (issue #292).
            .unwrap_or_else(|| panic_err(&env, Error::ArithmeticOverflow));
        if dist.distributed > dist.total_amount {
            panic_err(&env, Error::OverDistributed);
        }
        if dist.distributed >= dist.total_amount {
            dist.completed = true;
            env.storage()
                .persistent()
                .remove(&DataKey::Snapshot(distribution_id));
            env.storage()
                .persistent()
                .remove(&DataKey::Supply(distribution_id));
        }
        env.storage()
            .persistent()
            .set(&DataKey::Dist(distribution_id), &dist);
        env.storage().persistent().extend_ttl(
            &DataKey::Dist(distribution_id),
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
        bump(&env);
        env.events()
            .publish((symbol_short!("claim"), holder), (distribution_id, amount));
    }

    /// Sweep whatever remains unclaimed (`total_amount - distributed`) out of
    /// escrow to the admin, once a distribution's deadline has passed.
    /// Admin-authorized only (issue #2 policy). Errors:
    /// `NoDeadline (#15)` if the distribution has no deadline set;
    /// `DeadlineNotReached (#14)` if called before the deadline;
    /// `NothingToClaim (#6)` if everything was already claimed or reclaimed.
    pub fn reclaim_unclaimed(env: Env, admin: Address, distribution_id: u64) -> i128 {
        Self::require_admin(&env, &admin);
        let mut dist = Self::load(&env, distribution_id);
        if dist.deadline == 0 {
            panic_err(&env, Error::NoDeadline);
        }
        if env.ledger().sequence() <= dist.deadline {
            panic_err(&env, Error::DeadlineNotReached);
        }
        let remaining = dist.total_amount - dist.distributed;
        if remaining <= 0 {
            panic_err(&env, Error::NothingToClaim);
        }

        let this = env.current_contract_address();
        TokenClient::new(&env, &dist.payment_token).transfer(&this, &admin, &remaining);

        dist.distributed = dist.total_amount;
        dist.completed = true;
        env.storage()
            .persistent()
            .remove(&DataKey::Snapshot(distribution_id));
        env.storage()
            .persistent()
            .remove(&DataKey::Supply(distribution_id));
        env.storage()
            .persistent()
            .set(&DataKey::Dist(distribution_id), &dist);
        env.storage().persistent().extend_ttl(
            &DataKey::Dist(distribution_id),
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
        bump(&env);
        env.events().publish(
            (symbol_short!("reclaim"), admin),
            (distribution_id, remaining),
        );
        remaining
    }

    /// Cancel a distribution and return escrowed funds to the issuer.
    /// Only works while nothing has been claimed (distributed == 0). Admin only.
    pub fn cancel_distribution(env: Env, admin: Address, distribution_id: u64) {
        Self::require_admin(&env, &admin);
        let dist = Self::load(&env, distribution_id);
        // Only allow cancellation before any claim is made (issue #366).
        if dist.distributed > 0 {
            panic_err(&env, Error::InvalidAmount);
        }
        // Return escrowed funds to the issuer.
        let this = env.current_contract_address();
        TokenClient::new(&env, &dist.payment_token).transfer(&this, &admin, &dist.total_amount);
        // Mark as completed so no further claims are possible, and set
        // `cancelled = true` so callers can distinguish this terminal state
        // from a fully-paid distribution (issue #428). Both fields are set
        // before writing the entry back so the stored record is always
        // internally consistent.
        let mut cancelled_dist = dist;
        cancelled_dist.completed = true;
        cancelled_dist.cancelled = true;
        env.storage()
            .persistent()
            .set(&DataKey::Dist(distribution_id), &cancelled_dist);
        env.storage().persistent().extend_ttl(
            &DataKey::Dist(distribution_id),
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
        bump(&env);
        env.events()
            .publish((symbol_short!("cancel"), admin), distribution_id);
    }

    /// Fetch a distribution by id.
    pub fn get_distribution(env: Env, distribution_id: u64) -> Distribution {
        Self::load(&env, distribution_id)
    }

    /// All distributions created for a given asset token.
    /// Walks only the per-asset id index (issue #166) instead of scanning the
    /// global counter, keeping the cost proportional to that asset's
    /// distributions rather than every distribution ever created.
    pub fn get_distributions_for_asset(env: Env, asset_token: Address) -> Vec<Distribution> {
        let ids = env
            .storage()
            .persistent()
            .get::<DataKey, Vec<u64>>(&DataKey::AssetIds(asset_token.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        let mut out = Vec::new(&env);
        for id in ids.iter() {
            if let Some(d) = env
                .storage()
                .persistent()
                .get::<DataKey, Distribution>(&DataKey::Dist(id))
            {
                env.storage().persistent().extend_ttl(
                    &DataKey::Dist(id),
                    INSTANCE_LIFETIME_THRESHOLD,
                    INSTANCE_BUMP_AMOUNT,
                );
                out.push_back(d);
            }
        }
        out
    }

    /// Whether a holder has already claimed a distribution.
    pub fn has_claimed(env: Env, distribution_id: u64, holder: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Claimed(distribution_id, holder))
            .unwrap_or(false)
    }

    /// Effective supply for a distribution: the sum of the snapshot balances
    /// captured at creation (issue #163).
    fn load_supply(env: &Env, distribution_id: u64) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Supply(distribution_id))
            .unwrap_or(0)
    }

    /// A holder's entitlement basis: the balance recorded in the distribution's
    /// creation-time snapshot. Wallets not present in the snapshot (e.g. ones
    /// that received tokens only afterwards) have a basis of 0 and cannot claim
    /// (issue #163).
    fn snapshot_balance(env: &Env, distribution_id: u64, holder: &Address) -> i128 {
        let snap: Vec<(Address, i128)> = env
            .storage()
            .persistent()
            .get(&DataKey::Snapshot(distribution_id))
            .unwrap_or_else(|| panic_err(env, Error::DistributionNotFound));
        for (h, b) in snap.iter() {
            if h == *holder {
                return b;
            }
        }
        0
    }

    /// Configured admin.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_err(&env, Error::NotInitialized))
    }

    fn require_admin_or_asset_admin(env: &Env, admin: &Address, asset_token: &Address) {
        let stored: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_err(env, Error::NotInitialized));
        admin.require_auth();
        if *admin == stored {
            return;
        }
        let asset_admin = AssetClient::new(env, asset_token).get_metadata().admin;
        if *admin == asset_admin {
            return;
        }
        panic_err(env, Error::Unauthorized);
    }

    /// Propose a new admin. Requires authorization from the current admin.
    /// The role does not move yet — `new_admin` must call `accept_admin`
    /// before the handover takes effect (issue #4). This makes a mistyped
    /// `new_admin` harmless (it can simply be re-proposed or cancelled)
    /// instead of a single-step transfer that would permanently brick
    /// administration.
    pub fn propose_admin(env: Env, admin: Address, new_admin: Address) {
        Self::require_admin(&env, &admin);
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        bump(&env);
        env.events()
            .publish((symbol_short!("proposed"), admin), new_admin);
    }

    /// Cancel a pending admin proposal. Requires authorization from the
    /// current admin. Panics with `NoPendingAdmin` if there is nothing to
    /// cancel.
    pub fn cancel_admin_proposal(env: Env, admin: Address) {
        Self::require_admin(&env, &admin);
        if !env.storage().instance().has(&DataKey::PendingAdmin) {
            panic_err(&env, Error::NoPendingAdmin);
        }
        env.storage().instance().remove(&DataKey::PendingAdmin);
        bump(&env);
        env.events()
            .publish((symbol_short!("cancelled"), admin), ());
    }

    /// Accept a pending admin proposal, completing the handover. Must be
    /// called by the proposed successor (issue #4); the role only ever moves
    /// here, never in `propose_admin`. Emits `set_admin` carrying both the
    /// previous and new admin so off-chain indexers can observe this
    /// security-critical transition (issue #2).
    pub fn accept_admin(env: Env, new_admin: Address) {
        new_admin.require_auth();
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .unwrap_or_else(|| panic_err(&env, Error::NoPendingAdmin));
        if pending != new_admin {
            panic_err(&env, Error::Unauthorized);
        }
        let old_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_err(&env, Error::NotInitialized));
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        bump(&env);
        env.events()
            .publish((symbol_short!("set_admin"), old_admin), new_admin);
    }

    /// The address currently proposed as the next admin, if any.
    pub fn get_pending_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::PendingAdmin)
    }

    // ---- internal helpers ----

    fn load(env: &Env, id: u64) -> Distribution {
        let dist = env
            .storage()
            .persistent()
            .get(&DataKey::Dist(id))
            .unwrap_or_else(|| panic_err(env, Error::DistributionNotFound));
        env.storage().persistent().extend_ttl(
            &DataKey::Dist(id),
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
        dist
    }

    fn require_admin(env: &Env, admin: &Address) {
        let stored: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_err(env, Error::NotInitialized));
        admin.require_auth();
        if stored != *admin {
            panic_err(env, Error::Unauthorized);
        }
    }
}

fn bump(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

fn panic_err(env: &Env, error: Error) -> ! {
    soroban_sdk::panic_with_error!(env, error)
}

#[cfg(test)]
mod test;
