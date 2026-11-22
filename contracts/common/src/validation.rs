use soroban_sdk::{Env, String};

/// Panic if `s` has fewer than `min_len` or more than `max_len` bytes.
///
/// Extracted from `asset-token` (issue #46 / #765) so every contract with
/// string metadata fields uses the identical guard.
///
/// # Panics
/// Calls `env.panic_with_error` with the caller-supplied `error_code` when
/// the length check fails, so the calling contract controls the error variant.
pub fn check_str<E: Into<soroban_sdk::Error>>(
    env: &Env,
    s: &String,
    min_len: u32,
    max_len: u32,
    error_code: E,
) {
    let len = s.len();
    if len < min_len || len > max_len {
        env.panic_with_error(error_code);
    }
}
