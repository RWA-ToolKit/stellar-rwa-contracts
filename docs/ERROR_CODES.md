# Error Codes Reference

All four contracts in this workspace report failures by panicking with a contract error. In Soroban this is done via `panic_err` (asset-token) or `panic_with_error` (compliance, dividend, registry), which abort the invocation and surface the numeric discriminant of the contract's `Error` enum to the caller. SDK consumers, wallets, and UIs can map the returned `u32` code back to a human-readable message using the tables below.

Each contract defines its own `Error` enum, so the same numeric code can mean different things in different contracts. Always interpret a code in the context of the contract that raised it.

For authorization-related failures (missing signer, insufficient role, etc.) see [AUTHORITY_MATRIX.md](./AUTHORITY_MATRIX.md), which documents the auth model and the errors it produces in more detail.

## Asset Token

Defined in the `Error` enum of the asset-token contract and raised via `panic_err`.

| Variant | Code | Meaning |
| --- | --- | --- |
| `AlreadyInitialized` | 1 | Contract has already been initialized and cannot be initialized again. |
| `NotInitialized` | 2 | Contract has not been initialized yet; call `initialize` first. |
| `Unauthorized` | 3 | Caller is not authorized to perform this operation. |
| `InsufficientBalance` | 4 | Source account does not hold enough tokens for the transfer or burn. |
| `InsufficientAllowance` | 5 | Spender's allowance is lower than the requested transfer amount. |
| `InvalidAmount` | 6 | Amount is zero, negative, or otherwise outside the allowed range. |
| `AccountFrozen` | 7 | The source or destination account is frozen and cannot move tokens. |
| `ContractPaused` | 8 | The contract is paused; token operations are temporarily disabled. |
| `InvalidRecipient` | 9 | Destination address is invalid or not permitted (e.g. self-transfer where disallowed). |
| `Overflow` | 10 | Arithmetic operation overflowed the token's numeric bounds. |
| `Underflow` | 11 | Arithmetic operation underflowed the token's numeric bounds. |
| `AllowanceExpired` | 12 | The allowance has passed its expiration ledger and is no longer valid. |
| `NotApproved` | 13 | No active approval exists for the requested spender/amount. |
| `InvalidDecimals` | 14 | Configured decimals value is outside the supported range. |
| `SupplyCapExceeded` | 15 | Minting would push total supply above the configured cap. |

## Compliance

Defined in the `Error` enum of the compliance contract and raised via `panic_with_error`.

| Variant | Code | Meaning |
| --- | --- | --- |
| `AlreadyInitialized` | 1 | Contract has already been initialized. |
| `NotInitialized` | 2 | Contract has not been initialized yet. |
| `Unauthorized` | 3 | Caller lacks the role required for this compliance operation. |
| `NotAllowed` | 4 | The transfer or address fails the active compliance rules. |
| `AddressBlocked` | 5 | The address is on the blocklist and cannot participate. |
| `AddressNotVerified` | 6 | The address has not completed required verification/KYC. |
| `RuleNotFound` | 7 | The referenced compliance rule does not exist. |
| `RuleAlreadyExists` | 8 | A rule with the same identifier is already registered. |
| `InvalidRule` | 9 | The supplied rule definition is malformed or out of range. |
| `CountryNotAllowed` | 10 | The address's jurisdiction is not permitted by policy. |
| `LimitExceeded` | 11 | The operation exceeds a configured transfer or holding limit. |
| `InvalidAddress` | 12 | The supplied address is invalid for compliance evaluation. |

## Dividend

Defined in the `Error` enum of the dividend contract and raised via `panic_with_error`.

| Variant | Code | Meaning |
| --- | --- | --- |
| `AlreadyInitialized` | 1 | Contract has already been initialized. |
| `NotInitialized` | 2 | Contract has not been initialized yet. |
| `Unauthorized` | 3 | Caller is not authorized to manage or claim dividends. |
| `DistributionNotFound` | 4 | The referenced dividend distribution does not exist. |
| `DistributionAlreadyExists` | 5 | A distribution with the same identifier already exists. |
| `AlreadyClaimed` | 6 | The holder has already claimed their dividend for this distribution. |
| `NothingToClaim` | 7 | The holder has no claimable dividend for this distribution. |
| `InvalidAmount` | 8 | Deposit or distribution amount is zero or out of range. |
| `InsufficientFunds` | 9 | The distribution pool lacks funds to cover the claim or payout. |
| `DistributionClosed` | 10 | The distribution is closed and no longer accepts claims. |
| `InvalidPeriod` | 11 | The claim or accrual period is invalid or has not started. |
| `SnapshotNotFound` | 12 | The holder snapshot required to compute entitlements is missing. |

## Registry

Defined in the `Error` enum of the registry contract and raised via `panic_with_error`.

| Variant | Code | Meaning |
| --- | --- | --- |
| `AlreadyInitialized` | 1 | Contract has already been initialized. |
| `NotInitialized` | 2 | Contract has not been initialized yet. |
| `Unauthorized` | 3 | Caller is not authorized to modify registry entries. |
| `NotFound` | 4 | The requested registry entry does not exist. |
| `AlreadyExists` | 5 | An entry with the same key is already registered. |
| `InvalidEntry` | 6 | The supplied registry entry is malformed or fails validation. |
| `InvalidAddress` | 7 | The supplied address is invalid for registration. |
| `EntryInactive` | 8 | The registry entry exists but is marked inactive. |
| `DuplicateKey` | 9 | The key collides with an existing entry under a different namespace. |
| `LimitExceeded` | 10 | The registry has reached its maximum number of entries. |