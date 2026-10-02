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
//!
//! ## Admin is independent of the asset-token admin (issue #3)
//!
//! This contract's admin (set via [`ComplianceContract::initialize`]) is its
//! own, self-contained piece of state — nothing here reads or depends on the
//! `admin` stored by any asset-token contract that points at it. An issuer is
//! free to run compliance under a dedicated compliance officer's address
//! while a different address administers the asset token; `scripts/deploy.sh`
//! passes the same address for both purely as a convenience default for a
//! single-operator demo deployment, not because the contracts require it.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, String, Vec,
};

/// Approval state of an address.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComplianceStatus {
    Approved,
    /// Deprecated legacy ABI value. Kept only for backwards compatibility;
    /// the contract never writes this status and no public workflow produces it.
    #[deprecated(note = "Pending is retained for ABI compatibility only; current workflows never set it.")]
    Pending,
    /// Deprecated legacy ABI value. Kept only for backwards compatibility;
    /// the contract never writes this status and no public workflow produces it.
    #[deprecated(note = "Rejected is retained for ABI compatibility only; current workflows never set it.")]
    Rejected,
    Suspended,
}

/// One entry of an [`ComplianceContract::add_to_allowlist_batch`] call. Mirrors
/// the parameters of [`ComplianceContract::add_to_allowlist`] exactly, so each
/// entry is validated the same way it would be if submitted individually.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllowlistEntry {
    pub address: Address,
    pub jurisdiction: String,
    pub expires_at: u32,
}

/// A single KYC record for an address.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KycRecord {
    pub address: Address,
    pub status: ComplianceStatus,
    /// Canonical ISO-3166-1 alpha-2 country code, e.g. "US", "KE", "DE".
    /// Always exactly 2 uppercase ASCII letters; see [`normalize_jurisdiction`]
    /// for the enforced canonical form.
    pub jurisdiction: String,
    /// Ledger sequence at which the record was verified.
    pub verified_at: u32,
    /// Ledger sequence at which approval expires.
    ///
    /// Sentinel: `0` means the approval **never expires**. See the
    /// module-level "The `expires_at = 0` sentinel" section for why this is
    /// safe and how [`ComplianceContract::is_allowed`] treats it.
    pub expires_at: u32,
}

/// Storage keys.
#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    /// Address nominated by the current admin via `propose_admin`, pending
    /// acceptance via `accept_admin` (issue #4). Absent when there is no
    /// proposal in flight.
    PendingAdmin,
    /// Small fixed-size (current_page, current_page_len) cursor for appends.
    AllowlistMeta,
    /// One page of up to `ALLOWLIST_PAGE_SIZE` addresses, in persistent storage
    /// (issue #177): no single entry grows without bound or shares the
    /// instance ledger entry with the rest of the contract's state.
    AllowlistPage(u32),
    /// Which page an address currently lives on, for O(1) removal.
    AllowlistPageOf(Address),
    /// Maintained counter of addresses currently on the allowlist, kept in
    /// sync by `append_to_allowlist` / `remove_from_allowlist` so
    /// `get_allowlist_count` never has to walk any pages.
    AllowlistCount,
    Record(Address),
    Blocked(String),
    /// Ordered list of every jurisdiction currently blocked, kept in sync
    /// with the individual `Blocked(String)` flags so the full blocked set
    /// can be read directly instead of being inferred off-chain from the
    /// absence of approved addresses in a jurisdiction.
    BlockedList,
    /// Minimum number of ledgers that must elapse after a holder first
    /// acquires tokens before they are permitted to transfer them (issue
    /// #454). Absent means no holding-period requirement for that address.
    MinHoldingPeriod(Address),
    /// The ledger sequence at which `address` first acquired tokens (issue
    /// #454). Set by `record_acquisition` when no prior acquisition exists.
    /// Never updated once set so that the clock always starts from the
    /// earliest acquisition, not from a later top-up.
    FirstAcquiredLedger(Address),
}

/// Max addresses per allowlist page (issue #177). Bounds the size of any single
/// storage entry regardless of how large the KYC list grows.
const ALLOWLIST_PAGE_SIZE: u32 = 200;

/// Maximum number of addresses `get_allowlist_page` will return in a single
/// call, regardless of the requested `limit`. Callers that pass `0` or a
/// value greater than this get back exactly this many entries (or fewer, on
/// the final partial page).
pub const MAX_ALLOWLIST_PAGE_SIZE: u32 = 200;

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
    InvalidJurisdiction = 6,
    /// `accept_admin` or `cancel_admin_proposal` called with no pending
    /// admin proposal on file (issue #4).
    NoPendingAdmin = 7,
    /// `reinstate` was called on an address that is not currently `Suspended`.
    NotSuspended = 8,
    /// `set_min_holding_period` was called with `min_ledgers` overflowing when
    /// added to a realistic ledger sequence; treated as an invalid input.
    InvalidHoldingPeriod = 9,
}

const DAY_IN_LEDGERS: u32 = 17_280; // ~5s ledgers
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Contract ABI/behavior version. Bump on any change to storage layout or
/// externally observable behavior so clients and the indexer can detect it.
pub const VERSION: u32 = 3;

#[contract]
pub struct ComplianceContract;

#[contractimpl]
impl ComplianceContract {
    /// Returns the contract's ABI/behavior version number.
    ///
    /// Callers and indexers can use this to detect schema changes without
    /// having to probe individual storage entries.
    ///
    /// # Parameters
    /// - `_env`: Soroban environment (unused).
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// None.
    ///
    /// # Events
    /// None.
    pub fn version(_env: Env) -> u32 {
        VERSION
    }

    /// Initialize the contract and set the first admin. Callable exactly once.
    ///
    /// # Parameters
    /// - `admin`: Address that will administer KYC records and jurisdiction
    ///   blocks. Must authorize the call.
    ///
    /// # Authority
    /// `admin` must sign the transaction (`admin.require_auth()`).
    ///
    /// # Errors
    /// - [`Error::AlreadyInitialized`] — contract was already initialized.
    ///
    /// # Events
    /// Emits topic `("init",)` with data `admin`.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error(&env, Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
        env.events().publish((symbol_short!("init"),), admin);
    }

    /// Add (or re-approve) a single address on the KYC allowlist.
    ///
    /// Creates a [`KycRecord`] with status `Approved` for `address`, or
    /// overwrites an existing record (re-approval / reinstatement via
    /// replacement). The `jurisdiction` is normalized to an uppercase
    /// ISO-3166-1 alpha-2 code. Previous record state is captured in the
    /// emitted event for off-chain audit (issue #20).
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `address`: Address to approve.
    /// - `jurisdiction`: ISO-3166-1 alpha-2 country code (e.g. `"US"`,
    ///   `"KE"`). Whitespace is stripped; letters are uppercased. Exactly two
    ///   ASCII alpha characters required.
    /// - `expires_at`: Ledger sequence at which approval expires
    ///   (`expires_at` itself is already expired — see boundary semantics in
    ///   [`Self::is_allowed`]).  Pass `0` for a non-expiring approval.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    /// - [`Error::InvalidExpiry`] — `expires_at` is non-zero and already
    ///   in the past (`expires_at <= current_ledger`).
    /// - [`Error::InvalidJurisdiction`] — `jurisdiction` is not a valid
    ///   two-letter ASCII alpha code.
    ///
    /// # Events
    /// Emits topic `("approved", address)` with data
    /// `(jurisdiction, expires_at, prev_jurisdiction, prev_expires_at, was_suspended)`.
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
        // Normalize jurisdiction: uppercase and trim whitespace.
        let jurisdiction = normalize_jurisdiction(&env, &jurisdiction);

        // Capture previous state for audit trail (issue #20).
        let prev: Option<KycRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::Record(address.clone()));

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

        // Only a genuinely new address needs to be appended to a page; a
        // re-approval or reinstatement already has a page slot.
        if prev.is_none() {
            Self::append_to_allowlist(&env, &address);
        }
        Self::bump_instance(&env);

        // Emit before/after state so off-chain systems can distinguish a fresh
        // approval from a re-classification or reinstatement (issue #20).
        let (prev_jurisdiction, prev_expires_at, was_suspended) = match prev {
            Some(ref r) => (
                r.jurisdiction.clone(),
                r.expires_at,
                r.status == ComplianceStatus::Suspended,
            ),
            None => (jurisdiction.clone(), 0u32, false),
        };
        env.events().publish(
            (symbol_short!("approved"), address),
            (
                jurisdiction,
                expires_at,
                prev_jurisdiction,
                prev_expires_at,
                was_suspended,
            ),
        );
    }

    /// Add (or re-approve) several addresses on the KYC allowlist in one
    /// transaction (issue #338).
    ///
    /// Each [`AllowlistEntry`] carries its own `jurisdiction` and
    /// `expires_at`, so every entry is validated and normalized exactly as
    /// [`Self::add_to_allowlist`] validates a single address: same expiry
    /// check, same jurisdiction normalization, same audit-trail capture,
    /// same per-address `approved` event.
    ///
    /// If any entry fails validation (e.g. its `expires_at` is in the past,
    /// or its jurisdiction is malformed), the call panics immediately with
    /// the same error the single-address path would raise for that entry.
    /// Soroban rolls back all state changes made earlier in the same
    /// invocation when it panics, so a failing entry never silently skips
    /// itself while leaving earlier entries in the batch committed — the
    /// whole batch either fully applies or fully reverts.
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `entries`: Vec of [`AllowlistEntry`] values, each specifying an
    ///   `address`, `jurisdiction`, and `expires_at`.  Validated identically
    ///   to the single-address path.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    /// - [`Error::InvalidExpiry`] — any entry's `expires_at` is already past.
    /// - [`Error::InvalidJurisdiction`] — any entry's jurisdiction is invalid.
    ///
    /// # Events
    /// Emits one `("approved", address)` event per entry in the batch (same
    /// payload as [`Self::add_to_allowlist`]).
    pub fn add_to_allowlist_batch(env: Env, admin: Address, entries: Vec<AllowlistEntry>) {
        Self::require_admin(&env, &admin);
        let now = env.ledger().sequence();

        for entry in entries.iter() {
            let address = entry.address;
            let expires_at = entry.expires_at;
            if expires_at != 0 && expires_at <= now {
                panic_with_error(&env, Error::InvalidExpiry);
            }
            let jurisdiction = normalize_jurisdiction(&env, &entry.jurisdiction);

            let prev: Option<KycRecord> = env
                .storage()
                .persistent()
                .get(&DataKey::Record(address.clone()));

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

            if prev.is_none() {
                Self::append_to_allowlist(&env, &address);
            }

            let (prev_jurisdiction, prev_expires_at, was_suspended) = match prev {
                Some(ref r) => (
                    r.jurisdiction.clone(),
                    r.expires_at,
                    r.status == ComplianceStatus::Suspended,
                ),
                None => (jurisdiction.clone(), 0u32, false),
            };
            env.events().publish(
                (symbol_short!("approved"), address),
                (
                    jurisdiction,
                    expires_at,
                    prev_jurisdiction,
                    prev_expires_at,
                    was_suspended,
                ),
            );
        }
        Self::bump_instance(&env);
    }

    /// Suspend an approved address.
    ///
    /// The record is retained but [`Self::is_allowed`] returns `false` until
    /// the address is re-approved (via [`Self::add_to_allowlist`]) or
    /// reinstated (via [`Self::reinstate`]).
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `address`: Address to suspend.  Must already have a KYC record.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    /// - [`Error::RecordNotFound`] — `address` has no KYC record.
    ///
    /// # Events
    /// Emits topic `("suspend", address)` with data `()`.
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

    /// Reinstate a `Suspended` address without discarding its original KYC
    /// metadata.
    ///
    /// Unlike calling [`Self::add_to_allowlist`] again (which requires the
    /// caller to resupply `jurisdiction`/`expires_at` and overwrites
    /// `verified_at`), `reinstate` flips the status back to `Approved` and
    /// leaves `jurisdiction`, `verified_at` and `expires_at` untouched.
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `address`: Address to reinstate.  Must exist and be `Suspended`.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    /// - [`Error::RecordNotFound`] — `address` has no KYC record.
    /// - [`Error::NotSuspended`] — `address` exists but is not `Suspended`.
    ///
    /// # Events
    /// Emits topic `("reinstat", address)` with data `()`.
    pub fn reinstate(env: Env, admin: Address, address: Address) {
        Self::require_admin(&env, &admin);
        let mut record = Self::load_record(&env, &address);
        if record.status != ComplianceStatus::Suspended {
            panic_with_error(&env, Error::NotSuspended);
        }
        record.status = ComplianceStatus::Approved;
        env.storage()
            .persistent()
            .set(&DataKey::Record(address.clone()), &record);
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("reinstat"), address), ());
    }

    /// Remove an address entirely from the allowlist.
    ///
    /// Deletes both the [`KycRecord`] and the address's allowlist page slot.
    /// Use [`Self::suspend`] instead to keep the record for audit while
    /// blocking transfers.
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `address`: Address to remove.  Must have an existing KYC record.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    /// - [`Error::RecordNotFound`] — `address` has no KYC record.
    ///
    /// # Events
    /// Emits topic `("removed", address)` with data `()`.
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
        Self::remove_from_allowlist(&env, &address);
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("removed"), address), ());
    }

    /// Core compliance check called by the asset-token contract on every
    /// transfer and mint.
    ///
    /// Returns `true` only when all three conditions hold:
    /// 1. `address` has a [`KycRecord`] with status `Approved`.
    /// 2. The record has not expired (`expires_at == 0`, or
    ///    `current_ledger < expires_at`).
    /// 3. The record's jurisdiction is not currently blocked.
    ///
    /// Expiry boundary semantics (issue #341): `expires_at` is **exclusive**.
    /// A record is valid up to ledger `expires_at - 1`; it lapses starting at
    /// ledger `expires_at` (i.e. `now >= expires_at` means expired).
    ///
    /// # Parameters
    /// - `address`: Address to check.
    ///
    /// # Authority
    /// None — anyone may call (typically the asset-token contract).
    ///
    /// # Errors
    /// None — returns `false` rather than panicking for any non-compliant case.
    ///
    /// # Events
    /// Emits topic `("expired", address)` with data `expires_at` when a
    /// record is found to have lapsed at check time, so indexers can track
    /// the expiry transition without polling.
    pub fn is_allowed(env: Env, address: Address) -> bool {
        let record: Option<KycRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::Record(address.clone()));
        let record = match record {
            Some(r) => r,
            None => return false,
        };
        if record.status != ComplianceStatus::Approved {
            return false;
        }
        let now = env.ledger().sequence();
        // Boundary semantics (issue #341): `expires_at` is exclusive. A record
        // is still valid at `expires_at - 1`, and lapses starting exactly at
        // ledger `expires_at` (i.e. `now >= expires_at` is expired, not
        // `now > expires_at`). This matches `add_to_allowlist`, which already
        // rejects `expires_at <= now` as already-expired at creation time.
        if record.expires_at != 0 && now >= record.expires_at {
            // Emit an expiry event so indexers can track the transition (issue #21).
            env.events()
                .publish((symbol_short!("expired"), address), record.expires_at);
            return false;
        }
        if Self::is_jurisdiction_blocked(env.clone(), record.jurisdiction) {
            return false;
        }
        // Minimum holding period check (issue #454).
        // If a holding period has been set for this address, verify that
        // enough ledgers have elapsed since the first recorded acquisition.
        // Burning tokens (address == zero / the contract itself) should always
        // be permitted; we cannot detect that case from inside the compliance
        // contract, so we rely on the caller not registering a holding period
        // for the burn address. If no `FirstAcquiredLedger` entry exists we
        // fail-open (treat the period as satisfied) so existing holders whose
        // acquisition was never recorded are not inadvertently locked out.
        if let Some(min_ledgers) = env
            .storage()
            .persistent()
            .get::<DataKey, u64>(&DataKey::MinHoldingPeriod(address.clone()))
        {
            if min_ledgers > 0 {
                if let Some(first_acquired) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, u64>(&DataKey::FirstAcquiredLedger(address.clone()))
                {
                    let unlock_at = first_acquired.saturating_add(min_ledgers);
                    if (now as u64) < unlock_at {
                        env.events().publish(
                            (symbol_short!("hldfail"), address),
                            (first_acquired, min_ledgers, now as u64),
                        );
                        return false;
                    }
                }
                // No acquisition record → fail-open (see comment above).
            }
        }
        true
    }

    /// Fetch the raw KYC record for an address, if any.
    ///
    /// Returns `None` when the address has never been submitted for KYC.
    /// Use [`Self::status_of`] for a lighter-weight status-only query.
    ///
    /// # Parameters
    /// - `address`: Address to look up.
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// None.
    ///
    /// # Events
    /// None.
    pub fn get_record(env: Env, address: Address) -> Option<KycRecord> {
        env.storage().persistent().get(&DataKey::Record(address))
    }

    /// Approval status of an address, distinguishing "never submitted for KYC"
    /// from every recorded state (issue #183).
    ///
    /// Mapping:
    /// - `None` — the address has no KYC record at all (never seen).
    /// - `Some(Approved)` — currently on the allowlist. Note this does not by
    ///   itself mean `is_allowed` returns `true`: `is_allowed` additionally
    ///   checks expiry and jurisdiction blocks, neither of which changes the
    ///   stored status.
    /// - `Some(Pending)` / `Some(Rejected)` — deprecated legacy variants kept
    ///   only for ABI stability. The current contract never writes them, and
    ///   no public method can create them. Callers should treat them as
    ///   unreachable / unsupported values.
    /// - `Some(Suspended)` — was approved, then suspended via [`Self::suspend`].
    ///
    /// # Parameters
    /// - `address`: Address to query.
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// None.
    ///
    /// # Events
    /// None.
    pub fn status_of(env: Env, address: Address) -> Option<ComplianceStatus> {
        Self::get_record(env, address).map(|r| r.status)
    }

    /// Return every address currently on the allowlist.
    ///
    /// Iterates all allocated pages, skipping absent or empty pages so that
    /// pages emptied by heavy `remove` churn are not charged to callers
    /// (issue #306). Includes suspended addresses (their page slot is
    /// preserved by [`Self::suspend`]).
    ///
    /// For large lists prefer [`Self::get_allowlist_page`] to bound per-call
    /// resource usage; this function transfers the entire list in one call.
    ///
    /// # Parameters
    /// None.
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// None.
    ///
    /// # Events
    /// None.
    pub fn get_allowlist(env: Env) -> Vec<Address> {
        let mut all = Vec::new(&env);
        let (current_page, _) = Self::allowlist_meta(&env);
        for page_idx in 0..=current_page {
            let page: Option<Vec<Address>> = env
                .storage()
                .persistent()
                .get(&DataKey::AllowlistPage(page_idx));
            let page = match page {
                Some(p) if !p.is_empty() => p,
                _ => continue,
            };
            for a in page.iter() {
                all.push_back(a);
            }
        }
        all
    }

    /// Number of addresses currently on the allowlist, in O(1).
    ///
    /// Backed by a maintained counter rather than walking the allowlist
    /// pages.  The counter is incremented when a brand-new address is
    /// appended and decremented when an address is removed.  [`Self::suspend`]
    /// only flips `KycRecord::status` — the address's page membership (and
    /// thus this counter) is untouched, so suspended addresses are still
    /// counted.
    ///
    /// # Parameters
    /// None.
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// None.
    ///
    /// # Events
    /// None.
    pub fn get_allowlist_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::AllowlistCount)
            .unwrap_or(0u32)
    }

    /// Page through the allowlist without transferring the whole list.
    ///
    /// `offset` is the number of addresses to skip from the start of the
    /// allowlist; `limit` is the maximum number of addresses to return.
    /// `limit` is clamped to [`MAX_ALLOWLIST_PAGE_SIZE`] — passing `0` or a
    /// value above the maximum returns up to the maximum page size.  Passing
    /// an `offset` at or beyond the end of the list returns an empty `Vec`,
    /// which is how callers detect the final page.
    ///
    /// # Parameters
    /// - `offset`: Number of addresses to skip (0-based).
    /// - `limit`: Maximum number of addresses to return; clamped to
    ///   [`MAX_ALLOWLIST_PAGE_SIZE`].
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// None.
    ///
    /// # Events
    /// None.
    pub fn get_allowlist_page(env: Env, offset: u32, limit: u32) -> Vec<Address> {
        let limit = if limit == 0 || limit > MAX_ALLOWLIST_PAGE_SIZE {
            MAX_ALLOWLIST_PAGE_SIZE
        } else {
            limit
        };
        let mut result = Vec::new(&env);
        let mut skipped: u32 = 0;
        let (current_page, _) = Self::allowlist_meta(&env);
        for page_idx in 0..=current_page {
            if result.len() as u32 >= limit {
                break;
            }
            let page: Option<Vec<Address>> = env
                .storage()
                .persistent()
                .get(&DataKey::AllowlistPage(page_idx));
            let page = match page {
                Some(p) if !p.is_empty() => p,
                _ => continue,
            };
            for a in page.iter() {
                if skipped < offset {
                    skipped += 1;
                    continue;
                }
                if result.len() as u32 >= limit {
                    break;
                }
                result.push_back(a);
            }
        }
        result
    }

    /// Block an entire jurisdiction by ISO-3166-1 alpha-2 country code.
    ///
    /// Once blocked, any address whose KYC record carries that jurisdiction
    /// will fail [`Self::is_allowed`], even if the record itself is
    /// `Approved` and unexpired.  Idempotent: blocking an already-blocked
    /// jurisdiction does not emit a second event.
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `jurisdiction`: Two-letter ISO country code to block.  Normalized
    ///   (whitespace stripped, uppercased) before storage.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    /// - [`Error::InvalidJurisdiction`] — not a valid two-letter alpha code.
    ///
    /// # Events
    /// Emits topic `("blockjur",)` with data `jurisdiction`.
    pub fn block_jurisdiction(env: Env, admin: Address, jurisdiction: String) {
        Self::require_admin(&env, &admin);
        let jurisdiction = normalize_jurisdiction(&env, &jurisdiction);
        let already_blocked: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Blocked(jurisdiction.clone()))
            .unwrap_or(false);
        env.storage()
            .persistent()
            .set(&DataKey::Blocked(jurisdiction.clone()), &true);
        if !already_blocked {
            let mut list = Self::blocked_list(&env);
            list.push_back(jurisdiction.clone());
            env.storage().instance().set(&DataKey::BlockedList, &list);
        }
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("blockjur"),), jurisdiction);
    }

    /// Un-block a previously blocked jurisdiction.
    ///
    /// Removes the jurisdiction from the blocked set and from the ordered
    /// blocked-jurisdiction list.  Idempotent: calling on a jurisdiction that
    /// is not blocked succeeds silently.
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `jurisdiction`: Two-letter ISO country code to unblock.  Normalized
    ///   before the lookup.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    /// - [`Error::InvalidJurisdiction`] — not a valid two-letter alpha code.
    ///
    /// # Events
    /// Emits topic `("unblkjur",)` with data `jurisdiction`.
    pub fn unblock_jurisdiction(env: Env, admin: Address, jurisdiction: String) {
        Self::require_admin(&env, &admin);
        let jurisdiction = normalize_jurisdiction(&env, &jurisdiction);
        env.storage()
            .persistent()
            .remove(&DataKey::Blocked(jurisdiction.clone()));
        let list = Self::blocked_list(&env);
        let mut next = Vec::new(&env);
        for j in list.iter() {
            if j != jurisdiction {
                next.push_back(j);
            }
        }
        env.storage().instance().set(&DataKey::BlockedList, &next);
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("unblkjur"),), jurisdiction);
    }

    /// Returns `true` if the given jurisdiction is currently blocked.
    ///
    /// The `jurisdiction` is normalized before the lookup, so `"us"` and
    /// `"US"` are treated identically.
    ///
    /// # Parameters
    /// - `jurisdiction`: Two-letter ISO country code to query.
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// - [`Error::InvalidJurisdiction`] — not a valid two-letter alpha code.
    ///
    /// # Events
    /// None.
    pub fn is_jurisdiction_blocked(env: Env, jurisdiction: String) -> bool {
        let jurisdiction = normalize_jurisdiction(&env, &jurisdiction);
        env.storage()
            .persistent()
            .get(&DataKey::Blocked(jurisdiction))
            .unwrap_or(false)
    }

    /// Returns the ordered list of every jurisdiction currently blocked.
    ///
    /// The list is maintained in insertion order (earliest block first) and
    /// is kept in sync with the individual `Blocked(String)` flags, so
    /// callers read the authoritative set directly rather than inferring it
    /// from the absence of approved addresses.
    ///
    /// # Parameters
    /// None.
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// None.
    ///
    /// # Events
    /// None.
    pub fn get_blocked_jurisdictions(env: Env) -> Vec<String> {
        Self::blocked_list(&env)
    }

    /// Prune expired records from the allowlist. Admin only (issue #21).
    ///
    /// Removes expired entries from persistent storage and the allowlist page
    /// so indexers and [`Self::get_allowlist`] no longer count them as
    /// Approved.
    ///
    /// `max_records` bounds how many individual allowlist entries this call
    /// will inspect before returning (issue #333), so a large allowlist can
    /// be pruned incrementally across several transactions.  Passing `0` means
    /// "no bound" — scan everything, preserving prior behaviour for small
    /// allowlists.  The return value is the number of entries left unexamined
    /// once the bound is hit; callers should invoke again until the return
    /// value is `0`, which indicates a full pass completed.
    ///
    /// Only absent or already-empty pages are skipped to keep cost
    /// proportional to live pages, not historical page count (issue #306).
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `max_records`: Maximum number of allowlist entries to inspect.
    ///   `0` means inspect all.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    ///
    /// # Events
    /// Emits topic `("expired", address)` with data `expires_at` for each
    /// record pruned.
    pub fn prune_expired(env: Env, admin: Address, max_records: u32) -> u32 {
        Self::require_admin(&env, &admin);
        let now = env.ledger().sequence();
        let (current_page, _) = Self::allowlist_meta(&env);
        let mut examined: u32 = 0;
        for page_idx in 0..=current_page {
            let page: Option<Vec<Address>> = env
                .storage()
                .persistent()
                .get(&DataKey::AllowlistPage(page_idx));
            let page = match page {
                Some(p) if !p.is_empty() => p,
                _ => continue,
            };
            let mut next = Vec::new(&env);
            let mut changed = false;
            let mut stopped_early = false;
            let mut unexamined_in_page: u32 = 0;
            for addr in page.iter() {
                if max_records != 0 && examined >= max_records {
                    // Bound reached: keep this and every remaining entry in
                    // the page untouched, to be examined on a later call.
                    next.push_back(addr);
                    stopped_early = true;
                    unexamined_in_page += 1;
                    continue;
                }
                examined += 1;
                let record: Option<KycRecord> = env
                    .storage()
                    .persistent()
                    .get(&DataKey::Record(addr.clone()));
                let keep = match record {
                    Some(ref r) => {
                        if r.expires_at != 0 && now >= r.expires_at {
                            env.storage()
                                .persistent()
                                .remove(&DataKey::Record(addr.clone()));
                            env.storage()
                                .persistent()
                                .remove(&DataKey::AllowlistPageOf(addr.clone()));
                            env.events()
                                .publish((symbol_short!("expired"), addr.clone()), r.expires_at);
                            false
                        } else {
                            true
                        }
                    }
                    None => false,
                };
                if keep {
                    next.push_back(addr);
                } else {
                    changed = true;
                }
            }
            if changed {
                env.storage()
                    .persistent()
                    .set(&DataKey::AllowlistPage(page_idx), &next);
            }
            if stopped_early {
                // Entries left unexamined in this page, plus every entry on
                // pages not yet visited at all.
                let mut total_remaining = unexamined_in_page;
                for later_idx in (page_idx + 1)..=current_page {
                    let later: Option<Vec<Address>> = env
                        .storage()
                        .persistent()
                        .get(&DataKey::AllowlistPage(later_idx));
                    if let Some(p) = later {
                        total_remaining += p.len();
                    }
                }
                Self::bump_instance(&env);
                return total_remaining;
            }
        }
        Self::bump_instance(&env);
        0
    }

    /// Returns the configured admin address.
    ///
    /// # Parameters
    /// None.
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    ///
    /// # Events
    /// None.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error(&env, Error::NotInitialized))
    }

    /// Propose a new admin. The role does not transfer until `new_admin` calls
    /// [`Self::accept_admin`] (issue #4).
    ///
    /// The two-step handover makes a mistyped `new_admin` harmless — re-propose
    /// or cancel — rather than permanently bricking administration in a single
    /// call.
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `new_admin`: Address being nominated as successor.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    ///
    /// # Events
    /// Emits topic `("proposed", admin)` with data `new_admin`.
    pub fn propose_admin(env: Env, admin: Address, new_admin: Address) {
        Self::require_admin(&env, &admin);
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("proposed"), admin), new_admin);
    }

    /// Cancel a pending admin proposal.
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    /// - [`Error::NoPendingAdmin`] — no proposal is currently in flight.
    ///
    /// # Events
    /// Emits topic `("cancelled", admin)` with data `()`.
    pub fn cancel_admin_proposal(env: Env, admin: Address) {
        Self::require_admin(&env, &admin);
        if !env.storage().instance().has(&DataKey::PendingAdmin) {
            panic_with_error(&env, Error::NoPendingAdmin);
        }
        env.storage().instance().remove(&DataKey::PendingAdmin);
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("cancelled"), admin), ());
    }

    /// Accept a pending admin proposal and complete the handover.
    ///
    /// Must be called by the proposed successor (`new_admin`); the role only
    /// ever moves here, never in [`Self::propose_admin`] (issue #4).
    /// Emits `set_admin` carrying both the previous and new admin so
    /// off-chain indexers can observe this security-critical transition
    /// (issue #2).
    ///
    /// # Parameters
    /// - `new_admin`: The address accepting the admin role.  Must match the
    ///   pending proposal and must sign the transaction.
    ///
    /// # Authority
    /// `new_admin` must sign the transaction (`new_admin.require_auth()`).
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::NoPendingAdmin`] — no proposal is currently in flight.
    /// - [`Error::Unauthorized`] — `new_admin` does not match the pending
    ///   proposal.
    ///
    /// # Events
    /// Emits topic `("set_admin", old_admin)` with data `new_admin`.
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
        let old_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error(&env, Error::NotInitialized));
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("set_admin"), old_admin), new_admin);
    }

    /// Returns the address currently proposed as the next admin, if any.
    ///
    /// Returns `None` when no proposal is in flight.
    ///
    /// # Parameters
    /// None.
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// None.
    ///
    /// # Events
    /// None.
    pub fn get_pending_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::PendingAdmin)
    }

    // ---- minimum holding period (issue #454) ----

    /// Set (or clear) a minimum holding period for `holder`.
    ///
    /// `min_ledgers = 0` removes the requirement entirely. Any positive
    /// value means the holder must wait at least `min_ledgers` ledger
    /// sequences after their first token acquisition before they are
    /// permitted to transfer. The clock starts when `record_acquisition` is
    /// first called for the holder; subsequent top-ups do not reset it.
    ///
    /// Admin only. Emits `minhld` with `(holder, min_ledgers)`.
    ///
    /// Errors: `Unauthorized (#5)`, `NotInitialized (#2)`.
    pub fn set_min_holding_period(
        env: Env,
        admin: Address,
        holder: Address,
        min_ledgers: u64,
    ) {
        Self::require_admin(&env, &admin);
        let key = DataKey::MinHoldingPeriod(holder.clone());
        if min_ledgers == 0 {
            env.storage().persistent().remove(&key);
        } else {
            env.storage().persistent().set(&key, &min_ledgers);
            env.storage().persistent().extend_ttl(
                &key,
                INSTANCE_LIFETIME_THRESHOLD,
                INSTANCE_BUMP_AMOUNT,
            );
        }
        Self::bump_instance(&env);
        env.events()
            .publish((symbol_short!("minhld"), holder), min_ledgers);
    }

    /// Record the ledger sequence at which `holder` first acquired tokens.
    ///
    /// This is intended to be called by the asset-token contract (or any
    /// privileged caller the admin authorises) immediately after a mint or
    /// transfer that delivers tokens to `holder` for the first time. If a
    /// `FirstAcquiredLedger` entry already exists for this holder it is left
    /// untouched — the clock always starts at the *earliest* acquisition so
    /// that a top-up does not restart the lock-up window.
    ///
    /// Callers that only want `is_allowed` to enforce the holding period
    /// must call this before the first transfer *out* by the holder is
    /// attempted; if the record is absent, `is_allowed` treats the holding
    /// period as already satisfied (fail-open for backwards compatibility).
    ///
    /// Admin only. Emits `acquired` with `(holder, ledger)`.
    ///
    /// Errors: `Unauthorized (#5)`, `NotInitialized (#2)`.
    pub fn record_acquisition(env: Env, admin: Address, holder: Address) {
        Self::require_admin(&env, &admin);
        let key = DataKey::FirstAcquiredLedger(holder.clone());
        if !env.storage().persistent().has(&key) {
            let now = env.ledger().sequence();
            env.storage().persistent().set(&key, &(now as u64));
            env.storage().persistent().extend_ttl(
                &key,
                INSTANCE_LIFETIME_THRESHOLD,
                INSTANCE_BUMP_AMOUNT,
            );
            env.events()
                .publish((symbol_short!("acquired"), holder), now as u64);
        }
        Self::bump_instance(&env);
    }

    /// Return the minimum holding period (in ledgers) for `holder`, if any.
    ///
    /// Returns `None` when no holding period has been set for this address.
    pub fn get_min_holding_period(env: Env, holder: Address) -> Option<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::MinHoldingPeriod(holder))
    }

    /// Return the ledger sequence at which `holder` first acquired tokens,
    /// if that has been recorded via `record_acquisition`.
    pub fn get_first_acquired_ledger(env: Env, holder: Address) -> Option<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::FirstAcquiredLedger(holder))
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

    /// The current blocked-jurisdiction list, or empty if none are blocked.
    fn blocked_list(env: &Env) -> Vec<String> {
        env.storage()
            .instance()
            .get(&DataKey::BlockedList)
            .unwrap_or_else(|| Vec::new(env))
    }

    fn bump_instance(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Current (page index, length of that page) append cursor. Defaults to
    /// an empty page 0 when nothing has been added yet.
    fn allowlist_meta(env: &Env) -> (u32, u32) {
        env.storage()
            .instance()
            .get(&DataKey::AllowlistMeta)
            .unwrap_or((0u32, 0u32))
    }

    /// Append a new address to the current allowlist page, rolling over to a
    /// fresh page once the current one is full (issue #177).
    fn append_to_allowlist(env: &Env, address: &Address) {
        let (mut page_idx, mut page_len) = Self::allowlist_meta(env);
        if page_len >= ALLOWLIST_PAGE_SIZE {
            page_idx += 1;
            page_len = 0;
        }
        let mut page: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AllowlistPage(page_idx))
            .unwrap_or_else(|| Vec::new(env));
        page.push_back(address.clone());
        env.storage()
            .persistent()
            .set(&DataKey::AllowlistPage(page_idx), &page);
        env.storage()
            .persistent()
            .set(&DataKey::AllowlistPageOf(address.clone()), &page_idx);
        page_len += 1;
        env.storage()
            .instance()
            .set(&DataKey::AllowlistMeta, &(page_idx, page_len));

        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::AllowlistCount)
            .unwrap_or(0u32);
        env.storage()
            .instance()
            .set(&DataKey::AllowlistCount, &(count + 1));
    }

    /// Remove an address from whichever page it lives on. Leaves the page
    /// under-full rather than repacking pages, which keeps removal O(page
    /// size) instead of O(list size) (issue #177).
    ///
    /// # Fragmentation tradeoff (issue #308)
    ///
    /// This is an intentional design decision: removal rewrites the page
    /// without the evicted address but never compacts it into a neighbouring
    /// page or decrements the append cursor. As a consequence, on a
    /// long-lived allowlist with heavy churn a page can end up mostly empty
    /// while `append_to_allowlist` has already moved on to a later page.
    /// That permanently under-full page will still be visited by
    /// `get_allowlist` and `prune_expired` on every call (though both now
    /// skip pages that are absent or empty — issue #306).
    ///
    /// The alternative — compacting two half-full pages into one and
    /// rewriting `AllowlistPageOf` for every moved address — costs O(page
    /// size) writes per removal and re-introduces the O(list size) worst
    /// case that paging was designed to avoid.
    ///
    /// If fragmentation becomes a practical concern (e.g. a significant
    /// fraction of pages have fewer than ~10 live entries), operators can
    /// run a periodic off-chain reorganisation by removing and re-adding
    /// all current members, or a future contract upgrade can add an explicit
    /// `compact` admin operation.
    fn remove_from_allowlist(env: &Env, address: &Address) {
        let page_idx: Option<u32> = env
            .storage()
            .persistent()
            .get(&DataKey::AllowlistPageOf(address.clone()));
        let Some(page_idx) = page_idx else {
            return;
        };
        let page: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AllowlistPage(page_idx))
            .unwrap_or_else(|| Vec::new(env));
        let mut next = Vec::new(env);
        for a in page.iter() {
            if a != *address {
                next.push_back(a);
            }
        }
        env.storage()
            .persistent()
            .set(&DataKey::AllowlistPage(page_idx), &next);
        env.storage()
            .persistent()
            .remove(&DataKey::AllowlistPageOf(address.clone()));

        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::AllowlistCount)
            .unwrap_or(0u32);
        env.storage()
            .instance()
            .set(&DataKey::AllowlistCount, &count.saturating_sub(1));
    }
}

/// Small wrapper so call sites read cleanly and never use `unwrap`/`expect`.
fn panic_with_error(env: &Env, error: Error) -> ! {
    soroban_sdk::panic_with_error!(env, error)
}

/// Normalize and validate a jurisdiction code (issue #47).
/// Strips spaces, uppercases, then enforces exactly 2 ASCII alpha characters
/// so only real ISO-3166-1 alpha-2 codes (e.g. "US", "KE") are accepted.
fn normalize_jurisdiction(env: &Env, jurisdiction: &String) -> String {
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
    String::from_bytes(env, &buf[..2])
}

#[cfg(test)]
mod test;
