//! Jurisdiction-blocking helpers for the compliance contract.

use soroban_sdk::{Env, String, Vec};

use crate::DataKey;

/// Normalize a jurisdiction code to a canonical (uppercase) form.
///
/// Jurisdiction codes are compared case-insensitively by normalizing both the
/// write path (`block_jurisdiction`) and the read path
/// (`is_jurisdiction_blocked`) to uppercase. This ensures that a jurisdiction
/// blocked as `"US"` is also matched when a holder's record stores `"us"`.
pub(crate) fn normalize_jurisdiction(env: &Env, code: &String) -> String {
    let mut out = String::new(env);
    let len = code.len();
    let mut i: u32 = 0;
    while i < len {
        let b = code.get(i).unwrap_or(b' ');
        let upper = if b >= b'a' && b <= b'z' {
            b - (b'a' - b'A')
        } else {
            b
        };
        out.push_back(upper as char);
        i += 1;
    }
    out
}

/// Return the current list of blocked jurisdiction codes, or an empty vec.
pub(crate) fn blocked_list(env: &Env) -> Vec<String> {
    env.storage()
        .instance()
        .get(&DataKey::BlockedList)
        .unwrap_or_else(|| Vec::new(env))
}