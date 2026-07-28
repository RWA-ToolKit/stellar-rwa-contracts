#![no_std]
//! # Dividend Contract
//!
//! Distributes yield/dividends to asset-token holders in proportion to their
//! holdings at a fixed snapshot. An issuer funds a distribution with a payment
//! token and supplies the holder balances it is sized against; each holder then
//! claims `total_amount * snapshot_balance / snapshot_supply`, paid from the
//! escrow this contract holds. Each holder can claim a given distribution once.
//!
//! Shares are computed against the snapshot taken at creation, never against
//! live balances, so moving tokens after creation cannot mint new claims.

use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, symbol_short, Address,
    Env, Vec,
};

/// Read-only view of the asset token needed to size a holder's share.
#[contractclient(name = "AssetClient")]
pub trait AssetInterface {
    fn balance(env: Env, id: Address) -> i128;
    fn total_supply(env: Env) -> i128;
}

/// Minimal payment-token interface used to move escrowed funds.
#[contractclient(name = "TokenClient")]
pub trait TokenInterface {
    fn transfer(env: Env, from: Address, to: Address, amount: i128);
}

/// One holder's balance at the moment a distribution was snapshotted.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotEntry {
    pub holder: Address,
    pub balance: i128,
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
    /// Sum of the snapshotted balances; the denominator for every share.
    pub snapshot_supply: i128,
    /// Sum of every holder's floored share. `total_amount - allocated` is the
    /// rounding dust no holder can ever claim.
    pub allocated: i128,
    pub snapshot_ledger: u32,
    pub created_at: u32,
    pub completed: bool,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Counter,
    Ids,
    Dist(u64),
    Claimed(u64, Address),
    Snapshot(u64, Address),
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
    InvalidSnapshot = 8,
    ClaimWindowOpen = 9,
    NothingToReclaim = 10,
    DistributionClosed = 11,
}

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// How long holders have to claim before the admin may sweep the escrow. Kept
/// below `INSTANCE_BUMP_AMOUNT` so the contract instance is still live when the
/// window closes.
const CLAIM_WINDOW_LEDGERS: u32 = 14 * DAY_IN_LEDGERS;

/// Distributions, snapshots and claim markers are bumped well past the claim
/// window. A claim marker that expired before its distribution would make
/// `has_claimed` read `false` again and let the holder claim a second time, so
/// these must always outlive every other entry involved in a claim.
const PERSISTENT_BUMP_AMOUNT: u32 = 60 * DAY_IN_LEDGERS;
const PERSISTENT_LIFETIME_THRESHOLD: u32 = PERSISTENT_BUMP_AMOUNT - DAY_IN_LEDGERS;

#[contract]
pub struct DividendContract;

#[contractimpl]
impl DividendContract {
    /// Initialize with an admin. Callable once.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_err(&env, Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Counter, &0u64);
        env.storage()
            .instance()
            .set(&DataKey::Ids, &Vec::<u64>::new(&env));
        bump(&env);
        env.events().publish((symbol_short!("init"),), admin);
    }

    /// Create and fund a distribution against a fixed holder snapshot. Pulls
    /// `total_amount` of `payment_token` from the admin into this contract's
    /// escrow. Admin only.
    ///
    /// A Soroban contract cannot enumerate a token's holders on-chain, so the
    /// snapshot is supplied by the admin at creation rather than derived here.
    /// Every share is fixed from that moment, which is what stops a holder from
    /// claiming, moving the tokens on, and claiming again from the new address.
    /// The snapshot must fit in a single transaction; very large holder sets
    /// should be paid out over several distributions.
    pub fn create_distribution(
        env: Env,
        admin: Address,
        asset_token: Address,
        payment_token: Address,
        total_amount: i128,
        snapshot: Vec<SnapshotEntry>,
    ) -> u64 {
        Self::require_admin(&env, &admin);
        if total_amount <= 0 {
            panic_err(&env, Error::InvalidAmount);
        }
        if snapshot.is_empty() {
            panic_err(&env, Error::InvalidSnapshot);
        }

        let id: u64 = env.storage().instance().get(&DataKey::Counter).unwrap_or(0) + 1;

        // Total the snapshotted balances first. This sum, not the token's live
        // supply, is the denominator, so the shares can never add up to more
        // than `total_amount` no matter how partial the snapshot is.
        let mut snapshot_supply: i128 = 0;
        for entry in snapshot.iter() {
            if entry.balance <= 0 {
                panic_err(&env, Error::InvalidSnapshot);
            }
            snapshot_supply += entry.balance;
        }
        // A snapshot may not claim more tokens than the asset actually has.
        if snapshot_supply > AssetClient::new(&env, &asset_token).total_supply() {
            panic_err(&env, Error::InvalidSnapshot);
        }

        // Record each holder's snapshotted balance and total the floored shares
        // so the distribution knows the point at which it is fully claimed.
        let mut allocated: i128 = 0;
        for entry in snapshot.iter() {
            let key = DataKey::Snapshot(id, entry.holder.clone());
            if env.storage().persistent().has(&key) {
                panic_err(&env, Error::InvalidSnapshot); // duplicate holder
            }
            env.storage().persistent().set(&key, &entry.balance);
            bump_persistent(&env, &key);
            allocated += (total_amount * entry.balance) / snapshot_supply;
        }

        // Escrow the funds in this contract.
        let this = env.current_contract_address();
        TokenClient::new(&env, &payment_token).transfer(&admin, &this, &total_amount);

        let dist = Distribution {
            id,
            asset_token,
            payment_token,
            total_amount,
            distributed: 0,
            snapshot_supply,
            allocated,
            snapshot_ledger: env.ledger().sequence(),
            created_at: env.ledger().sequence(),
            // Every share floored to zero, so there is nothing to claim: the
            // distribution is closed on arrival and the admin can reclaim it.
            completed: allocated == 0,
        };
        env.storage().persistent().set(&DataKey::Dist(id), &dist);
        bump_persistent(&env, &DataKey::Dist(id));
        env.storage().instance().set(&DataKey::Counter, &id);
        let mut ids: Vec<u64> = env
            .storage()
            .instance()
            .get(&DataKey::Ids)
            .unwrap_or_else(|| Vec::new(&env));
        ids.push_back(id);
        env.storage().instance().set(&DataKey::Ids, &ids);
        bump(&env);
        env.events()
            .publish((symbol_short!("created"), admin), (id, total_amount));
        id
    }

    /// Amount a holder can still claim from a distribution (0 if already
    /// claimed, not in the snapshot, or the distribution is closed).
    pub fn claimable(env: Env, distribution_id: u64, holder: Address) -> i128 {
        let dist = Self::load(&env, distribution_id);
        if dist.completed || dist.snapshot_supply <= 0 {
            return 0;
        }
        if Self::has_claimed(env.clone(), distribution_id, holder.clone()) {
            return 0;
        }
        let balance = Self::snapshot_balance(env.clone(), distribution_id, holder);
        if balance <= 0 {
            return 0;
        }
        // Proportional share of the snapshot, floored by integer division.
        (dist.total_amount * balance) / dist.snapshot_supply
    }

    /// Claim a holder's proportional share, paid from escrow. Holder-authorized.
    pub fn claim(env: Env, distribution_id: u64, holder: Address) {
        holder.require_auth();
        let mut dist = Self::load(&env, distribution_id);
        if dist.completed {
            panic_err(&env, Error::DistributionClosed);
        }
        if Self::has_claimed(env.clone(), distribution_id, holder.clone()) {
            panic_err(&env, Error::AlreadyClaimed);
        }
        let amount = Self::claimable(env.clone(), distribution_id, holder.clone());
        if amount <= 0 {
            panic_err(&env, Error::NothingToClaim);
        }
        // The claim marker is the only thing standing between this holder and a
        // second payout, so it is bumped on every write and outlives the
        // distribution entry it guards.
        let claimed_key = DataKey::Claimed(distribution_id, holder.clone());
        env.storage().persistent().set(&claimed_key, &true);
        bump_persistent(&env, &claimed_key);

        let this = env.current_contract_address();
        TokenClient::new(&env, &dist.payment_token).transfer(&this, &holder, &amount);

        dist.distributed += amount;
        // Compare against `allocated`, not `total_amount`: the floored shares
        // never sum to the full amount, so this would otherwise stay false
        // forever and strand the rounding dust.
        if dist.distributed >= dist.allocated {
            dist.completed = true;
        }
        env.storage()
            .persistent()
            .set(&DataKey::Dist(distribution_id), &dist);
        bump_persistent(&env, &DataKey::Dist(distribution_id));
        bump(&env);
        env.events()
            .publish((symbol_short!("claim"), holder), (distribution_id, amount));
    }

    /// Sweep whatever is left in escrow back to the admin and close the
    /// distribution. Admin only. Allowed once every allocated share has been
    /// claimed (so only rounding dust remains) or once the claim window has
    /// elapsed. Returns the amount swept.
    pub fn reclaim_unclaimed(env: Env, admin: Address, distribution_id: u64) -> i128 {
        Self::require_admin(&env, &admin);
        let mut dist = Self::load(&env, distribution_id);
        let window_closed =
            env.ledger().sequence() >= dist.created_at.saturating_add(CLAIM_WINDOW_LEDGERS);
        if !dist.completed && !window_closed {
            panic_err(&env, Error::ClaimWindowOpen);
        }
        let remaining = dist.total_amount - dist.distributed;
        if remaining <= 0 {
            panic_err(&env, Error::NothingToReclaim);
        }

        let this = env.current_contract_address();
        TokenClient::new(&env, &dist.payment_token).transfer(&this, &admin, &remaining);

        dist.distributed = dist.total_amount;
        dist.completed = true;
        env.storage()
            .persistent()
            .set(&DataKey::Dist(distribution_id), &dist);
        bump_persistent(&env, &DataKey::Dist(distribution_id));
        bump(&env);
        env.events().publish(
            (symbol_short!("reclaim"), admin),
            (distribution_id, remaining),
        );
        remaining
    }

    /// A holder's balance as recorded in a distribution's snapshot (0 if the
    /// holder was not included).
    pub fn snapshot_balance(env: Env, distribution_id: u64, holder: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Snapshot(distribution_id, holder))
            .unwrap_or(0)
    }

    /// Fetch a distribution by id.
    pub fn get_distribution(env: Env, distribution_id: u64) -> Distribution {
        Self::load(&env, distribution_id)
    }

    /// All distributions created for a given asset token.
    pub fn get_distributions_for_asset(env: Env, asset_token: Address) -> Vec<Distribution> {
        let ids: Vec<u64> = env
            .storage()
            .instance()
            .get(&DataKey::Ids)
            .unwrap_or_else(|| Vec::new(&env));
        let mut out = Vec::new(&env);
        for id in ids.iter() {
            if let Some(d) = env
                .storage()
                .persistent()
                .get::<DataKey, Distribution>(&DataKey::Dist(id))
            {
                if d.asset_token == asset_token {
                    out.push_back(d);
                }
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

    /// Configured admin.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_err(&env, Error::NotInitialized))
    }

    // ---- internal helpers ----

    fn load(env: &Env, id: u64) -> Distribution {
        env.storage()
            .persistent()
            .get(&DataKey::Dist(id))
            .unwrap_or_else(|| panic_err(env, Error::DistributionNotFound))
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

fn bump_persistent(env: &Env, key: &DataKey) {
    env.storage().persistent().extend_ttl(
        key,
        PERSISTENT_LIFETIME_THRESHOLD,
        PERSISTENT_BUMP_AMOUNT,
    );
}

fn panic_err(env: &Env, error: Error) -> ! {
    soroban_sdk::panic_with_error!(env, error)
}

#[cfg(test)]
mod test;
