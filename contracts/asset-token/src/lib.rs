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
    Env, String,
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
    /// Maximum total supply. `-1` means no cap.
    pub max_supply: i128,
}

/// SEP-41 allowance record for a spender authorized by an owner.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allowance {
    pub amount: i128,
    pub expires_at: u32,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Metadata,
    Balance(Address),
    Allowance(Address, Address),
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
    MaxSupplyExceeded = 10,
}

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

#[contract]
pub struct AssetTokenContract;

#[contractimpl]
impl AssetTokenContract {
    /// Initialize the token and mint the full `total_supply` to the admin.
    /// The admin must already be compliance-approved to hold the asset.
    /// `max_supply` sets the maximum total supply; use `-1` for unlimited.
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
        max_supply: i128,
    ) {
        if env.storage().instance().has(&DataKey::Metadata) {
            panic_err(&env, Error::AlreadyInitialized);
        }
        admin.require_auth();
        if total_supply < 0 || valuation < 0 {
            panic_err(&env, Error::InvalidAmount);
        }
        if max_supply >= 0 && total_supply > max_supply {
            panic_err(&env, Error::InvalidAmount);
        }
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
            max_supply,
        };
        env.storage().instance().set(&DataKey::Metadata, &metadata);
        env.storage()
            .persistent()
            .set(&DataKey::Balance(admin.clone()), &total_supply);
        Self::bump(&env);
        env.events()
            .publish((symbol_short!("mint"), admin), total_supply);
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
        let to_bal = Self::balance(env.clone(), to.clone());
        Self::set_balance(&env, &from, from_bal - amount);
        Self::set_balance(&env, &to, to_bal + amount);
        Self::bump(&env);
        env.events()
            .publish((symbol_short!("transfer"), from, to), amount);
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
        if meta.max_supply >= 0 {
            let new_supply = meta
                .total_supply
                .checked_add(amount)
                .unwrap_or_else(|| panic_err(&env, Error::Overflow));
            if new_supply > meta.max_supply {
                panic_err(&env, Error::MaxSupplyExceeded);
            }
            let to_bal = Self::balance(env.clone(), to.clone());
            Self::set_balance(&env, &to, to_bal + amount);
            meta.total_supply = new_supply;
        } else {
            let new_supply = meta
                .total_supply
                .checked_add(amount)
                .unwrap_or_else(|| panic_err(&env, Error::Overflow));
            let to_bal = Self::balance(env.clone(), to.clone());
            Self::set_balance(&env, &to, to_bal + amount);
            meta.total_supply = new_supply;
        }
        env.storage().instance().set(&DataKey::Metadata, &meta);
        Self::bump(&env);
        env.events().publish((symbol_short!("mint"), to), amount);
    }

    /// Burn `amount` of the caller's own tokens.
    pub fn burn(env: Env, from: Address, amount: i128) {
        from.require_auth();
        Self::check_amount(&env, amount);
        let mut meta = Self::metadata(&env);
        let from_bal = Self::balance(env.clone(), from.clone());
        if from_bal < amount {
            panic_err(&env, Error::InsufficientBalance);
        }
        Self::set_balance(&env, &from, from_bal - amount);
        meta.total_supply -= amount;
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

    /// Approve `spender` to spend `amount` tokens on behalf of `owner`.
    /// Owner must authorize the call.
    pub fn approve(env: Env, owner: Address, spender: Address, amount: i128, expires_at: u32) {
        owner.require_auth();
        if amount < 0 {
            panic_err(&env, Error::InvalidAmount);
        }
        let now = env.ledger().sequence();
        if expires_at != 0 && expires_at <= now {
            panic_err(&env, Error::InvalidAmount);
        }
        let allowance = Allowance { amount, expires_at };
        env.storage()
            .persistent()
            .set(&DataKey::Allowance(owner.clone(), spender.clone()), &allowance);
        Self::bump(&env);
        env.events()
            .publish((symbol_short!("approve"), owner, spender), amount);
    }

    /// Returns the remaining allowance that `spender` is allowed to spend
    /// on behalf of `owner`.
    pub fn allowance(env: Env, owner: Address, spender: Address) -> i128 {
        let allowance: Option<Allowance> = env
            .storage()
            .persistent()
            .get(&DataKey::Allowance(owner, spender));
        match allowance {
            Some(a) => {
                if a.expires_at != 0 && env.ledger().sequence() >= a.expires_at {
                    0
                } else {
                    a.amount
                }
            }
            None => 0,
        }
    }

    /// Transfer `amount` from `from` to `to` using an allowance.
    /// The spender must authorize the call and must have a valid allowance.
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
        let from_bal = Self::balance(env.clone(), from.clone());
        if from_bal < amount {
            panic_err(&env, Error::InsufficientBalance);
        }
        let current_allowance = Self::allowance(env.clone(), from.clone(), spender.clone());
        if current_allowance < amount {
            panic_err(&env, Error::InsufficientBalance);
        }
        Self::set_balance(&env, &from, from_bal - amount);
        let to_bal = Self::balance(env.clone(), to.clone());
        Self::set_balance(&env, &to, to_bal + amount);
        let new_allowance = current_allowance - amount;
        if new_allowance == 0 {
            env.storage()
                .persistent()
                .remove(&DataKey::Allowance(from.clone(), spender.clone()));
        } else {
            let existing: Option<Allowance> = env
                .storage()
                .persistent()
                .get(&DataKey::Allowance(from.clone(), spender.clone()));
            let expires_at = existing.map(|a| a.expires_at).unwrap_or(0);
            env.storage()
                .persistent()
                .set(&DataKey::Allowance(from.clone(), spender.clone()), &Allowance { amount: new_allowance, expires_at });
        }
        Self::bump(&env);
        env.events()
            .publish((symbol_short!("xfer_from"), spender, from, to), amount);
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
    pub fn set_compliance(env: Env, admin: Address, compliance: Address) {
        let mut meta = Self::require_admin(&env, &admin);
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

#[cfg(test)]
mod test;
