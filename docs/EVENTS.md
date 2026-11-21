# Soroban event schema

This document is the indexer-friendly reference for **every event currently
published** by the four contracts. It is derived from the
`env.events().publish(topics, data)` call sites in `contracts/*/src/lib.rs`
(not from tests). There is no separate events-query helper contract: use the
Stellar RPC `getEvents` endpoint, filter by contract id, and match the first
topic (the event name symbol) from the tables below.

## Conventions

- `topics` are a Soroban `Vec<Val>`; `data` is one `Val`, often a tuple.
- `name!` in the tables means `symbol_short!("name")` — the 9-character
  Soroban short-symbol spelling is intentional and part of the ABI.
- `Address`, `i128`, `u64`, `u32`, `String`, and `Option<Address>` are encoded
  with the standard Soroban XDR/Val encodings. Tuples preserve their order.
- A batch emits one event per item, in input order.
- A reverted call emits **no** events: errors use `panic_with_error!`, and the
  host discards all events from a failed invocation.
- Read-only functions emit no events.

## asset-token (`contracts/asset-token/src/lib.rs`)

| Function | Topics | Data |
| --- | --- | --- |
| `initialize` | `(genesis!, admin)` | `total_supply: i128` |
| `initialize` | `(mint!, admin)` | `total_supply: i128` |
| `transfer` | `(transfer!, from, to)` | `(amount, new_from_bal, new_to_bal): (i128, i128, i128)` |
| `transfer_batch` | `(transfer!, from, to)` per entry | `(amount, new_from_bal, new_to_bal)` per entry |
| `mint` | `(mint!, to)` | `amount: i128` |
| `mint_batch` | `(mint!, to)` per entry | `amount: i128` per entry |
| `burn` | `(burn!, from)` | `amount: i128` |
| `approve` | `(approve!, from, spender)` | `(amount, expiration_ledger): (i128, u64)` |
| `transfer_from` | `(transfer!, from, to)` | `(amount, new_from_bal, new_to_bal)` |
| `pause` | `(pause!,)` | `caller: Address` |
| `unpause` | `(unpause!,)` | `admin: Address` |
| `set_guardian` | `(guardian!,)` | `guardian: Option<Address>` |
| `update_valuation` | `(valuation!,)` | `(old_valuation, new_valuation): (i128, i128)` |
| `set_compliance` | `(setcomp!,)` | `(old_compliance, new_compliance): (Address, Address)` |
| `propose_admin` | `(proposed!, admin)` | `new_admin: Address` |
| `cancel_admin_proposal` | `(cancelled!, admin)` | `()` |
| `accept_admin` | `(set_admin!, old_admin)` | `new_admin: Address` |

Notes:

- `initialize` emits both `genesis` and `mint` for the initial allocation so
  indexers that sum `mint` events see the correct initial supply.
- Self-transfers (including batch entries) emit the same `transfer` shape with
  `new_from_bal == new_to_bal == from_bal`.
- `approve` emits an event even when `amount == 0` (allowance clearing).

## compliance (`contracts/compliance/src/lib.rs`)

| Function | Topics | Data |
| --- | --- | --- |
| `initialize` | `(init!,)` | `admin: Address` |
| `add_to_allowlist` | `(approved!, address)` | `(jurisdiction, expires_at, prev_jurisdiction, prev_expires_at, was_suspended): (String, u32, String, u32, bool)` |
| `add_to_allowlist_batch` | `(approved!, address)` per entry | same 5-tuple as `add_to_allowlist` |
| `suspend` | `(suspend!, address)` | `()` |
| `reinstate` | `(reinstat!, address)` | `()` |
| `remove` | `(removed!, address)` | `()` |
| `is_allowed` | `(expired!, address)` | `expires_at: u64` |
| `is_allowed` | `(hldfail!, address)` | `(first_acquired, min_ledgers, now): (u64, u64, u64)` |
| `block_jurisdiction` | `(blockjur!,)` | `jurisdiction: String` |
| `unblock_jurisdiction` | `(unblkjur!,)` | `jurisdiction: String` |
| `prune_expired` | `(expired!, address)` | `expires_at: u64` |
| `propose_admin` | `(proposed!, admin)` | `new_admin: Address` |
| `cancel_admin_proposal` | `(cancelled!, admin)` | `()` |
| `accept_admin` | `(set_admin!, old_admin)` | `new_admin: Address` |
| `set_min_holding_period` | `(minhld!, holder)` | `min_ledgers: u64` |
| `record_acquisition` | `(acquired!, holder)` | `ledger: u64` |

Notes:

- `expired` is emitted both by `is_allowed` (when a stale record causes a
  denial) and by `prune_expired` (when the record is actually deleted). Treat
  it as a transition notification, not proof of deletion; use `prune_expired`
  to observe removal.
- `hldfail` is emitted when the minimum holding period is still active.
- `approved` carries the previous jurisdiction/expiry and a `was_suspended`
  flag so indexers can reconstruct re-approvals without a prior state read.
- An `expires_at` of `0` is the never-expires sentinel.

## dividend (`contracts/dividend/src/lib.rs`)

| Function | Topics | Data |
| --- | --- | --- |
| `initialize` | `(init!,)` | `admin: Address` |
| `create_distribution` / `create_distribution_deadline` | `(created!, admin)` | `(distribution_id, total_amount): (u64, i128)` |
| `claim` | `(claim!, holder)` | `(distribution_id, amount): (u64, i128)` |
| `reclaim_unclaimed` | `(reclaim!, admin)` | `(distribution_id, remaining): (u64, i128)` |
| `cancel_distribution` | `(cancel!, admin)` | `distribution_id: u64` |
| `propose_admin` | `(proposed!, admin)` | `new_admin: Address` |
| `cancel_admin_proposal` | `(cancelled!, admin)` | `()` |
| `accept_admin` | `(set_admin!, old_admin)` | `new_admin: Address` |

Notes:

- `create_distribution` is a wrapper around `create_distribution_deadline`, so
  both produce the same `created` event.
- `cancel_distribution` refunds escrow and emits `cancel`; it is only allowed
  while nothing has been claimed.

## registry (`contracts/registry/src/lib.rs`)

| Function | Topics | Data |
| --- | --- | --- |
| `initialize` | `(init!,)` | `admin: Address` |
| `register_asset` | `(register!, issuer)` | `asset_id: u64` |
| `deactivate_asset` | `(deactvate!,)` | `asset_id: u64` |
| `update_valuation` | `(valuation!, asset_id)` | `(old_valuation, new_valuation): (i128, i128)` |
| `update_valuations_batch` | `(valuation!, asset_id)` per entry | `(old_valuation, new_valuation)` per entry |
| `reactivate_asset` | `(reactvate!,)` | `asset_id: u64` |
| `propose_admin` | `(proposed!, admin)` | `new_admin: Address` |
| `cancel_admin_proposal` | `(cancelled!, admin)` | `()` |
| `accept_admin` | `(set_admin!, old_admin)` | `new_admin: Address` |

Notes:

- `reactivate_asset` on an already-active asset is a no-op and emits nothing.
- Registry `valuation` events include the asset id in **topics**; the
  asset-token `valuation` event has no id topic because the contract itself is
  the asset. Indexers must not treat these two schemas as interchangeable.
- TVL is not emitted directly; derive it from `valuation` events (or call
  `total_value_locked`) and remember that only active assets contribute.

## Cross-contract patterns

The same admin-handover event trio (`proposed`, `cancelled`, `set_admin`) is
used by all four contracts. To disambiguate, always include the emitting
contract id in your index key. Likewise, `init` is emitted once per contract
by `initialize`.

When adding or changing an event, update this document and the corresponding
event tests in the same PR, and note the change in `CHANGELOG.md` if contract
source changes.
