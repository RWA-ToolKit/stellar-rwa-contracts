#![no_std]
//! # Registry Contract
//!
//! A canonical, on-chain index of every tokenized asset on the platform. Each
//! issuer registers their asset-token contract here; the registry assigns a
//! monotonically increasing id and tracks issuer, type, valuation and active
//! status. It also reports total value locked (TVL) across active assets.
//!
//! ## Security properties
//! * **#10 – permissionless registration closed:** `register_asset` verifies
//!   via a cross-contract call that the caller (`issuer`) is the admin of
//!   `token_contract`.  Any other caller panics with `Unauthorized`.
//! * **#11 – no duplicate tokens:** a `DataKey::ByToken(Address)` reverse
//!   index prevents the same `token_contract` from being registered twice.
//!   A duplicate attempt panics with `DuplicateToken`.
//! * **#12 – valuation stays in sync:** `update_asset_valuation` lets the
//!   issuer **or** the registry admin push a new valuation for an entry at any
//!   time, keeping TVL accurate after the token's own `update_valuation` is
//!   called.

use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, symbol_short, Address,
    Env, String, Vec,
};

// ---------------------------------------------------------------------------
// Cross-contract interface: read the admin of an asset-token contract.
// Only the minimal surface we need is declared here.
// ---------------------------------------------------------------------------

/// Metadata returned by the asset-token's `get_metadata` method.
/// Only the `admin` field is used; the rest are ignored at the call site.
#[contracttype]
#[derive(Clone)]
pub struct AssetMetadata {
    pub name: String,
    pub symbol: String,
    pub asset_type: String,
    pub total_supply: i128,
    pub decimals: u32,
    pub admin: Address,
    pub compliance_contract: Address,
    pub asset_description: String,
    pub valuation: i128,
    pub paused: bool,
}

#[contractclient(name = "AssetTokenClient")]
pub trait AssetTokenInterface {
    fn get_metadata(env: Env) -> AssetMetadata;
}

// ---------------------------------------------------------------------------
// Storage types
// ---------------------------------------------------------------------------

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
    /// Ordered list of all registered asset ids.
    Ids,
    /// Asset entry keyed by its monotonic id.
    Asset(u64),
    /// Reverse index: token_contract address → asset id.
    /// Prevents duplicate registrations (#11).
    ByToken(Address),
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
    /// The same token_contract has already been registered (#11).
    DuplicateToken = 6,
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
        env.storage()
            .instance()
            .set(&DataKey::Ids, &Vec::<u64>::new(&env));
        bump(&env);
        env.events().publish((symbol_short!("init"),), admin);
    }

    /// Register a new tokenized asset.
    ///
    /// The caller must be the **admin of `token_contract`** (verified via a
    /// cross-contract call to `get_metadata`). This closes the permissionless
    /// registration attack described in issue #10.
    ///
    /// Registering the same `token_contract` twice is rejected with
    /// `DuplicateToken` (#11).
    ///
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

        // --- #10: verify issuer == token_contract.admin ---
        // A cross-contract call fetches the metadata from the token and
        // confirms the caller is actually its admin. Any random account that
        // does not control the token will be rejected here.
        let token_meta = AssetTokenClient::new(&env, &token_contract).get_metadata();
        if token_meta.admin != issuer {
            panic_err(&env, Error::Unauthorized);
        }

        // --- #11: reject duplicate token_contract registrations ---
        if env
            .storage()
            .persistent()
            .has(&DataKey::ByToken(token_contract.clone()))
        {
            panic_err(&env, Error::DuplicateToken);
        }

        let id: u64 = env.storage().instance().get(&DataKey::Counter).unwrap_or(0) + 1;
        let entry = AssetEntry {
            id,
            token_contract: token_contract.clone(),
            issuer: issuer.clone(),
            name,
            asset_type,
            valuation,
            created_at: env.ledger().sequence(),
            active: true,
        };

        // Persist the entry and both indexes.
        env.storage().persistent().set(&DataKey::Asset(id), &entry);
        env.storage()
            .persistent()
            .set(&DataKey::ByToken(token_contract), &id);
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
        entry.active = false;
        env.storage()
            .persistent()
            .set(&DataKey::Asset(asset_id), &entry);
        bump(&env);
        env.events()
            .publish((symbol_short!("deactvate"),), asset_id);
    }

    /// Update the recorded valuation for an asset entry (#12).
    ///
    /// Callable by either the **registry admin** or the **issuer** of the
    /// asset (both must authorize). This allows the valuation stored in the
    /// registry — and therefore TVL — to be kept in sync whenever the
    /// asset-token's own `update_valuation` is called.
    pub fn update_asset_valuation(
        env: Env,
        caller: Address,
        asset_id: u64,
        new_valuation: i128,
    ) {
        Self::assert_init(&env);
        caller.require_auth();

        if new_valuation < 0 {
            panic_err(&env, Error::InvalidValuation);
        }

        let mut entry: AssetEntry = env
            .storage()
            .persistent()
            .get(&DataKey::Asset(asset_id))
            .unwrap_or_else(|| panic_err(&env, Error::AssetNotFound));

        // Accept the call only from the registry admin or the asset's issuer.
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_err(&env, Error::NotInitialized));

        if caller != admin && caller != entry.issuer {
            panic_err(&env, Error::Unauthorized);
        }

        entry.valuation = new_valuation;
        env.storage()
            .persistent()
            .set(&DataKey::Asset(asset_id), &entry);
        bump(&env);
        env.events()
            .publish((symbol_short!("updtval"), caller), (asset_id, new_valuation));
    }

    /// Sum of valuations across all active assets, in USD cents.
    pub fn total_value_locked(env: Env) -> i128 {
        let mut tvl: i128 = 0;
        for entry in Self::iter_assets(&env) {
            if entry.active {
                tvl += entry.valuation;
            }
        }
        tvl
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
