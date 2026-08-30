#![no_std]
//! # Dividend Contract
//!
//! Distributes yield/dividends to asset-token holders in proportion to their
//! holdings. An issuer funds a distribution with a payment token; each holder
//! then claims `total_amount * balance / total_supply`, paid from the escrow
//! this contract holds. Each holder can claim a given distribution once.
//!
//! Balances are read at claim time from the asset token; there is no balance
//! snapshot. `created_at` records the ledger at which the distribution was
//! created for reference.

use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, symbol_short, Address,
    BytesN, Env, Vec,
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
    /// double init
    AlreadyInitialized = 1,
    /// used before init
    NotInitialized = 2,
    /// non-admin create
    Unauthorized = 3,
    /// unknown distribution id
    DistributionNotFound = 4,
    /// `total_amount <= 0`
    InvalidAmount = 5,
    /// claimable is zero
    NothingToClaim = 6,
    /// holder already claimed this distribution
    AlreadyClaimed = 7,
    /// `asset_token` has zero total supply; no holder can ever claim (issue #49).
    ZeroSupply = 8,
    /// Total distributed would exceed the distribution's `total_amount` (issue #164).
    OverDistributed = 9,
    /// `total_amount * balance` would overflow i128 (issue #165).
    ArithmeticOverflow = 10,
}

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Contract ABI/behavior version. Bump on any change to storage layout or
/// externally observable behavior so clients and the indexer can detect it.
pub const VERSION: u32 = 3;

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
    pub fn create_distribution(
        env: Env,
        admin: Address,
        asset_token: Address,
        payment_token: Address,
        total_amount: i128,
        eligible: Vec<(Address, i128)>,
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

        // Freeze the eligible-holder snapshot and its total (issue #163): every
        // holder's share is sized against this frozen list, not against the
        // asset token's live balances, so a post-creation transfer can neither
        // inflate nor dilute anyone's entitlement.
        let mut snapshot_supply: i128 = 0;
        for (_, balance) in eligible.iter() {
            snapshot_supply = snapshot_supply
                .checked_add(balance)
                .unwrap_or_else(|| panic_err(&env, Error::ArithmeticOverflow));
        }
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
        // Proportional share, floored by integer division. Guard the
        // multiplication against i128 overflow (issue #165).
        dist
            .total_amount
            .checked_mul(basis)
            .unwrap_or_else(|| panic_err(&env, Error::ArithmeticOverflow))
            / supply
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
        if dist.distributed > dist.total_amount {
            panic_err(&env, Error::OverDistributed);
        }
        if dist.distributed >= dist.total_amount {
            dist.completed = true;
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

    /// Deploy new Wasm bytecode to this contract instance in place, keeping the
    /// same contract id and storage. Admin only. See `deploy.sh --upgrade`.
    ///
    /// The storage layout (`DataKey`) must stay compatible with the previous
    /// version, or existing data becomes unreadable; bump `VERSION` when it
    /// changes.
    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) {
        Self::require_admin(&env, &admin);
        env.deployer().update_current_contract_wasm(new_wasm_hash);
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
