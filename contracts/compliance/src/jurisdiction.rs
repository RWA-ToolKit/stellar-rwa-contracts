//! Jurisdiction-blocking helpers for the compliance contract.

use soroban_sdk::{Env, String, Vec};

use crate::DataKey;

/// Return the current list of blocked jurisdiction codes, or an empty vec.
pub(crate) fn blocked_list(env: &Env) -> Vec<String> {
    env.storage()
        .instance()
        .get(&DataKey::BlockedList)
        .unwrap_or_else(|| Vec::new(env))
}
