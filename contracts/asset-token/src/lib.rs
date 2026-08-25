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
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Metadata,
    Balance(Address),
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
}

/// Maximum byte lengths for string metadata fields (issue #46).
const MAX_NAME_LEN: u32 = 64;
const MAX_SYMBOL_LEN: u32 = 16;
const MAX_ASSET_TYPE_LEN: u32 = 32;
const MAX_DESC_LEN: u32 = 256;

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Contract ABI/behavior version. Bump on any change to storage layout or
/// externally observable behavior so clients and the indexer can detect it.
pub const VERSION: u32 = 1;

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
        env.storage()
            .persistent()
            .set(&DataKey::Balance(admin.clone()), &total_supply);
        Self::bump(&env);
        env.events().publish(
            (symbol_short!("genesis"), admin.clone()),
            (total_supply, total_supply),
        );
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
            env.events()
                .publish((symbol_short!("transfer"), from, to), amount);
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
            Self::set_balance(&env, &to, to_bal + amount);
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
        env.storage()
            .persistent()
            .set(&DataKey::Balance(id.clone()), &amount);
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
