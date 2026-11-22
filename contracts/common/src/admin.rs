use soroban_sdk::{Address, Env, IntoVal, TryFromVal, Val};

/// Two-step admin-handover helpers (issue #764).
///
/// Each of the four RWA contracts implements the identical
/// propose → cancel/accept state machine.  These helpers are
/// storage-key-parameterised so every contract can share the logic
/// while keeping its own `DataKey` enum.
///
/// # Example (inside a contract impl)
/// ```ignore
/// pub fn propose_admin(env: Env, admin: Address, new_admin: Address) {
///     Self::require_admin(&env, &admin);
///     rwa_common::admin::propose(&env, &DataKey::PendingAdmin, &new_admin);
///     env.events().publish(("proposed", admin), new_admin);
/// }
/// ```
/// Write `new_admin` as the pending successor into instance storage.
pub fn propose<K>(env: &Env, pending_key: &K, new_admin: &Address)
where
    K: IntoVal<Env, Val>,
{
    env.storage().instance().set(pending_key, new_admin);
}

/// Remove the pending proposal from instance storage.
///
/// Returns `false` when there was no proposal; callers should panic with
/// their own `NoPendingAdmin` error in that case.
pub fn cancel<K>(env: &Env, pending_key: &K) -> bool
where
    K: IntoVal<Env, Val>,
{
    if !env.storage().instance().has(pending_key) {
        return false;
    }
    env.storage().instance().remove(pending_key);
    true
}

/// Complete the handover.
///
/// Verifies `new_admin.require_auth()`, that there is a pending proposal,
/// and that `new_admin` matches it.  On success updates the admin key and
/// clears the pending key, returning `(old_admin, new_admin)`.
///
/// Returns `None` when the pending admin storage slot is empty; `Some(Err(()))`
/// when `new_admin` does not match the pending proposal.
pub fn accept<K>(
    env: &Env,
    admin_key: &K,
    pending_key: &K,
    new_admin: &Address,
) -> Option<Result<(Address, Address), ()>>
where
    K: IntoVal<Env, Val> + Clone,
    Address: TryFromVal<Env, Val>,
{
    new_admin.require_auth();
    let pending: Address = env.storage().instance().get(pending_key)?;
    if &pending != new_admin {
        return Some(Err(()));
    }
    let old_admin: Address = env.storage().instance().get(admin_key)?;
    env.storage().instance().set(admin_key, new_admin);
    env.storage().instance().remove(pending_key);
    Some(Ok((old_admin, new_admin.clone())))
}

/// Return the currently pending admin, or `None`.
pub fn get_pending<K>(env: &Env, pending_key: &K) -> Option<Address>
where
    K: IntoVal<Env, Val>,
    Address: TryFromVal<Env, Val>,
{
    env.storage().instance().get(pending_key)
}
