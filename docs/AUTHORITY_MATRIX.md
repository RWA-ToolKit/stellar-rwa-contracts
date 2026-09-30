# Authority Matrix

Issue #345. Every contract in this workspace has its own admin address and
its own privileged methods — there is no shared access-control contract.
This document is the single place that lists every privileged (state
mutating, restricted-caller) function across all four contracts, the role
required to call it, and the error raised when the wrong address calls it.

All four contracts follow the same two patterns:

- **Admin-gated**: the caller passes an `admin: Address` argument, the
  contract loads the stored admin from instance storage, calls
  `admin.require_auth()` (so the Soroban host enforces that the *passed-in*
  address actually authorized the invocation — a caller cannot simply pass
  someone else's address), and then compares the passed-in address against
  the stored admin. A mismatch panics with `Error::Unauthorized`. Calling
  before `initialize` panics with `Error::NotInitialized`.
- **Self-authorized**: the caller passes their own address as an explicit
  argument (e.g. `from`, `holder`, `issuer`) and the method calls
  `<that address>.require_auth()`. Only that address (or one of its signers)
  can invoke it; there is no admin override.

Read-only view functions (`get_*`, `is_*`, `balance`, `total_supply`,
`version`, etc.) are omitted — they take no privileged action and have no
required role.

## compliance (`contracts/compliance`)

| Function | Required role | Failure mode when called by the wrong address |
|---|---|---|
| `initialize` | None (bootstrap) — but only succeeds once | Second call panics `Error::AlreadyInitialized` (#1) |
| `add_to_allowlist` | Admin | `Error::Unauthorized` (#5); `Error::NotInitialized` (#2) if uninitialized |
| `add_to_allowlist_batch` | Admin | `Error::Unauthorized` (#5); a single invalid entry aborts the whole batch with `Error::InvalidExpiry` (#4) or `Error::InvalidJurisdiction` (#6) |
| `suspend` | Admin | `Error::Unauthorized` (#5) |
| `remove` | Admin | `Error::Unauthorized` (#5) |
| `block_jurisdiction` | Admin | `Error::Unauthorized` (#5) |
| `unblock_jurisdiction` | Admin | `Error::Unauthorized` (#5) |
| `prune_expired` | Admin | `Error::Unauthorized` (#5) |

## asset-token (`contracts/asset-token`)

| Function | Required role | Failure mode when called by the wrong address |
|---|---|---|
| `initialize` | None (bootstrap) — but only succeeds once | Second call panics `Error::AlreadyInitialized` (#1) |
| `transfer` | Self (the `from` address) | `require_auth` failure (host-level auth error) if `from` did not authorize; separately reverts with `Error::SenderNotCompliant` (#7) / `Error::RecipientNotCompliant` (#8) / `Error::Paused` (#6) / `Error::InsufficientBalance` (#4) on business-rule failure |
| `mint` | Admin | `Error::Unauthorized` (#3) |
| `mint_batch` | Admin | `Error::Unauthorized` (#3) |
| `burn` | Self (the `from` address) | `require_auth` failure if `from` did not authorize |
| `pause` | Admin | `Error::Unauthorized` (#3) |
| `unpause` | Admin | `Error::Unauthorized` (#3) |
| `update_valuation` | Admin | `Error::Unauthorized` (#3) |
| `set_compliance` | Admin | `Error::Unauthorized` (#3) |

## dividend (`contracts/dividend`)

| Function | Required role | Failure mode when called by the wrong address |
|---|---|---|
| `initialize` | None (bootstrap) — but only succeeds once | Second call panics `Error::AlreadyInitialized` (#1) |
| `create_distribution` | Admin | `Error::Unauthorized` (#3) |
| `claim` | Self (the `holder` address) | `require_auth` failure if `holder` did not authorize; separately reverts with `Error::AlreadyClaimed` (#7) / `Error::NothingToClaim` (#6) on business-rule failure |
| `withdraw_unclaimed` | Admin | `Error::Unauthorized` (#3); `Error::DistributionTooYoung` (#16) before the requested minimum age; `Error::DeadlineNotReached` (#14) before a configured claim deadline; `Error::NothingToClaim` (#6) if no withdrawable balance remains |

## registry (`contracts/registry`)

| Function | Required role | Failure mode when called by the wrong address |
|---|---|---|
| `initialize` | None (bootstrap) — but only succeeds once | Second call panics `Error::AlreadyInitialized` (#1) |
| `register_asset` | Self (the `issuer` address) | `require_auth` failure if `issuer` did not authorize |
| `deactivate_asset` | Admin | `Error::Unauthorized` (#3) |

## Cross-contract trust

- `asset-token.transfer`/`mint` call into `compliance.is_allowed` for both
  sender and recipient. `compliance` does not authenticate the caller of
  `is_allowed` (it is a public view function) — the security boundary is
  that only `asset-token` is configured, via admin-only `set_compliance`, to
  gate transfers on its result.
- `dividend.create_distribution` reads token balances/supply from
  `asset-token` but does not require any special role on the `asset-token`
  side beyond public view access.

## Maintaining this document

Whenever a new `pub fn` is added to a contract in `contracts/*/src/lib.rs`
that takes an `admin: Address` argument, calls `require_auth()` on a
caller-supplied address, or otherwise gates behavior by identity, add a row
here in the same commit.
