//! Low-level helpers shared across compliance submodules.

use soroban_sdk::Env;

use crate::Error;

/// Abort the current invocation with the given contract error.
pub(crate) fn panic_with_error(env: &Env, error: Error) -> ! {
    soroban_sdk::panic_with_error!(env, error)
}

/// Normalize and validate a jurisdiction code (issue #47).
/// Strips spaces, uppercases, then enforces exactly 2 ASCII alpha characters
/// so only real ISO-3166-1 alpha-2 codes (e.g. "US", "KE") are accepted.
pub(crate) fn normalize_jurisdiction(
    env: &Env,
    jurisdiction: &soroban_sdk::String,
) -> soroban_sdk::String {
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
    soroban_sdk::String::from_bytes(env, &buf[..2])
}
