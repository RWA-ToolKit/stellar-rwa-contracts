#![cfg_attr(not(test), no_std)]
//! # Dividend Contract
//!
//! Distributes yield/dividends to asset-token holders in proportion to their
//! holdings. An issuer funds a distribution with a payment token; each holder
//! then claims `total_amount * balance / total_supply`, paid from the escrow
//! this contract holds. Each holder can claim a given distribution once.
//!
//! Balances are read at claim time from the asset token. `snapshot_ledger`
//! records the ledger at which the distribution was created for reference.

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

/// A single dividend distribution.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Distribution {
    pub id: u64,
    pub asset_token: Address,
    pub payment_token: Address,
    pub total_amount: i128,
    pub distributed: i128,
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
}

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

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
    pub fn create_distribution(
        env: Env,
        admin: Address,
        asset_token: Address,
        payment_token: Address,
        total_amount: i128,
    ) -> u64 {
        Self::require_admin(&env, &admin);
        if total_amount <= 0 {
            panic_err(&env, Error::InvalidAmount);
        }
        // Reject distributions where no holder can ever claim (issue #49).
        let supply = AssetClient::new(&env, &asset_token).total_supply();
        if supply <= 0 {
            panic_err(&env, Error::ZeroSupply);
        }
        // Escrow the funds in this contract.
        let this = env.current_contract_address();
        TokenClient::new(&env, &payment_token).transfer(&admin, &this, &total_amount);

        let id: u64 = env.storage().instance().get(&DataKey::Counter).unwrap_or(0) + 1;
        let dist = Distribution {
            id,
            asset_token,
            payment_token,
            total_amount,
            distributed: 0,
            snapshot_ledger: env.ledger().sequence(),
            created_at: env.ledger().sequence(),
            completed: false,
        };
        env.storage().persistent().set(&DataKey::Dist(id), &dist);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Dist(id), INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
        env.storage().instance().set(&DataKey::Counter, &id);
        bump(&env);
        env.events()
            .publish((symbol_short!("created"), admin), (id, total_amount));
        id
    }

    /// Amount a holder can still claim from a distribution (0 if already
    /// claimed, holds nothing, or the distribution is empty).
    pub fn claimable(env: Env, distribution_id: u64, holder: Address) -> i128 {
        let dist = Self::load(&env, distribution_id);
        if Self::has_claimed(env.clone(), distribution_id, holder.clone()) {
            return 0;
        }
        let asset = AssetClient::new(&env, &dist.asset_token);
        let supply = asset.total_supply();
        if supply <= 0 {
            return 0;
        }
        let balance = asset.balance(&holder);
        if balance <= 0 {
            return 0;
        }
        proportional_share(dist.total_amount, balance, supply)
    }

    /// Claim a holder's proportional share, paid from escrow. Holder-authorized.
    pub fn claim(env: Env, distribution_id: u64, holder: Address) {
        holder.require_auth();
        let mut dist = Self::load(&env, distribution_id);
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
            .unwrap_or_else(|| panic_err(&env, Error::InvalidAmount));
        assert!(dist.distributed <= dist.total_amount);
        if dist.distributed >= dist.total_amount {
            dist.completed = true;
        }
        env.storage()
            .persistent()
            .set(&DataKey::Dist(distribution_id), &dist);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Dist(distribution_id), INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
        bump(&env);
        env.events()
            .publish((symbol_short!("claim"), holder), (distribution_id, amount));
    }

    /// Fetch a distribution by id.
    pub fn get_distribution(env: Env, distribution_id: u64) -> Distribution {
        Self::load(&env, distribution_id)
    }

    /// All distributions created for a given asset token.
    /// Iterates via the monotonic Counter so the global Ids vector is never
    /// re-serialised; per-id keys are O(1) reads.
    pub fn get_distributions_for_asset(env: Env, asset_token: Address) -> Vec<Distribution> {
        let counter: u64 = env
            .storage()
            .instance()
            .get(&DataKey::Counter)
            .unwrap_or(0);
        let mut out = Vec::new(&env);
        for id in 1..=counter {
            if let Some(d) = env
                .storage()
                .persistent()
                .get::<DataKey, Distribution>(&DataKey::Dist(id))
            {
                env.storage()
                    .persistent()
                    .extend_ttl(&DataKey::Dist(id), INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
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
        let dist = env.storage()
            .persistent()
            .get(&DataKey::Dist(id))
            .unwrap_or_else(|| panic_err(env, Error::DistributionNotFound));
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Dist(id), INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
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

/// Proportional share, floored by integer division: `total_amount * balance / supply`.
/// Pulled out as a standalone pure function so its invariants (no overflow
/// for bounded inputs, sum-of-shares across holders <= total_amount) can be
/// property-tested directly (issue #109).
fn proportional_share(total_amount: i128, balance: i128, supply: i128) -> i128 {
    (total_amount * balance) / supply
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

#[cfg(test)]
mod proptest_tests;
