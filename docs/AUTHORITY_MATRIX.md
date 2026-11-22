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

### Explicitly open view functions (compliance)

The following compliance functions are confirmed to be intentionally open to
any caller — no auth gate, no privileged check:

| Function | Rationale |
|---|---|
| `get_allowlist_count` | Pure O(1) counter read from instance storage. No side-effects; exposing the count to any caller is consistent with all other `get_*` view functions on this contract. Confirmed by `test_get_allowlist_count_open_to_any_caller`. |

### Events emitted by compliance (for indexers)

| Function | Topics | Data | Notes |
|---|---|---|---|
| `initialize` | `("init",)` | `admin: Address` | Emitted once at deploy time. |
| `add_to_allowlist` / `add_to_allowlist_batch` | `("approved", address)` | `(jurisdiction, expires_at, prev_jurisdiction, prev_expires_at, was_suspended)` | Emitted per address; payload carries before/after state for audit. |
| `suspend` | `("suspend", address)` | `()` | |
| `reinstate` | `("reinstat", address)` | `()` | Emitted after the status flip to `Approved` is persisted. Confirmed by `test_reinstate_emits_event`. `reinstate` does **not** renew `expires_at`; an expired-but-suspended record will be flipped to `Approved` in storage yet still fail `is_allowed` — see `test_reinstate_while_record_expired_is_deliberate_not_panic`. |
| `remove` | `("removed", address)` | `()` | |
| `block_jurisdiction` | `("blockjur",)` | `jurisdiction: String` | Idempotent; event only on first block. |
| `unblock_jurisdiction` | `("unblkjur",)` | `jurisdiction: String` | |
| `is_allowed` (expiry path) | `("expired", address)` | `expires_at: u32` | Emitted lazily at check time when a record is found to be expired. |
| `propose_admin` | `("proposed", admin)` | `new_admin: Address` | |
| `cancel_admin_proposal` | `("cancelled", admin)` | `()` | |
| `accept_admin` | `("set_admin", old_admin)` | `new_admin: Address` | |
| `set_min_holding_period` | `("minhld", holder)` | `min_ledgers: u64` | |
| `record_acquisition` | `("acquired", holder)` | `ledger: u64` | Only emitted on first acquisition; idempotent. |
| `prune_expired` | `("expired", address)` | `expires_at: u32` | One event per pruned address. |

## compliance (`contracts/compliance`)

| Function | Required role | Failure mode when called by the wrong address |
|---|---|---|
| `initialize` | None (bootstrap) — but only succeeds once | Second call panics `Error::AlreadyInitialized` (#1) |
| `add_to_allowlist` | Admin | `Error::Unauthorized` (#5); `Error::NotInitialized` (#2) if uninitialized |
| `add_to_allowlist_batch` | Admin | `Error::Unauthorized` (#5); a single invalid entry aborts the whole batch with `Error::InvalidExpiry` (#4) or `Error::InvalidJurisdiction` (#6) |
| `suspend` | Admin | `Error::Unauthorized` (#5) |
| `reinstate` | Admin | `Error::Unauthorized` (#5) |
| `remove` | Admin | `Error::Unauthorized` (#5) |
| `block_jurisdiction` | Admin | `Error::Unauthorized` (#5) |
| `unblock_jurisdiction` | Admin | `Error::Unauthorized` (#5) |
| `prune_expired` | Admin | `Error::Unauthorized` (#5) |
| `set_min_holding_period` | Admin | `Error::Unauthorized` (#5) |
| `record_acquisition` | Admin | `Error::Unauthorized` (#5) |
| `propose_admin` | Admin | `Error::Unauthorized` (#5) |
| `cancel_admin_proposal` | Admin | `Error::Unauthorized` (#5); `Error::NoPendingAdmin` (#7) if no proposal |
| `accept_admin` | Pending admin (self) | `Error::Unauthorized` (#5) if not the proposed successor; `Error::NoPendingAdmin` (#7) if no proposal |

## asset-token (`contracts/asset-token`)

| Function | Required role | Failure mode when called by the wrong address |
|---|---|---|
| `initialize` | None (bootstrap) — but only succeeds once | Second call panics `Error::AlreadyInitialized` (#1) |
| `transfer` | Self (the `from` address) | `require_auth` failure (host-level auth error) if `from` did not authorize; separately reverts with `Error::SenderNotCompliant` (#7) / `Error::RecipientNotCompliant` (#8) / `Error::Paused` (#6) / `Error::InsufficientBalance` (#4) on business-rule failure |
| `transfer_batch` | Self (the `from` address) | `require_auth` failure if `from` did not authorize; separately reverts with `Error::SenderNotCompliant` (#7) / `Error::RecipientNotCompliant` (#8) / `Error::Paused` (#6) / `Error::InvalidAmount` / `Error::InsufficientBalance` (#4) on business-rule failure |
| `mint` | Admin | `Error::Unauthorized` (#3) |
| `snapshot` | Admin | `Error::Unauthorized` (#3) |
| `mint_batch` | Admin | `Error::Unauthorized` (#3) |
| `burn` | Self (the `from` address) | `require_auth` failure if `from` did not authorize |
| `pause` | Admin | `Error::Unauthorized` (#3) |
| `unpause` | Admin | `Error::Unauthorized` (#3) |
| `update_valuation` | Admin | `Error::Unauthorized` (#3) |
| `set_compliance` | Admin | `Error::Unauthorized` (#3) |
| `propose_admin` | Admin | `Error::Unauthorized` (#3) |
| `cancel_admin_proposal` | Admin | `Error::Unauthorized` (#3); `Error::NoPendingAdmin` (#12) if no proposal |
| `accept_admin` | Pending admin (self) | `Error::Unauthorized` (#3) if not the proposed successor; `Error::NoPendingAdmin` (#12) if no proposal |

## dividend (`contracts/dividend`)

| Function | Required role | Failure mode when called by the wrong address |
|---|---|---|
| `initialize` | None (bootstrap) — but only succeeds once | Second call panics `Error::AlreadyInitialized` (#1) |
| `create_distribution` | Admin | `Error::Unauthorized` (#3) |
| `create_distribution_snapshot` | Admin (or the asset token's admin, as `create_distribution`) | `Error::Unauthorized` (#3); the asset token's `SnapshotNotFound` (#16) for an unknown snapshot |
| `claim` | Self (the `holder` address) | `require_auth` failure if `holder` did not authorize; separately reverts with `Error::AlreadyClaimed` (#7) / `Error::NothingToClaim` (#6) on business-rule failure |
| `withdraw_unclaimed` | Admin | `Error::Unauthorized` (#3); `Error::DistributionTooYoung` (#16) before the requested minimum age; `Error::DeadlineNotReached` (#14) before a configured claim deadline; `Error::NothingToClaim` (#6) if no withdrawable balance remains |
| `propose_admin` | Admin | `Error::Unauthorized` (#3) |
| `cancel_admin_proposal` | Admin | `Error::Unauthorized` (#3); `Error::NoPendingAdmin` (#12) if no proposal |
| `accept_admin` | Pending admin (self) | `Error::Unauthorized` (#3) if not the proposed successor; `Error::NoPendingAdmin` (#12) if no proposal |

## registry (`contracts/registry`)

| Function | Required role | Failure mode when called by the wrong address |
|---|---|---|
| `initialize` | None (bootstrap) — but only succeeds once | Second call panics `Error::AlreadyInitialized` (#1) |
| `register_asset` | Self (the `issuer` address) | `require_auth` failure if `issuer` did not authorize |
| `deactivate_asset` | Admin | `Error::Unauthorized` (#3) |
| `propose_admin` | Admin | `Error::Unauthorized` (#3) |
| `cancel_admin_proposal` | Admin | `Error::Unauthorized` (#3); `Error::NoPendingAdmin` (#8) if no proposal |
| `accept_admin` | Pending admin (self) | `Error::Unauthorized` (#3) if not the proposed successor; `Error::NoPendingAdmin` (#8) if no proposal |

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
