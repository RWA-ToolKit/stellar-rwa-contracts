#![no_std]
//! # Registry Contract
//!
//! A canonical, on-chain index of every tokenized asset on the platform. Each
//! issuer registers their asset-token contract here; the registry assigns a
//! monotonically increasing id and tracks issuer, type, valuation and active
//! status. It also reports total value locked (TVL) across active assets.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, String, Vec,
};

/// A single registered asset.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetEntry {
    pub id: u64,
    pub token_contract: Address,
    pub issuer: Address,
    pub name: String,
    pub asset_type: String,
    /// Valuation in USD cents.
    pub valuation: i128,
    /// Ledger sequence at registration.
    pub created_at: u32,
    pub active: bool,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Counter,
    Ids,
    Asset(u64),
    Tvl,
}

#[contracterror]
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    AssetNotFound = 4,
    InvalidValuation = 5,
}

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

#[contract]
pub struct RegistryContract;

#[contractimpl]
impl RegistryContract {
    /// Initialize with an admin. Callable once.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_err(&env, Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Counter, &0u64);
        env.storage().instance().set(&DataKey::Tvl, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::Ids, &Vec::<u64>::new(&env));
        bump(&env);
        env.events().publish((symbol_short!("init"),), admin);
    }

    /// Register a new tokenized asset. The issuer must authorize the call.
    /// Returns the assigned asset id.
    pub fn register_asset(
        env: Env,
        issuer: Address,
        token_contract: Address,
        name: String,
        asset_type: String,
        valuation: i128,
    ) -> u64 {
        Self::assert_init(&env);
        issuer.require_auth();
        if valuation < 0 {
            panic_err(&env, Error::InvalidValuation);
        }
        let id: u64 = env.storage().instance().get(&DataKey::Counter).unwrap_or(0) + 1;
        let entry = AssetEntry {
            id,
            token_contract,
            issuer: issuer.clone(),
            name,
            asset_type,
            valuation,
            created_at: env.ledger().sequence(),
            active: true,
        };
        env.storage().persistent().set(&DataKey::Asset(id), &entry);
        env.storage().instance().set(&DataKey::Counter, &id);
        let mut tvl: i128 = env.storage().instance().get(&DataKey::Tvl).unwrap_or(0);
        tvl += valuation;
        env.storage().instance().set(&DataKey::Tvl, &tvl);
        let mut ids: Vec<u64> = env
            .storage()
            .instance()
            .get(&DataKey::Ids)
            .unwrap_or_else(|| Vec::new(&env));
        ids.push_back(id);
        env.storage().instance().set(&DataKey::Ids, &ids);
        bump(&env);
        env.events()
            .publish((symbol_short!("register"), issuer), id);
        id
    }

    /// Fetch a single asset by id.
    pub fn get_asset(env: Env, asset_id: u64) -> AssetEntry {
        env.storage()
            .persistent()
            .get(&DataKey::Asset(asset_id))
            .unwrap_or_else(|| panic_err(&env, Error::AssetNotFound))
    }

    /// Fetch assets with pagination (start index, maximum limit).
    pub fn get_assets(env: Env, start: u32, limit: u32) -> Vec<AssetEntry> {
        let ids: Vec<u64> = env
            .storage()
            .instance()
            .get(&DataKey::Ids)
            .unwrap_or_else(|| Vec::new(&env));
        let mut out = Vec::new(&env);
        let len = ids.len();
        let end = (start + limit).min(len);
        for i in start..end {
            if let Some(id) = ids.get(i) {
                if let Some(entry) = env.storage().persistent().get(&DataKey::Asset(id)) {
                    out.push_back(entry);
                }
            }
        }
        out
    }

    /// All assets registered by a given issuer.
    pub fn get_assets_by_issuer(env: Env, issuer: Address) -> Vec<AssetEntry> {
        let mut out = Vec::new(&env);
        for entry in Self::iter_assets(&env) {
            if entry.issuer == issuer {
                out.push_back(entry);
            }
        }
        out
    }

    /// All assets of a given asset type (e.g. "real_estate").
    pub fn get_assets_by_type(env: Env, asset_type: String) -> Vec<AssetEntry> {
        let mut out = Vec::new(&env);
        for entry in Self::iter_assets(&env) {
            if entry.asset_type == asset_type {
                out.push_back(entry);
            }
        }
        out
    }

    /// Every registered asset.
    pub fn get_all_assets(env: Env) -> Vec<AssetEntry> {
        Self::iter_assets(&env)
    }

    /// Deactivate an asset. Admin only. Excluded from TVL afterwards.
    pub fn deactivate_asset(env: Env, admin: Address, asset_id: u64) {
        Self::require_admin(&env, &admin);
        let mut entry: AssetEntry = env
            .storage()
            .persistent()
            .get(&DataKey::Asset(asset_id))
            .unwrap_or_else(|| panic_err(&env, Error::AssetNotFound));
        if entry.active {
            entry.active = false;
            let mut tvl: i128 = env.storage().instance().get(&DataKey::Tvl).unwrap_or(0);
            tvl = tvl.saturating_sub(entry.valuation);
            env.storage().instance().set(&DataKey::Tvl, &tvl);
            env.storage()
                .persistent()
                .set(&DataKey::Asset(asset_id), &entry);
        }
        bump(&env);
        env.events()
            .publish((symbol_short!("deactvate"),), asset_id);
    }

    /// Sum of valuations across all active assets, in USD cents. Calculated in O(1) time.
    pub fn total_value_locked(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::Tvl).unwrap_or(0)
    }

    /// Number of registered assets (active or not).
    pub fn asset_count(env: Env) -> u64 {
        env.storage().instance().get(&DataKey::Counter).unwrap_or(0)
    }

    /// Configured admin.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_err(&env, Error::NotInitialized))
    }

    // ---- internal helpers ----

    fn iter_assets(env: &Env) -> Vec<AssetEntry> {
        let ids: Vec<u64> = env
            .storage()
            .instance()
            .get(&DataKey::Ids)
            .unwrap_or_else(|| Vec::new(env));
        let mut out = Vec::new(env);
        for id in ids.iter() {
            if let Some(entry) = env.storage().persistent().get(&DataKey::Asset(id)) {
                out.push_back(entry);
            }
        }
        out
    }

    fn assert_init(env: &Env) {
        if !env.storage().instance().has(&DataKey::Admin) {
            panic_err(env, Error::NotInitialized);
        }
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
