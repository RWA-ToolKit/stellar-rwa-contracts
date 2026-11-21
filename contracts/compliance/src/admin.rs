//! Admin-related storage helpers for the compliance contract.

use soroban_sdk::{Address, Env};

use crate::{internal::panic_with_error, DataKey, Error, KycRecord};

/// Verify that `admin` matches the stored admin and has authorized the call.
pub(crate) fn require_admin(env: &Env, admin: &Address) {
    let stored: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .unwrap_or_else(|| panic_with_error(env, Error::NotInitialized));
    admin.require_auth();
    if stored != *admin {
        panic_with_error(env, Error::Unauthorized);
    }
}

/// Load the KYC record for `address`, panicking with `RecordNotFound` if absent.
pub(crate) fn load_record(env: &Env, address: &Address) -> KycRecord {
    env.storage()
        .persistent()
        .get(&DataKey::Record(address.clone()))
        .unwrap_or_else(|| panic_with_error(env, Error::RecordNotFound))
}

/// Extend instance TTL so state remains live after any state-writing call.
pub(crate) fn bump_instance(env: &Env) {
    use crate::{INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD};
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}
