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

// NOTE: The `Ids` instance-storage key is retained in the enum only for
// forward-compatibility reads of contracts already deployed; it is no longer
// written. New registrations are enumerated via the monotonic Counter alone.

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
    ActiveCount,
    IssuerIndex(Address),
    TypeIndex(String),
    TotalValuation,
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
    Overflow = 6,
    InvalidInput = 7,
}

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Contract ABI/behavior version. Bump on any change to storage layout or
/// externally observable behavior so clients and the indexer can detect it.
pub const VERSION: u32 = 1;

#[contract]
pub struct RegistryContract;

#[contractimpl]
impl RegistryContract {
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
        env.storage().instance().set(&DataKey::ActiveCount, &0u64);
        env.storage()
            .instance()
            .set(&DataKey::TotalValuation, &0i128);
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
        // Require non-empty name and a recognised asset type (issue #48).
        if name.len() == 0 {
            panic_err(&env, Error::InvalidInput);
        }
        validate_asset_type(&env, &asset_type);
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
        env.storage().persistent().extend_ttl(
            &DataKey::Asset(id),
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
        env.storage().instance().set(&DataKey::Counter, &id);
        let active_count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ActiveCount)
            .unwrap_or(0)
            + 1;
        env.storage()
            .instance()
            .set(&DataKey::ActiveCount, &active_count);

        let issuer_key = DataKey::IssuerIndex(entry.issuer.clone());
        let mut issuer_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&issuer_key)
            .unwrap_or_else(|| Vec::new(&env));
        issuer_ids.push_back(id);
        env.storage().persistent().set(&issuer_key, &issuer_ids);
        env.storage().persistent().extend_ttl(
            &issuer_key,
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );

        let type_key = DataKey::TypeIndex(entry.asset_type.clone());
        let mut type_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&type_key)
            .unwrap_or_else(|| Vec::new(&env));
        type_ids.push_back(id);
        env.storage().persistent().set(&type_key, &type_ids);
        env.storage().persistent().extend_ttl(
            &type_key,
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );

        let tvl: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalValuation)
            .unwrap_or(0);
        let new_tvl = tvl
            .checked_add(entry.valuation)
            .unwrap_or_else(|| panic_err(&env, Error::Overflow));
        env.storage()
            .instance()
            .set(&DataKey::TotalValuation, &new_tvl);

        bump(&env);
        env.events()
            .publish((symbol_short!("register"), issuer), id);
        id
    }

    /// Fetch a single asset by id.
    pub fn get_asset(env: Env, asset_id: u64) -> AssetEntry {
        let entry = env
            .storage()
            .persistent()
            .get(&DataKey::Asset(asset_id))
            .unwrap_or_else(|| panic_err(&env, Error::AssetNotFound));
        env.storage().persistent().extend_ttl(
            &DataKey::Asset(asset_id),
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
        entry
    }

    /// All assets registered by a given issuer. Backed by a per-issuer index,
    /// so cost scales with that issuer's asset count, not the whole registry.
    /// Note: This includes both active and deactivated assets. Deactivated assets
    /// are never removed from the index; use the `active` field to filter if needed.
    pub fn get_assets_by_issuer(env: Env, issuer: Address) -> Vec<AssetEntry> {
        let ids = Self::index_ids(&env, &DataKey::IssuerIndex(issuer));
        Self::fetch_assets(&env, &ids)
    }

    /// All assets of a given asset type (e.g. "real_estate"). Backed by a
    /// per-type index, so cost scales with that type's asset count, not the
    /// whole registry.
    /// Note: This includes both active and deactivated assets. Deactivated assets
    /// are never removed from the index; use the `active` field to filter if needed.
    pub fn get_assets_by_type(env: Env, asset_type: String) -> Vec<AssetEntry> {
        let ids = Self::index_ids(&env, &DataKey::TypeIndex(asset_type));
        Self::fetch_assets(&env, &ids)
    }

    /// A page of registered assets: ids `[start_id, start_id + limit)`,
    /// capped at the current counter. Page through the full set by calling
    /// again with `start_id + limit`. Bounds per-call cost regardless of how
    /// many assets have been registered.
    pub fn get_all_assets(env: Env, start_id: u64, limit: u32) -> Vec<AssetEntry> {
        let counter: u64 = env.storage().instance().get(&DataKey::Counter).unwrap_or(0);
        let mut out = Vec::new(&env);
        let start = start_id.max(1);
        let end = start.saturating_add(limit as u64).min(counter + 1);
        let mut id = start;
        while id < end {
            if let Some(entry) = env.storage().persistent().get(&DataKey::Asset(id)) {
                env.storage().persistent().extend_ttl(
                    &DataKey::Asset(id),
                    INSTANCE_LIFETIME_THRESHOLD,
                    INSTANCE_BUMP_AMOUNT,
                );
                out.push_back(entry);
            }
            id += 1;
        }
        out
    }

    /// Deactivate an asset. Admin only. Excluded from TVL afterwards.
    pub fn deactivate_asset(env: Env, admin: Address, asset_id: u64) {
        Self::require_admin(&env, &admin);
        let mut entry: AssetEntry = env
            .storage()
            .persistent()
            .get(&DataKey::Asset(asset_id))
            .unwrap_or_else(|| panic_err(&env, Error::AssetNotFound));
        let was_active = entry.active;
        entry.active = false;
        env.storage()
            .persistent()
            .set(&DataKey::Asset(asset_id), &entry);
        env.storage().persistent().extend_ttl(
            &DataKey::Asset(asset_id),
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
        if was_active {
            let active_count: u64 = env
                .storage()
                .instance()
                .get(&DataKey::ActiveCount)
                .unwrap_or(0u64)
                .saturating_sub(1);
            env.storage()
                .instance()
                .set(&DataKey::ActiveCount, &active_count);
            let tvl: i128 = env
                .storage()
                .instance()
                .get(&DataKey::TotalValuation)
                .unwrap_or(0);
            let new_tvl = tvl
                .checked_sub(entry.valuation)
                .unwrap_or_else(|| panic_err(&env, Error::Overflow));
            env.storage()
                .instance()
                .set(&DataKey::TotalValuation, &new_tvl);
        }
        bump(&env);
        env.events()
            .publish((symbol_short!("deactvate"),), asset_id);
    }

    /// Sum of valuations across all active assets, in USD cents. Maintained
    /// incrementally on register/deactivate, so this is a single read
    /// regardless of registry size.
    pub fn total_value_locked(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalValuation)
            .unwrap_or(0)
    }

    /// Number of registered assets (active or not).
    pub fn asset_count(env: Env) -> u64 {
        env.storage().instance().get(&DataKey::Counter).unwrap_or(0)
    }

    /// Number of assets that are currently active (excludes deactivated ones).
    pub fn active_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::ActiveCount)
            .unwrap_or(0)
    }

    /// Configured admin.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_err(&env, Error::NotInitialized))
    }

    // ---- internal helpers ----

    /// Read an id index (issuer/type), extending its TTL if present.
    fn index_ids(env: &Env, key: &DataKey) -> Vec<u64> {
        if let Some(ids) = env.storage().persistent().get(key) {
            env.storage().persistent().extend_ttl(
                key,
                INSTANCE_LIFETIME_THRESHOLD,
                INSTANCE_BUMP_AMOUNT,
            );
            ids
        } else {
            Vec::new(env)
        }
    }

    /// Resolve a list of asset ids to their entries, extending each TTL.
    fn fetch_assets(env: &Env, ids: &Vec<u64>) -> Vec<AssetEntry> {
        let mut out = Vec::new(env);
        for id in ids.iter() {
            if let Some(entry) = env.storage().persistent().get(&DataKey::Asset(id)) {
                env.storage().persistent().extend_ttl(
                    &DataKey::Asset(id),
                    INSTANCE_LIFETIME_THRESHOLD,
                    INSTANCE_BUMP_AMOUNT,
                );
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

/// Allowed asset types (issue #48). Any type outside this list is rejected.
const VALID_ASSET_TYPES: &[&str] = &[
    "real_estate",
    "invoice",
    "commodity",
    "bond",
    "equity",
    "fund",
];

fn validate_asset_type(env: &Env, asset_type: &String) {
    let bytes = asset_type.to_bytes();
    for &valid in VALID_ASSET_TYPES {
        let v = valid.as_bytes();
        if bytes.len() as usize == v.len() {
            let mut matches = true;
            for i in 0..v.len() {
                if bytes.get(i as u32).unwrap_or(0) != v[i] {
                    matches = false;
                    break;
                }
            }
            if matches {
                return;
            }
        }
    }
    panic_err(env, Error::InvalidInput);
}

#[cfg(test)]
mod test;
