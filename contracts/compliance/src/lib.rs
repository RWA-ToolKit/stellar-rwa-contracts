#![no_std]
//! # Compliance Contract
//!
//! Maintains the KYC allowlist and jurisdiction rules that gate who may hold or
//! transfer a tokenized real-world asset. The asset-token contract performs a
//! cross-contract call into [`ComplianceContract::is_allowed`] on every transfer
//! (for both sender and recipient) and on every mint (for the recipient).
//!
//! Time is expressed in ledger sequence numbers (`u32`), not wall-clock dates.
//! An `expires_at` of `0` means the KYC approval never expires.
//!
//! Fallible entrypoints signal errors via `panic_with_error!` (host trap)
//! rather than returning `Result<_, Error>` — see the [`Error`] enum below.
//! Note that `is_allowed`, called cross-contract from the asset token on
//! every transfer/mint, deliberately returns `bool` instead of panicking, so
//! that a non-compliant address fails the call cleanly without trapping the
//! caller.

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
    Allowlist,
    Record(Address),
    Blocked(String),
}

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
}

const DAY_IN_LEDGERS: u32 = 17_280; // ~5s ledgers
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

#[contract]
pub struct ComplianceContract;

#[contractimpl]
impl ComplianceContract {
    /// Initialize the contract with an admin. Callable exactly once.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error(&env, Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::Allowlist, &Vec::<Address>::new(&env));
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

        let mut list: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Allowlist)
            .unwrap_or_else(|| Vec::new(&env));
        if !list.contains(&address) {
            list.push_back(address.clone());
            env.storage().instance().set(&DataKey::Allowlist, &list);
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

        let list: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Allowlist)
            .unwrap_or_else(|| Vec::new(&env));
        let mut next = Vec::new(&env);
        for a in list.iter() {
            if a != address {
                next.push_back(a);
            }
        }
        env.storage().instance().set(&DataKey::Allowlist, &next);
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("removed"), address), ());
    }

    /// Core compliance check used by the asset token on every transfer/mint.
    /// Returns `true` only if the address is Approved, not expired, and its
    /// jurisdiction is not blocked.
    pub fn is_allowed(env: Env, address: Address) -> bool {
        let record: Option<KycRecord> = env.storage().persistent().get(&DataKey::Record(address.clone()));
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

    /// Return every address ever added to the allowlist, regardless of current
    /// status (includes suspended, expired, and blocked-jurisdiction entries).
    /// Use [`Self::get_allowed`] for addresses that currently pass `is_allowed`.
    pub fn get_allowlist(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::Allowlist)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Return only the addresses that currently pass `is_allowed` (Approved,
    /// not expired, and not in a blocked jurisdiction).
    pub fn get_allowed(env: Env) -> Vec<Address> {
        let list: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Allowlist)
            .unwrap_or_else(|| Vec::new(&env));
        let mut out = Vec::new(&env);
        for addr in list.iter() {
            if Self::is_allowed(env.clone(), addr.clone()) {
                out.push_back(addr);
            }
        }
        out
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
        let list: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Allowlist)
            .unwrap_or_else(|| Vec::new(&env));
        let mut next = Vec::new(&env);
        for addr in list.iter() {
            let record: Option<KycRecord> = env
                .storage()
                .persistent()
                .get(&DataKey::Record(addr.clone()));
            let keep = match record {
                Some(ref r) => {
                    if r.expires_at != 0 && now >= r.expires_at {
                        // Remove the expired persistent record.
                        env.storage()
                            .persistent()
                            .remove(&DataKey::Record(addr.clone()));
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
            }
        }
        env.storage().instance().set(&DataKey::Allowlist, &next);
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
}

/// Small wrapper so call sites read cleanly and never use `unwrap`/`expect`.
fn panic_with_error(env: &Env, error: Error) -> ! {
    soroban_sdk::panic_with_error!(env, error)
}

/// Normalize a jurisdiction code: uppercase and trim leading/trailing spaces
/// so that "us", "Us", " US " all map to the canonical "US" (issue #19).
fn normalize_jurisdiction(env: &Env, jurisdiction: &String) -> String {
    let raw = jurisdiction.to_bytes();
    let len = raw.len();
    // Fixed-size stack buffer; jurisdiction codes are short (≤ 16 bytes).
    let mut buf = [0u8; 16];
    let mut out_len: usize = 0;
    for i in 0..len {
        let b = raw.get(i).unwrap_or(0);
        if b != b' ' && out_len < 16 {
            buf[out_len] = b.to_ascii_uppercase();
            out_len += 1;
        }
    }
    String::from_bytes(env, &buf[..out_len])
}

#[cfg(test)]
mod test;
