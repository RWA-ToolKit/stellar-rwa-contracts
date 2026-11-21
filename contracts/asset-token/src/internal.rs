//! Internal helpers — not part of the public contract interface.

use soroban_sdk::{Env, String};

use crate::Error;

/// Abort the current invocation with the given contract error.
pub(crate) fn panic_err(env: &Env, error: Error) -> ! {
    soroban_sdk::panic_with_error!(env, error)
}

/// Reject strings that are empty or exceed `max_len` bytes (issue #46).
pub(crate) fn check_str(env: &Env, s: &String, min_len: u32, max_len: u32) {
    let len = s.len();
    if len < min_len || len > max_len {
        panic_err(env, Error::InvalidInput);
    }
}
