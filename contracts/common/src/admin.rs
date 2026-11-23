use soroban_sdk::{Address, Env, IntoVal, TryFromVal, Val};

/// Number of ledgers a pending admin proposal stays acceptable (issue #671).
///
/// After `propose` records the current ledger sequence, `accept` rejects the
/// proposal once `env.ledger().sequence() > proposal_ledger + ADMIN_PROPOSAL_TTL_LEDGERS`.
/// This bounds the window in which a compromised successor key could be used
/// to complete a stale handover.
pub const ADMIN_PROPOSAL_TTL_LEDGERS: u32 = 17_280; // ~1 day at 5s/ledger

/// Outcome of [`accept`].
///
/// Callers map each variant onto their own contract error so that an expired
/// proposal is distinguishable from a mismatched/unauthorized one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcceptOutcome {
    /// Handover completed; carries `(old_admin, new_admin)`.
    Accepted(Address, Address),
    /// There is no pending proposal.
    NoPending,
    /// `new_admin` does not match the pending proposal.
    Mismatch,
    /// The pending proposal has expired (issue #671).
    Expired,
}

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
/// Write `new_admin` as the pending successor into instance storage, along
/// with the ledger sequence at which the proposal was made.
pub fn propose<K>(env: &Env, pending_key: &K, new_admin: &Address)
where
    K: IntoVal<Env, Val>,
{
    env.storage().instance().set(pending_key, new_admin);
    env.storage()
        .instance()
        .set(&proposal_ledger_key(pending_key), &env.ledger().sequence());
}

/// Storage key under which the proposal ledger sequence is recorded.
///
/// Derived from the pending-admin key so the helpers stay
/// storage-key-parameterised and each contract keeps its own `DataKey` enum.
fn proposal_ledger_key<K>(pending_key: &K) -> (K, u32)
where
    K: IntoVal<Env, Val> + Clone,
{
    (pending_key.clone(), 0)
}

/// Remove the pending proposal from instance storage.
///
/// Returns `false` when there was no proposal; callers should panic with
/// their own `NoPendingAdmin` error in that case.
pub fn cancel<K>(env: &Env, pending_key: &K) -> bool
where
    K: IntoVal<Env, Val> + Clone,
{
    if !env.storage().instance().has(pending_key) {
        return false;
    }
    env.storage().instance().remove(pending_key);
    env.storage()
        .instance()
        .remove(&proposal_ledger_key(pending_key));
    true
}

/// Complete the handover.
///
/// Verifies `new_admin.require_auth()`, that there is a pending proposal,
/// that it has not expired (issue #671), and that `new_admin` matches it.
/// On success updates the admin key and clears the pending key, returning
/// `(old_admin, new_admin)`.
///
/// Returns [`AcceptOutcome::NoPending`] when the pending admin storage slot is
/// empty, [`AcceptOutcome::Expired`] when the proposal is older than
/// [`ADMIN_PROPOSAL_TTL_LEDGERS`], and [`AcceptOutcome::Mismatch`] when
/// `new_admin` does not match the pending proposal.
pub fn accept<K>(
    env: &Env,
    admin_key: &K,
    pending_key: &K,
    new_admin: &Address,
) -> AcceptOutcome
where
    K: IntoVal<Env, Val> + Clone,
    Address: TryFromVal<Env, Val>,
{
    new_admin.require_auth();
    let pending: Address = match env.storage().instance().get(pending_key) {
        Some(p) => p,
        None => return AcceptOutcome::NoPending,
    };
    if &pending != new_admin {
        return AcceptOutcome::Mismatch;
    }
    let proposal_ledger: u32 = env
        .storage()
        .instance()
        .get(&proposal_ledger_key(pending_key))
        .unwrap_or(0);
    if env.ledger().sequence() > proposal_ledger.saturating_add(ADMIN_PROPOSAL_TTL_LEDGERS) {
        return AcceptOutcome::Expired;
    }
    let old_admin: Address = match env.storage().instance().get(admin_key) {
        Some(a) => a,
        None => return AcceptOutcome::NoPending,
    };
    env.storage().instance().set(admin_key, new_admin);
    env.storage().instance().remove(pending_key);
    env.storage()
        .instance()
        .remove(&proposal_ledger_key(pending_key));
    AcceptOutcome::Accepted(old_admin, new_admin.clone())
}

/// Return the currently pending admin, or `None`.
pub fn get_pending<K>(env: &Env, pending_key: &K) -> Option<Address>
where
    K: IntoVal<Env, Val>,
    Address: TryFromVal<Env, Val>,
{
    env.storage().instance().get(pending_key)
}

/// Return the ledger sequence at which the pending proposal was made, or `None`.
pub fn get_pending_ledger<K>(env: &Env, pending_key: &K) -> Option<u32>
where
    K: IntoVal<Env, Val> + Clone,
{
    env.storage()
        .instance()
        .get(&proposal_ledger_key(pending_key))
}
