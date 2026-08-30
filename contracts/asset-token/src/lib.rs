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
//! the token's own `decimals` base.

use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, symbol_short, Address,
    BytesN, Env, String, Vec,
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
}

/// A SEP-41 style allowance: `spender` may move up to `amount` of `from`'s
/// tokens, and the allowance is treated as zero once `expiration_ledger` has
/// passed (issue #260, #261).
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllowanceValue {
    pub amount: i128,
    pub expiration_ledger: u32,
}

/// An upgrade awaiting its timelock before it can be applied (issue #259).
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingUpgrade {
    pub wasm_hash: BytesN<32>,
    pub ready_at: u32,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Metadata,
    Balance(Address),
    Allowance(Address, Address),
    PendingUpgrade,
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
    /// `spender` tried to move more than `from` has approved (issue #260).
    InsufficientAllowance = 12,
    /// `expiration_ledger` is in the past for a non-zero approval (issue #260).
    InvalidExpirationLedger = 13,
    /// No upgrade is currently pending (issue #259).
    NoPendingUpgrade = 14,
    /// A pending upgrade's timelock has not yet elapsed (issue #259).
    UpgradeNotReady = 15,
}

/// Maximum byte lengths for string metadata fields (issue #46).
const MAX_NAME_LEN: u32 = 64;
const MAX_SYMBOL_LEN: u32 = 16;
const MAX_ASSET_TYPE_LEN: u32 = 32;
const MAX_DESC_LEN: u32 = 256;

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Minimum delay between proposing and applying a contract upgrade, giving
/// holders and integrators time to react to an announced change (issue #259).
const UPGRADE_TIMELOCK_LEDGERS: u32 = 3 * DAY_IN_LEDGERS;

/// Contract ABI/behavior version. Bump on any change to storage layout or
/// externally observable behavior so clients and the indexer can detect it.
pub const VERSION: u32 = 2;

#[contract]
pub struct AssetTokenContract;

#[contractimpl]
impl AssetTokenContract {
    /// Current contract version.
    pub fn version(_env: Env) -> u32 {
        VERSION
    }

    /// Initialize the token and mint the full `total_supply` to the admin.
    /// The admin must already be compliance-approved to hold the asset.
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

    /// Transfer `amount` from `from` to `to`. Both parties must be
    /// compliance-approved and the token must not be paused.
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

    /// Mint new tokens to a compliance-approved recipient. Admin only.
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
    /// Admin only. Each `(recipient, amount)` pair is checked individually;
    /// if any recipient fails compliance the entire call reverts.
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

    /// Burn `amount` of the caller's own tokens.
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

    /// Current balance of `id`.
    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(id))
            .unwrap_or(0)
    }

    /// Current total supply.
    pub fn total_supply(env: Env) -> i128 {
        Self::metadata(&env).total_supply
    }

    /// Pause all transfers and mints. Admin only.
    pub fn pause(env: Env, admin: Address) {
        let mut meta = Self::require_admin(&env, &admin);
        meta.paused = true;
        env.storage().instance().set(&DataKey::Metadata, &meta);
        Self::bump(&env);
        env.events().publish((symbol_short!("pause"),), admin);
    }

    /// Resume transfers and mints. Admin only.
    pub fn unpause(env: Env, admin: Address) {
        let mut meta = Self::require_admin(&env, &admin);
        meta.paused = false;
        env.storage().instance().set(&DataKey::Metadata, &meta);
        Self::bump(&env);
        env.events().publish((symbol_short!("unpause"),), admin);
    }

    /// Full asset metadata.
    pub fn get_metadata(env: Env) -> AssetMetadata {
        Self::metadata(&env)
    }

    /// Update the recorded USD-cents valuation. Admin only.
    pub fn update_valuation(env: Env, admin: Address, new_valuation: i128) {
        let mut meta = Self::require_admin(&env, &admin);
        if new_valuation < 0 {
            panic_err(&env, Error::InvalidAmount);
        }
        meta.valuation = new_valuation;
        env.storage().instance().set(&DataKey::Metadata, &meta);
        Self::bump(&env);
        env.events()
            .publish((symbol_short!("valuation"),), new_valuation);
    }

    /// Point the token at a different compliance contract. Admin only.
    ///
    /// This does not re-validate existing holders against the new gate: a
    /// holder approved under the old contract keeps their balance even if
    /// the new contract would reject them. It only checks that `compliance`
    /// implements `is_allowed` and approves the admin, to catch a
    /// misconfigured address before it bricks every transfer.
    pub fn set_compliance(env: Env, admin: Address, compliance: Address) {
        let mut meta = Self::require_admin(&env, &admin);
        if !Self::compliant(&env, &compliance, &admin) {
            panic_err(&env, Error::InvalidCompliance);
        }
        meta.compliance_contract = compliance.clone();
        env.storage().instance().set(&DataKey::Metadata, &meta);
        Self::bump(&env);
        env.events()
            .publish((symbol_short!("setcomp"),), compliance);
    }

    // ---- SEP-41 metadata surface (issue #261) ----

    /// Token name (SEP-41).
    pub fn name(env: Env) -> String {
        Self::metadata(&env).name
    }

    /// Token symbol (SEP-41).
    pub fn symbol(env: Env) -> String {
        Self::metadata(&env).symbol
    }

    /// Token decimals (SEP-41).
    pub fn decimals(env: Env) -> u32 {
        Self::metadata(&env).decimals
    }

    // ---- allowance surface (issue #260, #261) ----

    /// Remaining amount `spender` may move from `from`, 0 if none or expired.
    pub fn allowance(env: Env, from: Address, spender: Address) -> i128 {
        Self::read_allowance(&env, &from, &spender).amount
    }

    /// Set how much `spender` may move from `from`'s balance, until
    /// `expiration_ledger`. Pass `amount = 0` to revoke. Mirrors SEP-41.
    pub fn approve(env: Env, from: Address, spender: Address, amount: i128, expiration_ledger: u32) {
        from.require_auth();
        if amount < 0 {
            panic_err(&env, Error::InvalidAmount);
        }
        if Self::metadata(&env).paused {
            panic_err(&env, Error::Paused);
        }
        Self::write_allowance(&env, &from, &spender, amount, expiration_ledger);
        Self::bump(&env);
        env.events().publish(
            (symbol_short!("approve"), from, spender),
            (amount, expiration_ledger),
        );
    }

    /// Transfer `amount` from `from` to `to` using `spender`'s allowance.
    /// Both `from` and `to` must be compliance-approved, matching `transfer`.
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
        Self::spend_allowance(&env, &from, &spender, amount);
        let from_bal = Self::balance(env.clone(), from.clone());
        if from_bal < amount {
            panic_err(&env, Error::InsufficientBalance);
        }
        if from == to {
            Self::bump(&env);
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
        env.events().publish(
            (symbol_short!("transfer"), from, to),
            (amount, new_from_bal, new_to_bal),
        );
    }

    /// Burn `amount` of `from`'s tokens using `spender`'s allowance.
    pub fn burn_from(env: Env, spender: Address, from: Address, amount: i128) {
        spender.require_auth();
        Self::check_amount(&env, amount);
        let mut meta = Self::metadata(&env);
        if meta.paused {
            panic_err(&env, Error::Paused);
        }
        if !Self::compliant(&env, &meta.compliance_contract, &from) {
            panic_err(&env, Error::SenderNotCompliant);
        }
        Self::spend_allowance(&env, &from, &spender, amount);
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

    // ---- upgrade (issue #259) ----

    /// Propose upgrading this contract to `new_wasm_hash`. Admin only. Cannot
    /// be applied until `UPGRADE_TIMELOCK_LEDGERS` have elapsed.
    pub fn propose_upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) {
        Self::require_admin(&env, &admin);
        let ready_at = env.ledger().sequence().saturating_add(UPGRADE_TIMELOCK_LEDGERS);
        let pending = PendingUpgrade {
            wasm_hash: new_wasm_hash.clone(),
            ready_at,
        };
        env.storage()
            .instance()
            .set(&DataKey::PendingUpgrade, &pending);
        Self::bump(&env);
        env.events()
            .publish((symbol_short!("upgprop"),), (new_wasm_hash, ready_at));
    }

    /// Cancel a pending upgrade. Admin only.
    pub fn cancel_upgrade(env: Env, admin: Address) {
        Self::require_admin(&env, &admin);
        if !env.storage().instance().has(&DataKey::PendingUpgrade) {
            panic_err(&env, Error::NoPendingUpgrade);
        }
        env.storage().instance().remove(&DataKey::PendingUpgrade);
        Self::bump(&env);
        env.events().publish((symbol_short!("upgcncl"),), ());
    }

    /// Apply a pending upgrade once its timelock has elapsed. Admin only.
    pub fn upgrade(env: Env, admin: Address) {
        Self::require_admin(&env, &admin);
        let pending: PendingUpgrade = env
            .storage()
            .instance()
            .get(&DataKey::PendingUpgrade)
            .unwrap_or_else(|| panic_err(&env, Error::NoPendingUpgrade));
        if env.ledger().sequence() < pending.ready_at {
            panic_err(&env, Error::UpgradeNotReady);
        }
        env.storage().instance().remove(&DataKey::PendingUpgrade);
        env.deployer()
            .update_current_contract_wasm(pending.wasm_hash.clone());
        Self::bump(&env);
        env.events()
            .publish((symbol_short!("upgraded"),), pending.wasm_hash);
    }

    /// The upgrade currently awaiting its timelock, if any.
    pub fn get_pending_upgrade(env: Env) -> Option<PendingUpgrade> {
        env.storage().instance().get(&DataKey::PendingUpgrade)
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

    /// Current allowance record, treated as zeroed out once expired.
    fn read_allowance(env: &Env, from: &Address, spender: &Address) -> AllowanceValue {
        let key = DataKey::Allowance(from.clone(), spender.clone());
        match env.storage().persistent().get::<_, AllowanceValue>(&key) {
            Some(a) if a.amount > 0 && a.expiration_ledger < env.ledger().sequence() => {
                AllowanceValue {
                    amount: 0,
                    expiration_ledger: a.expiration_ledger,
                }
            }
            Some(a) => a,
            None => AllowanceValue {
                amount: 0,
                expiration_ledger: 0,
            },
        }
    }

    fn write_allowance(
        env: &Env,
        from: &Address,
        spender: &Address,
        amount: i128,
        expiration_ledger: u32,
    ) {
        if amount > 0 && expiration_ledger < env.ledger().sequence() {
            panic_err(env, Error::InvalidExpirationLedger);
        }
        let key = DataKey::Allowance(from.clone(), spender.clone());
        let value = AllowanceValue {
            amount,
            expiration_ledger,
        };
        env.storage().persistent().set(&key, &value);
        env.storage().persistent().extend_ttl(
            &key,
            INSTANCE_LIFETIME_THRESHOLD,
            INSTANCE_BUMP_AMOUNT,
        );
    }

    fn spend_allowance(env: &Env, from: &Address, spender: &Address, amount: i128) {
        let allowance = Self::read_allowance(env, from, spender);
        if allowance.amount < amount {
            panic_err(env, Error::InsufficientAllowance);
        }
        Self::write_allowance(
            env,
            from,
            spender,
            allowance.amount - amount,
            allowance.expiration_ledger,
        );
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
