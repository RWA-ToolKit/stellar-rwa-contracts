//! Internal helpers — not part of the public contract interface.

use soroban_sdk::{symbol_short, Env, IntoVal, String, Val};

use crate::Error;

/// Debug-only diagnostic event (issue #740). `release-with-logs` sets
/// `debug-assertions = true`; the standard release profile does not, so this
/// code is compiled out of production wasm entirely.
#[cfg(debug_assertions)]
fn diagnostic(env: &Env, error: Error, value: Option<Val>) {
    match value {
        Some(value) => env
            .events()
            .publish((symbol_short!("panic_dbg"), value), error),
        None => env.events().publish((symbol_short!("panic_dbg"),), error),
    }
}

/// Abort the current invocation with the given contract error.
pub(crate) fn panic_err(env: &Env, error: Error) -> ! {
    #[cfg(debug_assertions)]
    diagnostic(env, error, None);
    soroban_sdk::panic_with_error!(env, error)
}

/// Like [`panic_err`], but publishes the offending value as a debug-only
/// diagnostic event topic before panicking.
pub(crate) fn panic_err_value<T>(env: &Env, error: Error, value: T) -> !
where
    T: IntoVal<Env, Val>,
{
    #[cfg(debug_assertions)]
    diagnostic(env, error, Some(value.into_val(env)));
    soroban_sdk::panic_with_error!(env, error)
}

/// Reject strings that are empty or exceed `max_len` bytes (issue #46).
pub(crate) fn check_str(env: &Env, s: &String, min_len: u32, max_len: u32) {
    let len = s.len();
    if len < min_len || len > max_len {
        panic_err_value(env, Error::InvalidInput, (min_len, max_len, len));
    }
}
