# Error Codes

Each contract defines a `#[contracterror] #[repr(u32)]` enum. A client only
ever sees the number (e.g. `Error(Contract, #7)`) — this maps every number
back to its name, exactly when it's raised, and what a caller should do
about it. Verified against each contract's actual `panic_err`/
`panic_with_error!` call sites, not just the enum's doc comments.

## asset-token (`contracts/asset-token/src/lib.rs`)

| # | Name | Raised when | Caller should |
|---|------|-------------|----------------|
| 1 | `AlreadyInitialized` | `initialize` called a second time | Not call `initialize` again — check `get_metadata` first if unsure |
| 2 | `NotInitialized` | Any call (`transfer`, `mint`, `balance`, `get_metadata`, …) made before `initialize`, since every entry point loads metadata through the same internal `metadata()` helper | Call `initialize` first |
| 3 | `Unauthorized` | Caller of an admin-only function isn't the stored `admin` | Use the correct admin address, or call `set_compliance`/re-issue as the real admin |
| 4 | `InsufficientBalance` | `transfer`/`burn` amount exceeds the caller's balance | Check `balance()` before calling |
| 5 | `InvalidAmount` | `amount <= 0` on `transfer`/`mint`/`burn`/`mint_batch`, or `total_supply < 0` / `valuation < 0` on `initialize`/`update_valuation` | Pass a strictly positive amount / non-negative supply-valuation |
| 6 | `Paused` | `transfer`/`mint`/`mint_batch`/`burn` called while the token is paused | Wait for `unpause`, or contact the admin |
| 7 | `SenderNotCompliant` | `transfer`'s or `burn`'s `from`/caller fails the compliance gate | Get the sender KYC-approved via the compliance contract |
| 8 | `RecipientNotCompliant` | `transfer`'s/`mint`'s/`mint_batch`'s recipient (or `initialize`'s admin) fails the compliance gate | Get the recipient KYC-approved first |
| 9 | `Overflow` | `total_supply` would overflow `i128` on mint, or underflow on burn | Not directly recoverable by a caller — this indicates a supply near `i128::MAX`; treat as a protocol-level issue |
| 10 | `InvalidInput` | A string metadata field (`name`, `symbol`, `asset_type`, `asset_description`) on `initialize` is empty or exceeds its max length | Shorten/fill in the field; see the contract source for the exact per-field length limits |
| 11 | `InvalidCompliance` | `set_compliance`'s new `compliance` address doesn't approve the current admin under `is_allowed` | Point at a compliance contract that already approves the admin, or approve the admin there first |

## compliance (`contracts/compliance/src/lib.rs`)

| # | Name | Raised when | Caller should |
|---|------|-------------|----------------|
| 1 | `AlreadyInitialized` | `initialize` called a second time | Don't re-initialize |
| 2 | `NotInitialized` | `get_admin` (or any admin-gated call) used before `initialize` | Call `initialize` first |
| 3 | `RecordNotFound` | `suspend`/`remove` targets an address with no KYC record | Approve the address first via `add_to_allowlist`, or double-check the address |
| 4 | `InvalidExpiry` | `add_to_allowlist`'s `expires_at` is non-zero and already in the past | Pass `0` (never expires) or a future ledger sequence |
| 5 | `Unauthorized` | Caller of an admin-only function isn't the stored admin | Use the correct admin address |
| 6 | `InvalidJurisdiction` | `jurisdiction` isn't exactly 2 ASCII letters after trimming whitespace — see [docs/compliance.md](compliance.md#jurisdiction-code-format) | Pass a well-formed 2-letter code |

## dividend (`contracts/dividend/src/lib.rs`)

| # | Name | Raised when | Caller should |
|---|------|-------------|----------------|
| 1 | `AlreadyInitialized` | `initialize` called a second time | Don't re-initialize |
| 2 | `NotInitialized` | Admin-gated call used before `initialize` | Call `initialize` first |
| 3 | `Unauthorized` | `create_distribution`'s caller isn't the stored admin | Use the correct admin address |
| 4 | `DistributionNotFound` | `get_distribution`/`claim` references an unknown `distribution_id` | Check the id against `get_distributions_for_asset` first |
| 5 | `InvalidAmount` | `create_distribution`'s `total_amount <= 0` | Pass a positive amount |
| 6 | `NothingToClaim` | `claim`'s computed claimable share is `0` (already claimed, zero snapshot balance, or zero supply) | Call `claimable` first to check before `claim`ing |
| 7 | `AlreadyClaimed` | `claim` called twice for the same `(distribution_id, holder)` | Each holder can only claim a given distribution once — this is expected, not a bug to retry around |
| 8 | `ZeroSupply` | `create_distribution`'s `asset_token` reports `total_supply() <= 0` at creation time | Fund the asset token with a positive supply before creating a distribution against it |
| 9 | `OverDistributed` | Internal guard: cumulative `distributed` would exceed `total_amount` | Not directly triggerable by a well-formed caller under normal use; indicates a bug if seen |

## registry (`contracts/registry/src/lib.rs`)

| # | Name | Raised when | Caller should |
|---|------|-------------|----------------|
| 1 | `AlreadyInitialized` | `initialize` called a second time | Don't re-initialize |
| 2 | `NotInitialized` | Admin-gated call used before `initialize` | Call `initialize` first |
| 3 | `Unauthorized` | `deactivate_asset`'s caller isn't the stored admin | Use the correct admin address |
| 4 | `AssetNotFound` | `get_asset`/`deactivate_asset` references an unknown `asset_id` | Check the id via `get_all_assets`/`get_assets_by_issuer` first |
| 5 | `InvalidValuation` | `register_asset`'s `valuation < 0` | Pass a non-negative valuation (USD cents) |
| 6 | `Overflow` | `total_value_locked`'s running sum would overflow `i128` | Not directly recoverable by a caller; indicates valuations summing near `i128::MAX` |
| 7 | `InvalidInput` | `register_asset`'s `name` is empty, or `asset_type` isn't one of the recognised values (see [docs/registry.md](registry.md)) | Pass a non-empty name and a valid `asset_type` |
