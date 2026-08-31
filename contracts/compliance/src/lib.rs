#![no_std]
#![allow(deprecated)]
//! # Compliance Contract
//!
//! Maintains the KYC allowlist and jurisdiction rules that gate who may hold or
//! transfer a tokenized real-world asset. The asset-token contract performs a
//! cross-contract call into [`ComplianceContract::is_allowed`] on every transfer
//! (for both sender and recipient) and on every mint (for the recipient).
//!
//! Time is expressed in ledger sequence numbers (`u32`), not wall-clock dates.
//! An `expires_at` of `0` means the KYC approval never expires.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, String, Vec,
};

/// Approval state of an address.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComplianceStatus {
    Approved,
    Pending,
    Rejected,
    Suspended,
}

/// A single KYC record for an address.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KycRecord {
    pub address: Address,
    pub status: ComplianceStatus,
    /// ISO country code, e.g. "US", "KE", "DE".
    pub jurisdiction: String,
    /// Ledger sequence at which the record was verified.
    pub verified_at: u32,
    /// Ledger sequence at which approval expires; `0` = never expires.
    pub expires_at: u32,
}

/// Storage keys.
#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    /// Small fixed-size (current_page, current_page_len) cursor for appends.
    AllowlistMeta,
    /// One page of up to `ALLOWLIST_PAGE_SIZE` addresses, in persistent storage
    /// (issue #177): no single entry grows without bound or shares the
    /// instance ledger entry with the rest of the contract's state.
    AllowlistPage(u32),
    /// Which page an address currently lives on, for O(1) removal.
    AllowlistPageOf(Address),
    Record(Address),
    Blocked(String),
}

/// Max addresses per allowlist page (issue #177). Bounds the size of any single
/// storage entry regardless of how large the KYC list grows.
const ALLOWLIST_PAGE_SIZE: u32 = 200;

/// Typed contract errors. Signalled via `panic_with_error!`, which produces a
/// deterministic contract error (not an unhandled host panic).
#[contracterror]
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    RecordNotFound = 3,
    InvalidExpiry = 4,
    Unauthorized = 5,
    InvalidJurisdiction = 6,
}

const DAY_IN_LEDGERS: u32 = 17_280; // ~5s ledgers
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Contract ABI/behavior version. Bump on any change to storage layout or
/// externally observable behavior so clients and the indexer can detect it.
pub const VERSION: u32 = 1;

#[contract]
pub struct ComplianceContract;

#[contractimpl]
impl ComplianceContract {
    /// Current contract version.
    pub fn version(_env: Env) -> u32 {
        VERSION
    }

    /// Initialize the contract with an admin. Callable exactly once.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error(&env, Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
        env.events().publish((symbol_short!("init"),), admin);
    }

    /// Add (or re-approve) an address on the KYC allowlist.
    pub fn add_to_allowlist(
        env: Env,
        admin: Address,
        address: Address,
        jurisdiction: String,
        expires_at: u32,
    ) {
        Self::require_admin(&env, &admin);
        let now = env.ledger().sequence();
        if expires_at != 0 && expires_at <= now {
            panic_with_error(&env, Error::InvalidExpiry);
        }
        // Normalize jurisdiction: uppercase and trim whitespace.
        let jurisdiction = normalize_jurisdiction(&env, &jurisdiction);

        // Capture previous state for audit trail (issue #20).
        let prev: Option<KycRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::Record(address.clone()));

        let record = KycRecord {
            address: address.clone(),
            status: ComplianceStatus::Approved,
            jurisdiction: jurisdiction.clone(),
            verified_at: now,
            expires_at,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Record(address.clone()), &record);

        // Only a genuinely new address needs to be appended to a page; a
        // re-approval or reinstatement already has a page slot.
        if prev.is_none() {
            Self::append_to_allowlist(&env, &address);
        }
        Self::bump_instance(&env);

        // Emit before/after state so off-chain systems can distinguish a fresh
        // approval from a re-classification or reinstatement (issue #20).
        let (prev_jurisdiction, prev_expires_at, was_suspended) = match prev {
            Some(ref r) => (
                r.jurisdiction.clone(),
                r.expires_at,
                r.status == ComplianceStatus::Suspended,
            ),
            None => (jurisdiction.clone(), 0u32, false),
        };
        env.events().publish(
            (symbol_short!("approved"), address),
            (
                jurisdiction,
                expires_at,
                prev_jurisdiction,
                prev_expires_at,
                was_suspended,
            ),
        );
    }

    /// Suspend an approved address. Its record is retained but `is_allowed`
    /// returns `false` until it is re-approved.
    pub fn suspend(env: Env, admin: Address, address: Address) {
        Self::require_admin(&env, &admin);
        let mut record = Self::load_record(&env, &address);
        record.status = ComplianceStatus::Suspended;
        env.storage()
            .persistent()
            .set(&DataKey::Record(address.clone()), &record);
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("suspend"), address), ());
    }

    /// Remove an address entirely from the allowlist.
    pub fn remove(env: Env, admin: Address, address: Address) {
        Self::require_admin(&env, &admin);
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Record(address.clone()))
        {
            panic_with_error(&env, Error::RecordNotFound);
        }
        env.storage()
            .persistent()
            .remove(&DataKey::Record(address.clone()));
        Self::remove_from_allowlist(&env, &address);
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("removed"), address), ());
    }

    /// Core compliance check used by the asset token on every transfer/mint.
    /// Returns `true` only if the address is Approved, not expired, and its
    /// jurisdiction is not blocked.
    pub fn is_allowed(env: Env, address: Address) -> bool {
        let record: Option<KycRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::Record(address.clone()));
        let record = match record {
            Some(r) => r,
            None => return false,
        };
        if record.status != ComplianceStatus::Approved {
            return false;
        }
        let now = env.ledger().sequence();
        if record.expires_at != 0 && now >= record.expires_at {
            // Emit an expiry event so indexers can track the transition (issue #21).
            env.events()
                .publish((symbol_short!("expired"), address), record.expires_at);
            return false;
        }
        if Self::is_jurisdiction_blocked(env.clone(), record.jurisdiction) {
            return false;
        }
        true
    }

    /// Fetch the raw KYC record for an address, if any.
    pub fn get_record(env: Env, address: Address) -> Option<KycRecord> {
        env.storage().persistent().get(&DataKey::Record(address))
    }

    /// Approval status of an address, distinguishing "never submitted for KYC"
    /// from every recorded state (issue #183).
    ///
    /// Mapping:
    /// - `None` — the address has no KYC record at all (never seen).
    /// - `Some(Approved)` — currently on the allowlist. Note this does not by
    ///   itself mean `is_allowed` returns `true`: `is_allowed` additionally
    ///   checks expiry and jurisdiction blocks, neither of which changes the
    ///   stored status.
    /// - `Some(Pending)` / `Some(Rejected)` — reserved for future workflows;
    ///   no current method sets these.
    /// - `Some(Suspended)` — was approved, then suspended via [`Self::suspend`].
    pub fn status_of(env: Env, address: Address) -> Option<ComplianceStatus> {
        Self::get_record(env, address).map(|r| r.status)
    }

    /// Return every address currently on the allowlist.
    pub fn get_allowlist(env: Env) -> Vec<Address> {
        let mut all = Vec::new(&env);
        let (current_page, _) = Self::allowlist_meta(&env);
        for page_idx in 0..=current_page {
            let page: Vec<Address> = env
                .storage()
                .persistent()
                .get(&DataKey::AllowlistPage(page_idx))
                .unwrap_or_else(|| Vec::new(&env));
            for a in page.iter() {
                all.push_back(a);
            }
        }
        all
    }

    /// Block an entire jurisdiction (country code). Approved addresses in a
    /// blocked jurisdiction fail `is_allowed`.
    pub fn block_jurisdiction(env: Env, admin: Address, jurisdiction: String) {
        Self::require_admin(&env, &admin);
        let jurisdiction = normalize_jurisdiction(&env, &jurisdiction);
        env.storage()
            .persistent()
            .set(&DataKey::Blocked(jurisdiction.clone()), &true);
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("blockjur"),), jurisdiction);
    }

    /// Un-block a previously blocked jurisdiction.
    pub fn unblock_jurisdiction(env: Env, admin: Address, jurisdiction: String) {
        Self::require_admin(&env, &admin);
        let jurisdiction = normalize_jurisdiction(&env, &jurisdiction);
        env.storage()
            .persistent()
            .remove(&DataKey::Blocked(jurisdiction.clone()));
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("unblkjur"),), jurisdiction);
    }

    /// Whether a jurisdiction is currently blocked.
    pub fn is_jurisdiction_blocked(env: Env, jurisdiction: String) -> bool {
        let jurisdiction = normalize_jurisdiction(&env, &jurisdiction);
        env.storage()
            .persistent()
            .get(&DataKey::Blocked(jurisdiction))
            .unwrap_or(false)
    }

    /// Prune all expired records from the allowlist. Admin only (issue #21).
    /// Removes expired entries from persistent storage and the allowlist vector
    /// so indexers and `get_allowlist` no longer count them as Approved.
    pub fn prune_expired(env: Env, admin: Address) {
        Self::require_admin(&env, &admin);
        let now = env.ledger().sequence();
        let (current_page, _) = Self::allowlist_meta(&env);
        for page_idx in 0..=current_page {
            let page: Vec<Address> = env
                .storage()
                .persistent()
                .get(&DataKey::AllowlistPage(page_idx))
                .unwrap_or_else(|| Vec::new(&env));
            let mut next = Vec::new(&env);
            let mut changed = false;
            for addr in page.iter() {
                let record: Option<KycRecord> = env
                    .storage()
                    .persistent()
                    .get(&DataKey::Record(addr.clone()));
                let keep = match record {
                    Some(ref r) => {
                        if r.expires_at != 0 && now >= r.expires_at {
                            env.storage()
                                .persistent()
                                .remove(&DataKey::Record(addr.clone()));
                            env.storage()
                                .persistent()
                                .remove(&DataKey::AllowlistPageOf(addr.clone()));
                            env.events()
                                .publish((symbol_short!("expired"), addr.clone()), r.expires_at);
                            false
                        } else {
                            true
                        }
                    }
                    None => false,
                };
                if keep {
                    next.push_back(addr);
                } else {
                    changed = true;
                }
            }
            if changed {
                env.storage()
                    .persistent()
                    .set(&DataKey::AllowlistPage(page_idx), &next);
            }
        }
        Self::bump_instance(&env);
    }

    /// Return the configured admin address.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error(&env, Error::NotInitialized))
    }

    // ---- internal helpers ----

    fn require_admin(env: &Env, admin: &Address) {
        let stored: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error(env, Error::NotInitialized));
        // The declared admin must both match storage and authorize the call.
        admin.require_auth();
        if stored != *admin {
            panic_with_error(env, Error::Unauthorized);
        }
    }

    fn load_record(env: &Env, address: &Address) -> KycRecord {
        env.storage()
            .persistent()
            .get(&DataKey::Record(address.clone()))
            .unwrap_or_else(|| panic_with_error(env, Error::RecordNotFound))
    }

    fn bump_instance(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Current (page index, length of that page) append cursor. Defaults to
    /// an empty page 0 when nothing has been added yet.
    fn allowlist_meta(env: &Env) -> (u32, u32) {
        env.storage()
            .instance()
            .get(&DataKey::AllowlistMeta)
            .unwrap_or((0u32, 0u32))
    }

    /// Append a new address to the current allowlist page, rolling over to a
    /// fresh page once the current one is full (issue #177).
    fn append_to_allowlist(env: &Env, address: &Address) {
        let (mut page_idx, mut page_len) = Self::allowlist_meta(env);
        if page_len >= ALLOWLIST_PAGE_SIZE {
            page_idx += 1;
            page_len = 0;
        }
        let mut page: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AllowlistPage(page_idx))
            .unwrap_or_else(|| Vec::new(env));
        page.push_back(address.clone());
        env.storage()
            .persistent()
            .set(&DataKey::AllowlistPage(page_idx), &page);
        env.storage()
            .persistent()
            .set(&DataKey::AllowlistPageOf(address.clone()), &page_idx);
        page_len += 1;
        env.storage()
            .instance()
            .set(&DataKey::AllowlistMeta, &(page_idx, page_len));
    }

    /// Remove an address from whichever page it lives on. Leaves the page
    /// under-full rather than repacking pages, which keeps removal O(page
    /// size) instead of O(list size) (issue #177).
    fn remove_from_allowlist(env: &Env, address: &Address) {
        let page_idx: Option<u32> = env
            .storage()
            .persistent()
            .get(&DataKey::AllowlistPageOf(address.clone()));
        let Some(page_idx) = page_idx else {
            return;
        };
        let page: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AllowlistPage(page_idx))
            .unwrap_or_else(|| Vec::new(env));
        let mut next = Vec::new(env);
        for a in page.iter() {
            if a != *address {
                next.push_back(a);
            }
        }
        env.storage()
            .persistent()
            .set(&DataKey::AllowlistPage(page_idx), &next);
        env.storage()
            .persistent()
            .remove(&DataKey::AllowlistPageOf(address.clone()));
    }
}

/// Small wrapper so call sites read cleanly and never use `unwrap`/`expect`.
fn panic_with_error(env: &Env, error: Error) -> ! {
    soroban_sdk::panic_with_error!(env, error)
}

/// Normalize and validate a jurisdiction code (issue #47).
/// Strips spaces, uppercases, then enforces exactly 2 ASCII alpha characters
/// so only real ISO-3166-1 alpha-2 codes (e.g. "US", "KE") are accepted.
fn normalize_jurisdiction(env: &Env, jurisdiction: &String) -> String {
    let raw = jurisdiction.to_bytes();
    let len = raw.len();
    let mut buf = [0u8; 2];
    let mut out_len: usize = 0;
    for i in 0..len {
        let b = raw.get(i).unwrap_or(0);
        if b == b' ' {
            continue;
        }
        if out_len >= 2 || !b.is_ascii_alphabetic() {
            panic_with_error(env, Error::InvalidJurisdiction);
        }
        buf[out_len] = b.to_ascii_uppercase();
        out_len += 1;
    }
    if out_len != 2 {
        panic_with_error(env, Error::InvalidJurisdiction);
    }
    String::from_bytes(env, &buf[..2])
}

#[cfg(test)]
mod test;
