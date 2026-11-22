use soroban_sdk::{contracttype, Address, String};

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
