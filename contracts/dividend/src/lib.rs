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
    /// Returns the contract's ABI/behavior version number.
    ///
    /// Callers and indexers can use this to detect schema changes without
    /// having to probe individual storage entries.
    ///
    /// # Parameters
    /// - `_env`: Soroban environment (unused).
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// None.
    ///
    /// # Events
    /// None.
    pub fn version(_env: Env) -> u32 {
        VERSION
    }

    /// Initialize the contract and set the first admin. Callable exactly once.
    ///
    /// # Parameters
    /// - `admin`: Address that will administer distributions.  Must authorize
    ///   the call.
    ///
    /// # Authority
    /// `admin` must sign the transaction (`admin.require_auth()`).
    ///
    /// # Errors
    /// - [`Error::AlreadyInitialized`] — contract was already initialized.
    ///
    /// # Events
    /// Emits topic `("init",)` with data `admin`.
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

    /// Create and fund a distribution from the current holders' snapshot.
    ///
    /// Convenience wrapper around [`Self::create_distribution_deadline`] with
    /// `deadline = 0` (no deadline).  All validation and escrow behaviour is
    /// identical; see [`Self::create_distribution_deadline`] for the full
    /// parameter and error documentation.
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `asset_token`: Address of the asset-token contract whose holders are
    ///   eligible.  Must expose `total_supply()`.
    /// - `payment_token`: SAC / SEP-41 token used to pay holders.  Must
    ///   implement `transfer`.
    /// - `total_amount`: Total units of `payment_token` to escrow.  Must be
    ///   > 0.
    /// - `eligible`: Snapshot of `(holder_address, balance)` pairs that
    ///   determines each holder's share.  Must be non-empty; each balance must
    ///   be ≥ 0; no address may appear more than once.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    /// - [`Error::InvalidAmount`] — `total_amount` ≤ 0, `eligible` is empty,
    ///   or any entry's balance is negative.
    /// - [`Error::ZeroSupply`] — `asset_token` reports a zero total supply.
    /// - [`Error::DuplicateHolder`] — the same address appears more than once
    ///   in `eligible`.
    /// - [`Error::ArithmeticOverflow`] — the sum of snapshot balances would
    ///   overflow `i128`.
    ///
    /// # Events
    /// Emits topic `("created", admin)` with data `(id, total_amount)`.
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

    /// Create and fund a distribution with an optional claim deadline.
    ///
    /// Pulls `total_amount` of `payment_token` from the admin into this
    /// contract's escrow.  The `eligible` snapshot is frozen verbatim at
    /// creation time; each holder's share is sized against it rather than
    /// live balances, so post-creation transfers cannot inflate or dilute any
    /// entitlement.
    ///
    /// When `deadline != 0`: holders may claim up to and including ledger
    /// `deadline`; after that `claim` is rejected and only
    /// [`Self::reclaim_unclaimed`] (admin-only) may move the remaining funds.
    /// When `deadline == 0`: behaviour is identical to
    /// [`Self::create_distribution`] — no deadline, funds are never
    /// reclaimable.
    ///
    /// See the module-level "Claim deadline & reclaim policy" section for
    /// the full rules (issue #2).
    ///
    /// # Trust assumption on `eligible` (issue #293)
    ///
    /// This contract does **not** cross-check the supplied balances against the
    /// asset token's live state.  Producing an `eligible` list that faithfully
    /// mirrors the asset token's holders at creation time is the caller's
    /// responsibility.
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `asset_token`: Address of the asset-token contract whose holders are
    ///   eligible.  Must expose `total_supply()`.
    /// - `payment_token`: SAC / SEP-41 token used to pay holders.  Must
    ///   implement `transfer`.
    /// - `total_amount`: Total units of `payment_token` to escrow.  Must be
    ///   > 0.
    /// - `eligible`: Snapshot of `(holder_address, balance)` pairs.  Must be
    ///   non-empty; each balance must be ≥ 0; no address may appear more than
    ///   once.
    /// - `deadline`: Ledger sequence after which claims are rejected.  `0`
    ///   means no deadline.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    /// - [`Error::InvalidAmount`] — `total_amount` ≤ 0, `eligible` is empty,
    ///   or any entry's balance is negative.
    /// - [`Error::ZeroSupply`] — `asset_token` reports a zero total supply.
    /// - [`Error::DuplicateHolder`] — the same address appears more than once
    ///   in `eligible`.
    /// - [`Error::ArithmeticOverflow`] — the sum of snapshot balances would
    ///   overflow `i128`.
    ///
    /// # Events
    /// Emits topic `("created", admin)` with data `(id, total_amount)`.
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

    /// Amount a holder can still claim from a distribution.
    ///
    /// Returns `0` if the holder has already claimed, is not in the
    /// distribution's snapshot, or the snapshot supply is zero.
    ///
    /// Each holder's share is `floor(total_amount * balance_i / snapshot_supply)`:
    /// integer division truncates toward zero, so a small proportional share
    /// may round down to zero and be permanently unclaimable.  See the note
    /// below for the formal dust bound.
    ///
    /// # Rounding behaviour & dust (issue #4)
    ///
    /// Writing each exact share as `a_i = total_amount * balance_i / supply`
    /// (real number), the permanently-unclaimable dust is
    /// `total_amount - sum(floor(a_i))`.  Because each fractional term lies in
    /// `[0, 1)`, the dust is strictly less than the number of eligible holders
    /// (`N`) and satisfies `0 ≤ dust ≤ N − 1`.  **At most one unit of
    /// `payment_token` per eligible holder can be stranded in escrow.**  See
    /// [`Self::reclaim_unclaimed`] for the only way to recover dust once a
    /// deadline has passed.
    ///
    /// # Parameters
    /// - `distribution_id`: Id of the distribution to query.
    /// - `holder`: Address of the holder to check.
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// - [`Error::DistributionNotFound`] — no distribution with
    ///   `distribution_id` exists.
    /// - [`Error::ArithmeticOverflow`] — `total_amount * balance_i` would
    ///   overflow `i128` (issue #165).
    ///
    /// # Events
    /// None.
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

    /// Claim a holder's proportional share from a distribution.
    ///
    /// Transfers the holder's share (as computed by [`Self::claimable`]) from
    /// this contract's escrow to the holder.  Each `(distribution_id, holder)`
    /// pair may be claimed at most once.
    ///
    /// The claim flag is written **before** the outbound token transfer: if the
    /// transfer traps, Soroban rolls back all storage writes, so there is no
    /// window where a holder can be double-paid, nor a window where the flag is
    /// set but the funds were not sent (issue #1).
    ///
    /// If the distribution carries a `deadline`, claims are rejected once
    /// `current_ledger > deadline`; only [`Self::reclaim_unclaimed`]
    /// (admin-only) may move funds after that point (issue #2).
    ///
    /// # Parameters
    /// - `distribution_id`: Id of the distribution to claim from.
    /// - `holder`: Address claiming their share.  Must authorize the call.
    ///
    /// # Authority
    /// `holder` must sign the transaction (`holder.require_auth()`).
    ///
    /// # Errors
    /// - [`Error::DistributionNotFound`] — no distribution with
    ///   `distribution_id` exists.
    /// - [`Error::DeadlinePassed`] — the distribution's deadline has passed.
    /// - [`Error::AlreadyClaimed`] — `holder` has already claimed this
    ///   distribution.
    /// - [`Error::NothingToClaim`] — `holder`'s computed share is zero
    ///   (not in the snapshot or balance rounded to zero).
    /// - [`Error::ArithmeticOverflow`] — share or running-total calculation
    ///   overflows `i128`.
    /// - [`Error::OverDistributed`] — the new running total would exceed
    ///   `total_amount` (invariant violation).
    ///
    /// # Events
    /// Emits topic `("claim", holder)` with data `(distribution_id, amount)`.
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

    /// Sweep unclaimed funds out of escrow after a distribution's deadline.
    ///
    /// Transfers `total_amount - distributed` from this contract's escrow to
    /// the admin, then marks the distribution `completed` and clears its
    /// snapshot — exactly as if it had been fully claimed.  This is the only
    /// way to recover dust and unclaimed amounts once a deadline has passed
    /// (issue #2 policy).
    ///
    /// Reclaim before the deadline, or on a distribution with no deadline, is
    /// rejected.
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `distribution_id`: Id of the distribution to reclaim from.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    /// - [`Error::DistributionNotFound`] — no distribution with
    ///   `distribution_id` exists.
    /// - [`Error::NoDeadline`] — the distribution has no deadline set
    ///   (`deadline == 0`); its policy never permits reclaiming.
    /// - [`Error::DeadlineNotReached`] — `current_ledger <= deadline`;
    ///   the deadline has not yet passed.
    /// - [`Error::NothingToClaim`] — all funds were already claimed or
    ///   reclaimed.
    ///
    /// # Events
    /// Emits topic `("reclaim", admin)` with data `(distribution_id, remaining)`.
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

    /// Cancel a distribution and return the full escrowed amount to the admin.
    ///
    /// Only permitted before any claim has been processed (`distributed == 0`).
    /// Marks the distribution `completed` so no further claims can be made.
    /// Admin only.
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `distribution_id`: Id of the distribution to cancel.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    /// - [`Error::DistributionNotFound`] — no distribution with
    ///   `distribution_id` exists.
    /// - [`Error::InvalidAmount`] — at least one claim has already been
    ///   processed (`distributed > 0`); cancellation is not allowed.
    ///
    /// # Events
    /// Emits topic `("cancel", admin)` with data `distribution_id`.
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
        // Mark as completed so no further claims are possible.
        let mut cancelled_dist = dist;
        cancelled_dist.completed = true;
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

    /// Fetch a distribution by its id.
    ///
    /// # Parameters
    /// - `distribution_id`: Id of the distribution to fetch.
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// - [`Error::DistributionNotFound`] — no distribution with
    ///   `distribution_id` exists.
    ///
    /// # Events
    /// None.
    pub fn get_distribution(env: Env, distribution_id: u64) -> Distribution {
        Self::load(&env, distribution_id)
    }

    /// Returns all distributions created for a given asset token.
    ///
    /// Walks only the per-asset id index (issue #166) rather than scanning
    /// the global counter, so cost scales with the number of distributions
    /// for that asset, not the total distribution count.  Includes both
    /// open and completed distributions.
    ///
    /// # Parameters
    /// - `asset_token`: Asset-token contract address to query.
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// None.
    ///
    /// # Events
    /// None.
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

    /// Returns `true` if `holder` has already claimed `distribution_id`.
    ///
    /// # Parameters
    /// - `distribution_id`: Id of the distribution to query.
    /// - `holder`: Address of the holder to check.
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// None.
    ///
    /// # Events
    /// None.
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

    /// Returns the configured admin address.
    ///
    /// # Parameters
    /// None.
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    ///
    /// # Events
    /// None.
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

    /// Cancel a pending admin proposal.
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    /// - [`Error::NoPendingAdmin`] — no proposal is currently in flight.
    ///
    /// # Events
    /// Emits topic `("cancelled", admin)` with data `()`.
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

    /// Accept a pending admin proposal and complete the handover.
    ///
    /// Must be called by the proposed successor (`new_admin`); the role only
    /// ever moves here, never in [`Self::propose_admin`] (issue #4).
    /// Emits `set_admin` carrying both the previous and new admin so
    /// off-chain indexers can observe this security-critical transition
    /// (issue #2).
    ///
    /// # Parameters
    /// - `new_admin`: The address accepting the admin role.  Must match the
    ///   pending proposal and must sign the transaction.
    ///
    /// # Authority
    /// `new_admin` must sign the transaction (`new_admin.require_auth()`).
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::NoPendingAdmin`] — no proposal is currently in flight.
    /// - [`Error::Unauthorized`] — `new_admin` does not match the pending
    ///   proposal.
    ///
    /// # Events
    /// Emits topic `("set_admin", old_admin)` with data `new_admin`.
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

    /// Returns the address currently proposed as the next admin, if any.
    ///
    /// Returns `None` when no proposal is in flight.
    ///
    /// # Parameters
    /// None.
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// None.
    ///
    /// # Events
    /// None.
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
