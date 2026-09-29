#![no_std]
//! # Registry Contract
//!
//! A canonical, on-chain index of every tokenized asset on the platform. Each
//! issuer registers their asset-token contract here; the registry assigns a
//! monotonically increasing id and tracks issuer, type, valuation and active
//! status. It also reports total value locked (TVL) across active assets.

use asset_token::AssetMetadata;
use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, symbol_short, Address,
    Env, String, Vec,
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
    /// Address nominated by the current admin via `propose_admin`, pending
    /// acceptance via `accept_admin` (issue #4). Absent when there is no
    /// proposal in flight.
    PendingAdmin,
    Counter,
    Ids,
    Asset(u64),
    ActiveCount,
    IssuerIndex(Address),
    TypeIndex(String),
    TotalValuation,
    /// Reverse index from token contract address to its registered asset id,
    /// used to reject double-registration of the same token contract
    /// (issue #308: duplicate registration would double-count in TVL).
    TokenContractIndex(Address),
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
    /// `accept_admin` or `cancel_admin_proposal` called with no pending
    /// admin proposal on file (issue #4).
    NoPendingAdmin = 8,
    /// A token contract is already registered under a different asset id.
    DuplicateAsset = 9,
}

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Maximum number of assets `get_all_assets` will return in a single call,
/// regardless of the requested `limit` (issue #310). Callers that need more
/// must page through with successive calls using the returned count to
/// compute the next `start_id`.
pub const MAX_PAGE_SIZE: u32 = 100;

/// Maximum byte length for an asset name (issue #439).
///
/// Matches the cap enforced by the asset-token contract's `check_str` helper
/// so the two layers are consistent. The web wizard already limits the name
/// field to 64 characters in Step2AssetDetails; names containing multi-byte
/// UTF-8 characters count toward this limit by **bytes**, not code-points,
/// so a name consisting of 32 two-byte characters (e.g. é, ñ) reaches the
/// limit with only 32 visible glyphs.
pub const MAX_NAME_LEN: u32 = 64;

/// Contract ABI/behavior version. Bump on any change to storage layout or
/// externally observable behavior so clients and the indexer can detect it.
pub const VERSION: u32 = 3;

#[contractclient(name = "AssetClient")]
pub trait AssetInterface {
    fn get_metadata(env: Env) -> AssetMetadata;
}

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
    ///
    /// `valuation` is copied into the registry entry and TVL at registration
    /// time. It is independent from any valuation stored by the token
    /// contract: the registry has no callback or synchronization hook when a
    /// token updates its metadata valuation. Divergence is therefore possible
    /// and callers must verify both records; the current registry API has no
    /// valuation-update entrypoint.
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
        // Reject names longer than MAX_NAME_LEN bytes (issue #439).
        // The asset-token contract enforces the same 64-byte cap; keeping
        // them in sync prevents unbounded ledger entries that inflate the
        // cost of every registry read and break UI card layout.
        if name.len() > MAX_NAME_LEN {
            panic_err(&env, Error::InvalidInput);
        }
        validate_asset_type(&env, &asset_type);
        // Reject re-registering the same token contract under a new id
        // (issue #308): otherwise TVL and the explore page would double-count
        // the same underlying asset.
        let token_index_key = DataKey::TokenContractIndex(token_contract.clone());
        if env.storage().persistent().has(&token_index_key) {
            panic_err(&env, Error::DuplicateAsset);
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
        env.storage().persistent().extend_ttl(
            &DataKey::Asset(id),
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
        env.storage().persistent().set(&token_index_key, &id);
        env.storage().persistent().extend_ttl(
            &token_index_key,
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
    ///
    /// Matching is **byte-exact**: the index key is the `asset_type` string as
    /// stored on the entry at registration time, so lookups are case-sensitive
    /// and whitespace-sensitive. `"real_estate"`, `"Real_Estate"` and
    /// `"real_estate "` are three distinct index keys; since only the values
    /// in `VALID_ASSET_TYPES` can ever be registered (see
    /// `validate_asset_type`), a query must match one of those canonical
    /// strings exactly to return any results.
    pub fn get_assets_by_type(env: Env, asset_type: String) -> Vec<AssetEntry> {
        let ids = Self::index_ids(&env, &DataKey::TypeIndex(asset_type));
        Self::fetch_assets(&env, &ids)
    }

    /// A page of registered assets: ids `[start_id, start_id + limit)`,
    /// capped at the current counter. Page through the full set by calling
    /// again with `start_id + limit`. Bounds per-call cost regardless of how
    /// many assets have been registered.
    ///
    /// `limit` is silently clamped to [`MAX_PAGE_SIZE`] (issue #310) so a
    /// misbehaving or malicious caller cannot force an unbounded response;
    /// small registries that request the whole set in one call (e.g.
    /// `start_id = 1, limit = u32::MAX`) keep working exactly as before as
    /// long as they fit under the cap.
    pub fn get_all_assets(env: Env, start_id: u64, limit: u32) -> Vec<AssetEntry> {
        let counter: u64 = env.storage().instance().get(&DataKey::Counter).unwrap_or(0);
        let mut out = Vec::new(&env);
        let start = start_id.max(1);
        let capped_limit = limit.min(MAX_PAGE_SIZE);
        let end = start.saturating_add(capped_limit as u64).min(counter + 1);
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
    /// Does nothing if the asset is already inactive (no event emitted).
    pub fn deactivate_asset(env: Env, admin: Address, asset_id: u64) {
        let mut entry: AssetEntry = env
            .storage()
            .persistent()
            .get(&DataKey::Asset(asset_id))
            .unwrap_or_else(|| panic_err(&env, Error::AssetNotFound));
        Self::require_admin_or_asset_admin(&env, &admin, &entry.token_contract);
        let was_active = entry.active;
        if !was_active {
            return;
        }
        entry.active = false;
        env.storage()
            .persistent()
            .set(&DataKey::Asset(asset_id), &entry);
        env.storage().persistent().extend_ttl(
            &DataKey::Asset(asset_id),
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
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
        bump(&env);
        env.events()
            .publish((symbol_short!("deactvate"),), asset_id);
    }

    /// Update an asset's valuation. Admin only. Adjusts total value locked
    /// accordingly (only while the asset is `active`) and emits a
    /// `valuation` event carrying the asset id and both the old and new
    /// valuations, so indexers can observe the change without polling
    /// (issue #3).
    pub fn update_valuation(env: Env, admin: Address, asset_id: u64, new_valuation: i128) {
        Self::require_admin(&env, &admin);
        if new_valuation < 0 {
            panic_err(&env, Error::InvalidValuation);
        }
        let mut entry: AssetEntry = env
            .storage()
            .persistent()
            .get(&DataKey::Asset(asset_id))
            .unwrap_or_else(|| panic_err(&env, Error::AssetNotFound));
        let old_valuation = entry.valuation;
        entry.valuation = new_valuation;
        env.storage()
            .persistent()
            .set(&DataKey::Asset(asset_id), &entry);
        env.storage().persistent().extend_ttl(
            &DataKey::Asset(asset_id),
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );

        if entry.active {
            let tvl: i128 = env
                .storage()
                .instance()
                .get(&DataKey::TotalValuation)
                .unwrap_or(0);
            let new_tvl = tvl
                .checked_sub(old_valuation)
                .and_then(|v| v.checked_add(new_valuation))
                .unwrap_or_else(|| panic_err(&env, Error::Overflow));
            env.storage()
                .instance()
                .set(&DataKey::TotalValuation, &new_tvl);
        }

        bump(&env);
        env.events().publish(
            (symbol_short!("valuation"), asset_id),
            (old_valuation, new_valuation),
        );
    }

    /// Reactivate a previously deactivated asset. Admin only. Included in TVL
    /// and `active_count` again afterwards. Does nothing if the asset is
    /// already active (no event emitted).
    ///
    /// Deactivation is not treated as final: assets are sometimes deactivated
    /// by mistake (wrong id, premature admin action), and re-registering under
    /// a new id would break existing references to the original one (issuer
    /// index, type index, external links). Reactivation restores the same
    /// entry in place instead.
    pub fn reactivate_asset(env: Env, admin: Address, asset_id: u64) {
        let mut entry: AssetEntry = env
            .storage()
            .persistent()
            .get(&DataKey::Asset(asset_id))
            .unwrap_or_else(|| panic_err(&env, Error::AssetNotFound));
        Self::require_admin_or_asset_admin(&env, &admin, &entry.token_contract);
        if entry.active {
            return;
        }
        entry.active = true;
        env.storage()
            .persistent()
            .set(&DataKey::Asset(asset_id), &entry);
        env.storage().persistent().extend_ttl(
            &DataKey::Asset(asset_id),
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
        let active_count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ActiveCount)
            .unwrap_or(0u64)
            + 1;
        env.storage()
            .instance()
            .set(&DataKey::ActiveCount, &active_count);
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
            .publish((symbol_short!("reactvate"),), asset_id);
    }

    /// Sum of valuations across all active assets, in USD cents. Maintained
    /// incrementally on register/deactivate, so this is a single read
    /// regardless of registry size.
    ///
    /// This registry entry's `valuation` is a point-in-time snapshot taken at
    /// `register_asset` and is not synced with the corresponding
    /// asset-token contract's live valuation. The asset-token contract's
    /// `update_valuation` bounds any single change to
    /// `MAX_VALUATION_CHANGE_BPS` (50%) of the previous value precisely
    /// because a runaway or mistyped valuation there would otherwise be able
    /// to skew this TVL figure for every downstream client that reads it.
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

    // ---- pagination helpers (issue #455) ----

    /// Total number of registered assets (active and inactive).
    ///
    /// Equivalent to `asset_count`; provided under this name so callers that
    /// are building paginated UIs can use a uniform naming convention:
    /// `get_total_asset_count` / `get_assets_page` pair naturally together.
    pub fn get_total_asset_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get::<DataKey, u64>(&DataKey::Counter)
            .unwrap_or(0) as u32
    }

    /// Return a page of assets by zero-based index offset.
    ///
    /// `start_index` is a zero-based position in the logical list of all
    /// registered assets ordered by id. `page_size` is capped at
    /// [`MAX_PAGE_SIZE`] (100); passing `0` or a value above the cap returns
    /// up to the cap. An `start_index` at or beyond the total count returns
    /// an empty `Vec`, which signals the end of the list.
    ///
    /// This is a convenient index-based companion to `get_all_assets`, which
    /// uses an id-based range. Both ultimately iterate over the same ordered
    /// sequence of registered ids.
    pub fn get_assets_page(env: Env, start_index: u32, page_size: u32) -> Vec<AssetEntry> {
        let counter: u64 = env.storage().instance().get(&DataKey::Counter).unwrap_or(0);
        if counter == 0 {
            return Vec::new(&env);
        }
        let capped = if page_size == 0 || page_size > MAX_PAGE_SIZE {
            MAX_PAGE_SIZE
        } else {
            page_size
        };
        // Registry ids are 1-based and contiguous (no gaps after deactivation).
        // Convert zero-based start_index to a 1-based id.
        let start_id: u64 = (start_index as u64).saturating_add(1);
        if start_id > counter {
            return Vec::new(&env);
        }
        let mut out = Vec::new(&env);
        let end_id = start_id
            .saturating_add(capped as u64)
            .min(counter.saturating_add(1));
        let mut id = start_id;
        while id < end_id {
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

    /// Return a page of assets filtered by their `active` flag.
    ///
    /// When `active_only` is `true`, only assets whose `active` field is
    /// `true` are included; when `false`, only inactive (deactivated) assets
    /// are returned. This is useful for building paginated "active assets"
    /// or "deactivated assets" views without pulling the whole registry to
    /// the client.
    ///
    /// `start_index` and `page_size` have the same semantics as
    /// `get_assets_page`, but they count positions in the **filtered** list
    /// rather than across all assets. The scan walks the full id range from
    /// 1 up to the current counter, skipping assets that do not match the
    /// filter, and returns at most `page_size` (capped at [`MAX_PAGE_SIZE`])
    /// matching entries starting from `start_index` in the filtered order.
    ///
    /// Because this is a linear scan over all registered ids the cost scales
    /// with the total registry size, not just the matching subset. For large
    /// registries, prefer per-issuer or per-type index queries when possible.
    pub fn get_active_assets_page(
        env: Env,
        start_index: u32,
        page_size: u32,
        active_only: bool,
    ) -> Vec<AssetEntry> {
        let counter: u64 = env.storage().instance().get(&DataKey::Counter).unwrap_or(0);
        if counter == 0 {
            return Vec::new(&env);
        }
        let capped = if page_size == 0 || page_size > MAX_PAGE_SIZE {
            MAX_PAGE_SIZE
        } else {
            page_size
        };
        let mut skipped: u32 = 0;
        let mut out = Vec::new(&env);
        let mut id: u64 = 1;
        while id <= counter && (out.len() as u32) < capped {
            if let Some(entry) = env
                .storage()
                .persistent()
                .get::<DataKey, AssetEntry>(&DataKey::Asset(id))
            {
                if entry.active == active_only {
                    if skipped < start_index {
                        skipped += 1;
                    } else {
                        env.storage().persistent().extend_ttl(
                            &DataKey::Asset(id),
                            INSTANCE_LIFETIME_THRESHOLD,
                            INSTANCE_BUMP_AMOUNT,
                        );
                        out.push_back(entry);
                    }
                }
            }
            id += 1;
        }
        out
    }

    /// Configured admin.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_err(&env, Error::NotInitialized))
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

/// Validates that the given asset_type matches one of the allowed types.
/// This is a **byte-exact comparison** (case- and whitespace-sensitive).
/// For example, "Real_Estate" or "real_estate " will be rejected.
/// Only exact matches like "real_estate" (in lowercase, no leading/trailing whitespace) are accepted.
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
