#![no_std]
//! # Asset Token Contract
//!
//! A compliant token representing a tokenized real-world asset (real estate,
//! invoice, commodity, ...). Every transfer is gated by an external compliance
//! contract: both the sender and the recipient must pass `is_allowed` before any
//! balance moves. Minting is likewise gated on the recipient, so only KYC-approved
//! addresses can ever hold the asset.
//!
//! Valuation is stored in USD cents (`i128`). Amounts are integer token units in
//! the token's own `decimals` base. A zero-decimal token therefore cannot
//! represent fractional token units; downstream proportional calculations can
//! floor small claims to zero.
//!
//! ## Admin is independent of the compliance admin (issue #3)
//!
//! The `admin` stored in [`AssetMetadata`] (mint/pause/valuation/etc.) and the
//! admin of the linked `compliance_contract` are tracked in entirely separate
//! storage and are never compared to each other. Only `compliance_contract`'s
//! `is_allowed` result is consulted here; its admin's identity is opaque to
//! this contract. A real issuer can therefore have compliance administered by
//! a dedicated compliance officer while a different address runs the asset
//! token. `scripts/deploy.sh` uses one address for both only as a convenience
//! default for its sample single-operator deployment.

#[cfg(test)]
extern crate std;

use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, symbol_short, Address,
    Env, String, Vec,
};

/// Cross-contract client for the compliance contract. Only the method the asset
/// token needs is declared here, decoupling the two contracts at build time.
#[contractclient(name = "ComplianceClient")]
pub trait ComplianceInterface {
    fn is_allowed(env: Env, address: Address) -> bool;
}

/// On-chain metadata describing the tokenized asset.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetMetadata {
    pub name: String,
    pub symbol: String,
    /// e.g. "real_estate", "invoice", "commodity".
    pub asset_type: String,
    pub total_supply: i128,
    pub decimals: u32,
    pub admin: Address,
    pub compliance_contract: Address,
    pub asset_description: String,
    /// Asset value in USD cents.
    pub valuation: i128,
    pub paused: bool,
    /// Optional emergency-pause delegate (issue #1). May call `pause` but
    /// not `unpause`, `mint`, `mint_batch`, or any other admin action.
    /// Absent (`None`) by default.
    pub guardian: Option<Address>,
    /// Address nominated by the current admin via `propose_admin`, pending
    /// acceptance via `accept_admin` (issue #4). Absent when there is no
    /// proposal in flight.
    pub pending_admin: Option<Address>,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Metadata,
    Balance(Address),
    Allowance(Address, Address),
}

/// SEP-41 allowance record: amount plus the ledger sequence it expires at.
#[contracttype]
#[derive(Clone)]
pub struct AllowanceValue {
    pub amount: i128,
    pub expiration_ledger: u32,
}

#[contracterror]
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InsufficientBalance = 4,
    InvalidAmount = 5,
    Paused = 6,
    SenderNotCompliant = 7,
    RecipientNotCompliant = 8,
    Overflow = 9,
    InvalidInput = 10,
    InvalidCompliance = 11,
    /// `accept_admin` or `cancel_admin_proposal` called with no pending
    /// admin proposal on file (issue #4).
    NoPendingAdmin = 12,
    InsufficientAllowance = 13,
    ValuationChangeTooLarge = 14,
}

/// Maximum byte lengths for string metadata fields (issue #46).
const MAX_NAME_LEN: u32 = 64;
const MAX_SYMBOL_LEN: u32 = 16;
const MAX_ASSET_TYPE_LEN: u32 = 32;
const MAX_DESC_LEN: u32 = 256;

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// `update_valuation` rejects any single change larger than this fraction of
/// the previous valuation, expressed in basis points (5_000 = 50%). This
/// guards against a mistyped USD-cent value propagating to the registry's
/// total value locked. Chosen as a generous-but-bounded ceiling: legitimate
/// re-appraisals rarely move a real-world asset's value by more than half in
/// one update, while a fat-fingered extra digit (a 10x+ change) is reliably
/// caught. A valuation of `0` is exempt since there is no prior magnitude to
/// compare against.
const MAX_VALUATION_CHANGE_BPS: i128 = 5_000;
const BPS_DENOMINATOR: i128 = 10_000;

/// Contract ABI/behavior version. Bump on any change to storage layout or
/// externally observable behavior so clients and the indexer can detect it.
pub const VERSION: u32 = 1;

#[contract]
pub struct AssetTokenContract;

#[contractimpl]
impl AssetTokenContract {
    /// Returns the contract's ABI/behavior version number.
    ///
    /// Callers and indexers can use this to detect schema or behavior changes
    /// without probing individual storage entries.
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

    /// Initialize the token, store metadata, and mint the full `total_supply`
    /// to the admin. Callable exactly once.
    ///
    /// The admin must already be compliance-approved on `compliance_contract`
    /// to hold the initial supply; [`Error::RecipientNotCompliant`] is raised
    /// otherwise.
    ///
    /// # Parameters
    /// - `admin`: Initial admin address (mint / pause / valuation / etc.).
    ///   Must authorize the call and be compliance-approved.
    /// - `name`: Human-readable token name (1–64 bytes).
    /// - `symbol`: Ticker symbol (1–16 bytes).
    /// - `asset_type`: Asset class, e.g. `"real_estate"` (1–32 bytes).
    /// - `total_supply`: Total tokens to mint.  Must be ≥ 0.
    /// - `decimals`: Decimal precision of token amounts.
    /// - `compliance_contract`: Address of the compliance contract that gates
    ///   transfers and mints.
    /// - `asset_description`: Free-text description of the asset (1–256 bytes).
    /// - `valuation`: Initial USD-cent valuation of the asset.  Must be ≥ 0.
    ///
    /// # Authority
    /// `admin` must sign the transaction (`admin.require_auth()`).
    ///
    /// # Errors
    /// - [`Error::AlreadyInitialized`] — contract was already initialized.
    /// - [`Error::InvalidAmount`] — `total_supply` or `valuation` is negative.
    /// - [`Error::InvalidInput`] — any string field is empty or exceeds its
    ///   maximum length.
    /// - [`Error::RecipientNotCompliant`] — `admin` is not approved on the
    ///   compliance contract.
    ///
    /// # Events
    /// Emits topic `("genesis", admin)` with data `total_supply` (one-time
    /// initialization marker).  Also emits topic `("mint", admin)` with data
    /// `total_supply` so indexers that sum `mint` events see the initial
    /// allocation (issue #176).
    #[allow(clippy::too_many_arguments)]
    pub fn initialize(
        env: Env,
        admin: Address,
        name: String,
        symbol: String,
        asset_type: String,
        total_supply: i128,
        decimals: u32,
        compliance_contract: Address,
        asset_description: String,
        valuation: i128,
    ) {
        if env.storage().instance().has(&DataKey::Metadata) {
            panic_err(&env, Error::AlreadyInitialized);
        }
        admin.require_auth();
        if total_supply < 0 || valuation < 0 {
            panic_err(&env, Error::InvalidAmount);
        }
        // Validate string metadata: non-empty and within max lengths (issue #46).
        check_str(&env, &name, 1, MAX_NAME_LEN);
        check_str(&env, &symbol, 1, MAX_SYMBOL_LEN);
        check_str(&env, &asset_type, 1, MAX_ASSET_TYPE_LEN);
        check_str(&env, &asset_description, 1, MAX_DESC_LEN);
        // The admin must be allowed to hold the initial supply.
        if !Self::compliant(&env, &compliance_contract, &admin) {
            panic_err(&env, Error::RecipientNotCompliant);
        }
        let metadata = AssetMetadata {
            name,
            symbol,
            asset_type,
            total_supply,
            decimals,
            admin: admin.clone(),
            compliance_contract,
            asset_description,
            valuation,
            paused: false,
            guardian: None,
            pending_admin: None,
        };
        env.storage().instance().set(&DataKey::Metadata, &metadata);
        Self::set_balance(&env, &admin, total_supply);
        Self::bump(&env);
        // Emit `genesis` for the one-time initialization event, and also `mint`
        // (matching `mint`/`mint_batch`) so indexers that sum `mint` topics to
        // track supply see the initial allocation instead of under-reporting it
        // (issue #176). Each topic carries a single `total_supply` value, not a
        // duplicated tuple (issue #175).
        env.events()
            .publish((symbol_short!("genesis"), admin.clone()), total_supply);
        env.events()
            .publish((symbol_short!("mint"), admin.clone()), total_supply);
    }

    /// Transfer `amount` tokens from `from` to `to`.
    ///
    /// Both parties must be compliance-approved and the token must not be
    /// paused.  `amount` must be strictly positive.
    ///
    /// ## Deliberate policy: zero-amount transfers
    /// A `transfer` of `0` is rejected with [`Error::InvalidAmount`] via
    /// [`Self::check_amount`], rather than silently succeeding as a no-op.
    /// A zero-amount call that emits a `transfer` event with no balance
    /// change is misleading to indexers/observers and still costs the
    /// caller fees for nothing; requiring a strictly positive amount makes
    /// that intent explicit and forces callers to skip the call entirely
    /// instead of relying on the contract to swallow it.
    ///
    /// ## Deliberate policy: self-transfers (`from == to`)
    /// A transfer where `from == to` is **allowed** (it is not rejected)
    /// but is short-circuited into a pure no-op: balances are not touched,
    /// but a `transfer` event is still emitted with
    /// `new_from_bal == new_to_bal == from_bal` so downstream indexers see a
    /// consistent event shape.  Rejecting self-transfers outright would be an
    /// additional special case for callers to defend against; treating it as
    /// an explicit no-op is simpler and cannot corrupt balances.
    ///
    /// # Parameters
    /// - `from`: Sender address.  Must authorize the call, be
    ///   compliance-approved, and have a sufficient balance.
    /// - `to`: Recipient address.  Must be compliance-approved.
    /// - `amount`: Number of tokens to transfer.  Must be > 0.
    ///
    /// # Authority
    /// `from` must sign the transaction (`from.require_auth()`).
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::InvalidAmount`] — `amount` ≤ 0.
    /// - [`Error::Paused`] — token is paused.
    /// - [`Error::SenderNotCompliant`] — `from` is not compliance-approved.
    /// - [`Error::RecipientNotCompliant`] — `to` is not compliance-approved.
    /// - [`Error::InsufficientBalance`] — `from` balance < `amount`.
    /// - [`Error::Overflow`] — recipient balance would overflow `i128`.
    ///
    /// # Events
    /// Emits topic `("transfer", from, to)` with data
    /// `(amount, new_from_bal, new_to_bal)`.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        Self::check_amount(&env, amount);
        let meta = Self::metadata(&env);
        if meta.paused {
            panic_err(&env, Error::Paused);
        }
        if !Self::compliant(&env, &meta.compliance_contract, &from) {
            panic_err(&env, Error::SenderNotCompliant);
        }
        if !Self::compliant(&env, &meta.compliance_contract, &to) {
            panic_err(&env, Error::RecipientNotCompliant);
        }
        let from_bal = Self::balance(env.clone(), from.clone());
        if from_bal < amount {
            panic_err(&env, Error::InsufficientBalance);
        }
        // A self-transfer is a no-op: reading `to_bal` and writing it back after
        // the `from` write would otherwise overwrite the debit and inflate the
        // balance. Skip the balance moves entirely once funds/compliance checks
        // have passed.
        if from == to {
            Self::bump(&env);
            // Keep the payload shape identical to the normal path
            // (amount, new_from_bal, new_to_bal) so indexers can decode both
            // uniformly; a self-transfer leaves the balance unchanged.
            env.events().publish(
                (symbol_short!("transfer"), from, to),
                (amount, from_bal, from_bal),
            );
            return;
        }
        let to_bal = Self::balance(env.clone(), to.clone());
        let new_from_bal = from_bal - amount;
        Self::set_balance(&env, &from, new_from_bal);
        let new_to_bal = to_bal
            .checked_add(amount)
            .unwrap_or_else(|| panic_err(&env, Error::Overflow));
        Self::set_balance(&env, &to, new_to_bal);
        Self::bump(&env);
        // Include post-balances so indexers don't need to re-read state (issue #41).
        env.events().publish(
            (symbol_short!("transfer"), from, to),
            (amount, new_from_bal, new_to_bal),
        );
    }

    /// Mint `amount` new tokens to a compliance-approved recipient. Admin only.
    ///
    /// ## Deliberate policy: mint gates the recipient
    /// Unlike `transfer`, `mint` has no "sender" to gate — but the recipient
    /// (`to`) is checked against the compliance contract exactly like the
    /// `to` side of a `transfer`, and reverts with
    /// [`Error::RecipientNotCompliant`] if it fails.  This is deliberate:
    /// minting is the only way new supply enters circulation, so if it were
    /// not gated an admin could hand tokens to an unverified address that no
    /// `transfer` could ever reach.  [`Self::mint_batch`] applies the same
    /// per-recipient check to every entry in the batch.  See
    /// `docs/asset-token.md` for the documented decision.
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `to`: Recipient address.  Must be compliance-approved.
    /// - `amount`: Number of tokens to mint.  Must be > 0.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    /// - [`Error::InvalidAmount`] — `amount` ≤ 0.
    /// - [`Error::Paused`] — token is paused.
    /// - [`Error::RecipientNotCompliant`] — `to` is not compliance-approved.
    /// - [`Error::Overflow`] — total supply or recipient balance would overflow
    ///   `i128`.
    ///
    /// # Events
    /// Emits topic `("mint", to)` with data `amount`.
    pub fn mint(env: Env, admin: Address, to: Address, amount: i128) {
        let mut meta = Self::require_admin(&env, &admin);
        Self::check_amount(&env, amount);
        if meta.paused {
            panic_err(&env, Error::Paused);
        }
        if !Self::compliant(&env, &meta.compliance_contract, &to) {
            panic_err(&env, Error::RecipientNotCompliant);
        }
        let new_supply = meta
            .total_supply
            .checked_add(amount)
            .unwrap_or_else(|| panic_err(&env, Error::Overflow));
        let to_bal = Self::balance(env.clone(), to.clone());
        let new_to_bal = to_bal
            .checked_add(amount)
            .unwrap_or_else(|| panic_err(&env, Error::Overflow));
        Self::set_balance(&env, &to, new_to_bal);
        meta.total_supply = new_supply;
        env.storage().instance().set(&DataKey::Metadata, &meta);
        Self::bump(&env);
        env.events().publish((symbol_short!("mint"), to), amount);
    }

    /// Batch-mint to multiple compliance-approved recipients in a single call.
    /// Admin only.
    ///
    /// Each `(recipient, amount)` pair is checked individually against the
    /// compliance contract; if any recipient fails compliance the entire call
    /// reverts.
    ///
    /// Cost model: [`Self::compliant`] (a cross-contract call into
    /// `compliance_contract`) is invoked once per entry in `recipients`, so
    /// both resource cost and cross-contract call count scale linearly with
    /// `recipients.len()`.  Split very large batches across multiple
    /// `mint_batch` calls if needed.
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `recipients`: Vec of `(to, amount)` pairs.  Each amount must be > 0
    ///   and each recipient must be compliance-approved.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    /// - [`Error::InvalidAmount`] — any amount in the batch is ≤ 0.
    /// - [`Error::Paused`] — token is paused.
    /// - [`Error::RecipientNotCompliant`] — any recipient is not
    ///   compliance-approved.
    /// - [`Error::Overflow`] — total supply or any recipient balance would
    ///   overflow `i128`.
    ///
    /// # Events
    /// Emits one topic `("mint", to)` with data `amount` per entry in the
    /// batch.
    pub fn mint_batch(env: Env, admin: Address, recipients: Vec<(Address, i128)>) {
        let mut meta = Self::require_admin(&env, &admin);
        if meta.paused {
            panic_err(&env, Error::Paused);
        }
        let mut new_supply = meta.total_supply;
        for (to, amount) in recipients.iter() {
            Self::check_amount(&env, amount);
            if !Self::compliant(&env, &meta.compliance_contract, &to) {
                panic_err(&env, Error::RecipientNotCompliant);
            }
            new_supply = new_supply
                .checked_add(amount)
                .unwrap_or_else(|| panic_err(&env, Error::Overflow));
            let to_bal = Self::balance(env.clone(), to.clone());
            let new_to_bal = to_bal
                .checked_add(amount)
                .unwrap_or_else(|| panic_err(&env, Error::Overflow));
            Self::set_balance(&env, &to, new_to_bal);
            env.events().publish((symbol_short!("mint"), to), amount);
        }
        meta.total_supply = new_supply;
        env.storage().instance().set(&DataKey::Metadata, &meta);
        Self::bump(&env);
    }

    /// Burn `amount` of the caller's own tokens, reducing total supply.
    ///
    /// ## Deliberate policy: a suspended holder may not burn
    /// `burn` checks the caller against the compliance contract exactly like
    /// the `from` side of a `transfer`, and reverts with
    /// [`Error::SenderNotCompliant`] if the caller is not currently approved
    /// (whether suspended or removed outright).  Burning still moves balance
    /// and total-supply state, so it is treated as a balance-changing
    /// operation subject to the same compliance gate.  A holder who needs to
    /// exit while suspended must first be reinstated.  See
    /// `docs/asset-token.md` for the documented decision.
    ///
    /// # Parameters
    /// - `from`: Address burning tokens.  Must authorize the call,
    ///   be compliance-approved, and have a sufficient balance.
    /// - `amount`: Number of tokens to burn.  Must be > 0.
    ///
    /// # Authority
    /// `from` must sign the transaction (`from.require_auth()`).
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::InvalidAmount`] — `amount` ≤ 0.
    /// - [`Error::Paused`] — token is paused.
    /// - [`Error::SenderNotCompliant`] — `from` is not compliance-approved.
    /// - [`Error::InsufficientBalance`] — `from` balance < `amount`.
    /// - [`Error::Overflow`] — total supply underflows (should not occur in
    ///   practice).
    ///
    /// # Events
    /// Emits topic `("burn", from)` with data `amount`.
    pub fn burn(env: Env, from: Address, amount: i128) {
        from.require_auth();
        Self::check_amount(&env, amount);
        let mut meta = Self::metadata(&env);
        if meta.paused {
            panic_err(&env, Error::Paused);
        }
        if !Self::compliant(&env, &meta.compliance_contract, &from) {
            panic_err(&env, Error::SenderNotCompliant);
        }
        let from_bal = Self::balance(env.clone(), from.clone());
        if from_bal < amount {
            panic_err(&env, Error::InsufficientBalance);
        }
        Self::set_balance(&env, &from, from_bal - amount);
        meta.total_supply = meta
            .total_supply
            .checked_sub(amount)
            .unwrap_or_else(|| panic_err(&env, Error::Overflow));
        env.storage().instance().set(&DataKey::Metadata, &meta);
        Self::bump(&env);
        env.events().publish((symbol_short!("burn"), from), amount);
    }

    /// Authorize `spender` to move up to `amount` of `from`'s tokens until
    /// `expiration_ledger` (inclusive). Passing `amount == 0` clears the
    /// allowance regardless of `expiration_ledger`. (SEP-41)
    ///
    /// Paused tokens reject `approve` the same as `transfer`, since an
    /// approval is only meaningful if a matching `transfer_from` could later
    /// succeed (documented in `docs/asset-token.md`).
    ///
    /// # Parameters
    /// - `from`: Token owner granting the allowance.  Must authorize the call.
    /// - `spender`: Address being authorized to spend.
    /// - `amount`: Maximum tokens spender may move.  Must be ≥ 0.  Pass `0`
    ///   to revoke.
    /// - `expiration_ledger`: Ledger sequence at which the allowance expires
    ///   (inclusive).  Must be ≥ current ledger when `amount > 0`.
    ///
    /// # Authority
    /// `from` must sign the transaction (`from.require_auth()`).
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::InvalidAmount`] — `amount` < 0.
    /// - [`Error::Paused`] — token is paused.
    /// - [`Error::InvalidInput`] — `amount > 0` and `expiration_ledger` is
    ///   already in the past.
    ///
    /// # Events
    /// Emits topic `("approve", from, spender)` with data
    /// `(amount, expiration_ledger)`.
    pub fn approve(
        env: Env,
        from: Address,
        spender: Address,
        amount: i128,
        expiration_ledger: u32,
    ) {
        from.require_auth();
        if amount < 0 {
            panic_err(&env, Error::InvalidAmount);
        }
        let meta = Self::metadata(&env);
        if meta.paused {
            panic_err(&env, Error::Paused);
        }
        if amount > 0 && expiration_ledger < env.ledger().sequence() {
            panic_err(&env, Error::InvalidInput);
        }
        let key = DataKey::Allowance(from.clone(), spender.clone());
        env.storage().temporary().set(
            &key,
            &AllowanceValue {
                amount,
                expiration_ledger,
            },
        );
        if amount > 0 {
            let live_for = expiration_ledger.saturating_sub(env.ledger().sequence());
            env.storage()
                .temporary()
                .extend_ttl(&key, live_for, live_for);
        }
        env.events().publish(
            (symbol_short!("approve"), from, spender),
            (amount, expiration_ledger),
        );
    }

    /// Returns the remaining amount `spender` may transfer from `from`. (SEP-41)
    ///
    /// Returns `0` once `expiration_ledger` has passed, matching the spec's
    /// "expired allowances read as zero" semantics rather than returning a
    /// stale value.
    ///
    /// # Parameters
    /// - `from`: Token owner.
    /// - `spender`: Authorized spender.
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// None.
    ///
    /// # Events
    /// None.
    pub fn allowance(env: Env, from: Address, spender: Address) -> i128 {
        let key = DataKey::Allowance(from, spender);
        match env.storage().temporary().get::<_, AllowanceValue>(&key) {
            Some(v) if v.expiration_ledger >= env.ledger().sequence() => v.amount,
            _ => 0,
        }
    }

    /// Move `amount` from `from` to `to` using a prior [`Self::approve`].
    /// Subject to the same pause and compliance gates as [`Self::transfer`].
    /// (SEP-41)
    ///
    /// # Parameters
    /// - `spender`: Address exercising the allowance.  Must authorize the call.
    /// - `from`: Token owner.  Must be compliance-approved.
    /// - `to`: Recipient.  Must be compliance-approved.
    /// - `amount`: Number of tokens to move.  Must be > 0.
    ///
    /// # Authority
    /// `spender` must sign the transaction (`spender.require_auth()`).
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::InvalidAmount`] — `amount` ≤ 0.
    /// - [`Error::Paused`] — token is paused.
    /// - [`Error::SenderNotCompliant`] — `from` is not compliance-approved.
    /// - [`Error::RecipientNotCompliant`] — `to` is not compliance-approved.
    /// - [`Error::InsufficientAllowance`] — allowance expired or insufficient.
    /// - [`Error::InsufficientBalance`] — `from` balance < `amount`.
    /// - [`Error::Overflow`] — recipient balance would overflow `i128`.
    ///
    /// # Events
    /// Emits topic `("transfer", from, to)` with data
    /// `(amount, new_from_bal, new_to_bal)`.
    pub fn transfer_from(env: Env, spender: Address, from: Address, to: Address, amount: i128) {
        spender.require_auth();
        Self::check_amount(&env, amount);
        let meta = Self::metadata(&env);
        if meta.paused {
            panic_err(&env, Error::Paused);
        }
        if !Self::compliant(&env, &meta.compliance_contract, &from) {
            panic_err(&env, Error::SenderNotCompliant);
        }
        if !Self::compliant(&env, &meta.compliance_contract, &to) {
            panic_err(&env, Error::RecipientNotCompliant);
        }
        let key = DataKey::Allowance(from.clone(), spender.clone());
        let current = env
            .storage()
            .temporary()
            .get::<_, AllowanceValue>(&key)
            .unwrap_or(AllowanceValue {
                amount: 0,
                expiration_ledger: 0,
            });
        if current.expiration_ledger < env.ledger().sequence() || current.amount < amount {
            panic_err(&env, Error::InsufficientAllowance);
        }
        let from_bal = Self::balance(env.clone(), from.clone());
        if from_bal < amount {
            panic_err(&env, Error::InsufficientBalance);
        }
        let new_from_bal = from_bal - amount;
        Self::set_balance(&env, &from, new_from_bal);
        let to_bal = Self::balance(env.clone(), to.clone());
        let new_to_bal = to_bal
            .checked_add(amount)
            .unwrap_or_else(|| panic_err(&env, Error::Overflow));
        Self::set_balance(&env, &to, new_to_bal);
        env.storage().temporary().set(
            &key,
            &AllowanceValue {
                amount: current.amount - amount,
                expiration_ledger: current.expiration_ledger,
            },
        );
        Self::bump(&env);
        env.events().publish(
            (symbol_short!("transfer"), from, to),
            (amount, new_from_bal, new_to_bal),
        );
    }

    /// Returns the current token balance of `id`.
    ///
    /// Returns `0` for addresses that have never held tokens.
    ///
    /// # Parameters
    /// - `id`: Address to query.
    ///
    /// # Authority
    /// None — anyone may call.
    ///
    /// # Errors
    /// None.
    ///
    /// # Events
    /// None.
    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(id))
            .unwrap_or(0)
    }

    /// Returns the current total token supply.
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
    pub fn total_supply(env: Env) -> i128 {
        Self::metadata(&env).total_supply
    }

    /// Pause every balance-changing operation.
    ///
    /// Callable by the admin or by the optional guardian (issue #1) if one
    /// has been set via [`Self::set_guardian`].  The guardian cannot unpause,
    /// mint, or perform any other admin action.
    ///
    /// While paused, `transfer`, `transfer_from`, `mint`, `mint_batch`,
    /// `burn`, and `approve` all revert with [`Error::Paused`].  Read-only
    /// calls (`balance`, `allowance`, `get_metadata`, `total_supply`) keep
    /// working.  This is intentionally total: a pause is meant to freeze
    /// token state during an incident, not just block trading while admin
    /// actions continue.
    ///
    /// # Parameters
    /// - `caller`: Admin or guardian address.  Must authorize the call.
    ///
    /// # Authority
    /// `caller` must be the stored admin or the configured guardian, and must
    /// sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `caller` is neither the admin nor the
    ///   guardian.
    ///
    /// # Events
    /// Emits topic `("pause",)` with data `caller`.
    pub fn pause(env: Env, caller: Address) {
        caller.require_auth();
        let mut meta = Self::metadata(&env);
        let is_guardian = meta.guardian.as_ref() == Some(&caller);
        if meta.admin != caller && !is_guardian {
            panic_err(&env, Error::Unauthorized);
        }
        meta.paused = true;
        env.storage().instance().set(&DataKey::Metadata, &meta);
        Self::bump(&env);
        env.events().publish((symbol_short!("pause"),), caller);
    }

    /// Resume transfers and mints. Admin only; the guardian cannot unpause.
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
    ///
    /// # Events
    /// Emits topic `("unpause",)` with data `admin`.
    pub fn unpause(env: Env, admin: Address) {
        let mut meta = Self::require_admin(&env, &admin);
        meta.paused = false;
        env.storage().instance().set(&DataKey::Metadata, &meta);
        Self::bump(&env);
        env.events().publish((symbol_short!("unpause"),), admin);
    }

    /// Set or clear the optional guardian address. Admin only.
    ///
    /// Pass `None` to remove the guardian and restrict [`Self::pause`] back
    /// to the admin alone.
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `guardian`: `Some(address)` to set the guardian, or `None` to clear
    ///   it.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    ///
    /// # Events
    /// Emits topic `("guardian",)` with data `guardian` (the new guardian
    /// value, which may be `None`).
    pub fn set_guardian(env: Env, admin: Address, guardian: Option<Address>) {
        let mut meta = Self::require_admin(&env, &admin);
        meta.guardian = guardian;
        env.storage().instance().set(&DataKey::Metadata, &meta);
        Self::bump(&env);
        env.events()
            .publish((symbol_short!("guardian"),), meta.guardian);
    }

    /// Returns the full [`AssetMetadata`] struct.
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
    pub fn get_metadata(env: Env) -> AssetMetadata {
        Self::metadata(&env)
    }

    /// Update the recorded USD-cent valuation of the asset. Admin only.
    ///
    /// A single update may not move the valuation by more than
    /// `MAX_VALUATION_CHANGE_BPS` (50%) of its previous value, to guard
    /// against a mistyped USD-cent value propagating to the registry's TVL.
    /// A valuation of `0` is exempt since there is no prior magnitude to
    /// compare against.  Larger re-appraisals must be phased across multiple
    /// calls.
    ///
    /// This updates only the token metadata.  If this token is also registered
    /// in the registry, the registry's valuation is an independent snapshot
    /// from registration and is **not** updated by this call.  Use a separate
    /// registry update workflow when the records need to agree.
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `new_valuation`: New USD-cent valuation.  Must be ≥ 0.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    /// - [`Error::InvalidAmount`] — `new_valuation` < 0.
    /// - [`Error::ValuationChangeTooLarge`] — change exceeds 50% of the
    ///   previous valuation.
    /// - [`Error::Overflow`] — overflow while computing the allowed change
    ///   threshold.
    ///
    /// # Events
    /// Emits topic `("valuation",)` with data `(old_valuation, new_valuation)`.
    pub fn update_valuation(env: Env, admin: Address, new_valuation: i128) {
        let mut meta = Self::require_admin(&env, &admin);
        if new_valuation < 0 {
            panic_err(&env, Error::InvalidAmount);
        }
        let old_valuation = meta.valuation;
        if old_valuation > 0 {
            let diff = (new_valuation - old_valuation).abs();
            let max_change = old_valuation
                .checked_mul(MAX_VALUATION_CHANGE_BPS)
                .and_then(|v| v.checked_div(BPS_DENOMINATOR))
                .unwrap_or_else(|| panic_err(&env, Error::Overflow));
            if diff > max_change {
                panic_err(&env, Error::ValuationChangeTooLarge);
            }
        }
        meta.valuation = new_valuation;
        env.storage().instance().set(&DataKey::Metadata, &meta);
        Self::bump(&env);
        env.events().publish(
            (symbol_short!("valuation"),),
            (old_valuation, new_valuation),
        );
    }

    /// Point the token at a different compliance contract. Admin only.
    ///
    /// Does not re-validate existing holders against the new gate: a holder
    /// approved under the old contract keeps their balance even if the new
    /// contract would reject them.  It only checks that `compliance` implements
    /// `is_allowed` and approves the admin, to catch a misconfigured address
    /// before it bricks every transfer.
    ///
    /// # Parameters
    /// - `admin`: Current admin address.  Must authorize the call.
    /// - `compliance`: Address of the new compliance contract.  Must implement
    ///   [`ComplianceInterface`] and return `true` for `admin`.
    ///
    /// # Authority
    /// `admin` must be the stored admin and must sign the transaction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    /// - [`Error::Unauthorized`] — `admin` does not match the stored admin.
    /// - [`Error::InvalidCompliance`] — `compliance` does not implement
    ///   `is_allowed`, or does not approve the admin.
    ///
    /// # Events
    /// Emits topic `("setcomp",)` with data `(old_compliance, new_compliance)`.
    pub fn set_compliance(env: Env, admin: Address, compliance: Address) {
        let mut meta = Self::require_admin(&env, &admin);
        // Probe the target for the expected interface (`is_allowed`) before
        // accepting the swap: calling it here, before any state changes,
        // means an address that doesn't implement `ComplianceInterface`
        // traps this invocation instead of silently bricking every future
        // transfer once it's already wired in as the gate.
        if !Self::compliant(&env, &compliance, &admin) {
            panic_err(&env, Error::InvalidCompliance);
        }
        let old_compliance = meta.compliance_contract.clone();
        meta.compliance_contract = compliance.clone();
        env.storage().instance().set(&DataKey::Metadata, &meta);
        Self::bump(&env);
        env.events()
            .publish((symbol_short!("setcomp"),), (old_compliance, compliance));
    }

    /// Propose a new admin. The role does not transfer until `new_admin` calls
    /// [`Self::accept_admin`] (issue #4).
    ///
    /// The two-step handover makes a mistyped `new_admin` harmless — re-propose
    /// or cancel — rather than permanently bricking administration.  Note this
    /// does not affect the optional guardian (issue #1), which is set
    /// independently via [`Self::set_guardian`].
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
        let mut meta = Self::require_admin(&env, &admin);
        meta.pending_admin = Some(new_admin.clone());
        env.storage().instance().set(&DataKey::Metadata, &meta);
        Self::bump(&env);
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
        let mut meta = Self::require_admin(&env, &admin);
        if meta.pending_admin.is_none() {
            panic_err(&env, Error::NoPendingAdmin);
        }
        meta.pending_admin = None;
        env.storage().instance().set(&DataKey::Metadata, &meta);
        Self::bump(&env);
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
        let mut meta = Self::metadata(&env);
        let pending = meta
            .pending_admin
            .clone()
            .unwrap_or_else(|| panic_err(&env, Error::NoPendingAdmin));
        if pending != new_admin {
            panic_err(&env, Error::Unauthorized);
        }
        let old_admin = meta.admin.clone();
        meta.admin = new_admin.clone();
        meta.pending_admin = None;
        env.storage().instance().set(&DataKey::Metadata, &meta);
        Self::bump(&env);
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
    /// - [`Error::NotInitialized`] — contract not yet initialized.
    ///
    /// # Events
    /// None.
    pub fn get_pending_admin(env: Env) -> Option<Address> {
        Self::metadata(&env).pending_admin
    }

    // ---- internal helpers ----

    fn metadata(env: &Env) -> AssetMetadata {
        env.storage()
            .instance()
            .get(&DataKey::Metadata)
            .unwrap_or_else(|| panic_err(env, Error::NotInitialized))
    }

    fn require_admin(env: &Env, admin: &Address) -> AssetMetadata {
        let meta = Self::metadata(env);
        admin.require_auth();
        if meta.admin != *admin {
            panic_err(env, Error::Unauthorized);
        }
        meta
    }

    fn compliant(env: &Env, compliance: &Address, who: &Address) -> bool {
        ComplianceClient::new(env, compliance).is_allowed(who)
    }

    fn check_amount(env: &Env, amount: i128) {
        if amount <= 0 {
            panic_err(env, Error::InvalidAmount);
        }
    }

    fn set_balance(env: &Env, id: &Address, amount: i128) {
        let key = DataKey::Balance(id.clone());
        env.storage().persistent().set(&key, &amount);
        env.storage().persistent().extend_ttl(
            &key,
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
    }

    fn bump(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }
}

fn panic_err(env: &Env, error: Error) -> ! {
    soroban_sdk::panic_with_error!(env, error)
}

/// Reject strings that are empty or exceed `max_len` bytes (issue #46).
fn check_str(env: &Env, s: &String, min_len: u32, max_len: u32) {
    let len = s.len();
    if len < min_len || len > max_len {
        panic_err(env, Error::InvalidInput);
    }
}

#[cfg(test)]
mod test;
