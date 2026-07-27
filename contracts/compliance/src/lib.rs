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
    PendingAdmin,
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
    NoPendingAdmin = 6,
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
        env.events().publish(
            (symbol_short!("approved"), address),
            (jurisdiction, expires_at),
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
        let record: Option<KycRecord> = env.storage().persistent().get(&DataKey::Record(address));
        let record = match record {
            Some(r) => r,
            None => return false,
        };
        if record.status != ComplianceStatus::Approved {
            return false;
        }
        let now = env.ledger().sequence();
        if record.expires_at != 0 && now >= record.expires_at {
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

    /// Return every address currently on the allowlist.
    pub fn get_allowlist(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::Allowlist)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Block an entire jurisdiction (country code). Approved addresses in a
    /// blocked jurisdiction fail `is_allowed`.
    pub fn block_jurisdiction(env: Env, admin: Address, jurisdiction: String) {
        Self::require_admin(&env, &admin);
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
        env.storage()
            .persistent()
            .remove(&DataKey::Blocked(jurisdiction.clone()));
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("unblkjur"),), jurisdiction);
    }

    /// Whether a jurisdiction is currently blocked.
    pub fn is_jurisdiction_blocked(env: Env, jurisdiction: String) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Blocked(jurisdiction))
            .unwrap_or(false)
    }

    /// Return the configured admin address.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error(&env, Error::NotInitialized))
    }

    /// Return the pending admin address, if set.
    pub fn get_pending_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::PendingAdmin)
    }

    /// Propose a new admin address. Current admin only.
    pub fn propose_admin(env: Env, admin: Address, new_admin: Address) {
        Self::require_admin(&env, &admin);
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("prop_adm"), admin), new_admin);
    }

    /// Accept admin handover. Must be called by the proposed pending admin.
    pub fn accept_admin(env: Env, new_admin: Address) {
        new_admin.require_auth();
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .unwrap_or_else(|| panic_with_error(&env, Error::NoPendingAdmin));
        if pending != new_admin {
            panic_with_error(&env, Error::Unauthorized);
        }
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("acc_adm"),), new_admin);
    }

    /// Upgrade contract WASM code. Admin only.
    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: soroban_sdk::BytesN<32>) {
        Self::require_admin(&env, &admin);
        env.deployer().update_current_contract_wasm(new_wasm_hash.clone());
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("upgrade"), admin), new_wasm_hash);
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

#[cfg(test)]
mod test;
