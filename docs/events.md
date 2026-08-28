# Event Schema

Every state-changing call publishes a Soroban event as `(topics...) -> data`.
This is the exact topic tuple and payload shape for every event emitted by
every contract, verified against each contract's actual `env.events().publish(...)`
call sites (not just their doc comments) — where a contract's docs page
already listed an event, this repeats it here for indexer convenience; where
this page adds one the per-contract docs didn't mention, it's noted.

## asset-token

| Event (2nd topic = `symbol_short!`) | Topics | Data | When |
|---|---|---|---|
| `genesis` | `(genesis, admin: Address)` | `(total_supply: i128, total_supply: i128)` | `initialize` — the initial mint. Data is `(amount, amount)`, matching `mint`'s single-amount shape doubled; not `(0, total_supply)`. |
| `mint` | `(mint, to: Address)` | `amount: i128` | `mint`, and once per recipient inside `mint_batch` |
| `transfer` | `(transfer, from: Address, to: Address)` | `(amount: i128, new_from_balance: i128, new_to_balance: i128)` | `transfer` |
| `burn` | `(burn, from: Address)` | `amount: i128` | `burn` |
| `pause` | `(pause,)` | `admin: Address` | `pause` |
| `unpause` | `(unpause,)` | `admin: Address` | `unpause` |
| `valuation` | `(valuation,)` | `new_valuation: i128` | `update_valuation` |
| `setcomp` | `(setcomp,)` | `compliance: Address` | `set_compliance` |

**The inconsistent shape flagged by this issue:** `transfer` is the only
event on the token with a **3-element topic tuple** (`transfer, from, to`)
*and* a 3-element data tuple (`amount, new_from_balance, new_to_balance`).
Every other event here has a 1- or 2-element topic tuple and a single scalar
(or, for `genesis`, a 2-tuple of the same value) as data. An indexer that
assumes "data is always one value" or "topics are always `(name, subject)`"
will need a special case for `transfer`.

## compliance

| Event | Topics | Data | When |
|---|---|---|---|
| `init` | `(init,)` | `admin: Address` | `initialize` |
| `approved` | `(approved, address: Address)` | `(jurisdiction: String, expires_at: u32, prev_jurisdiction: String, prev_expires_at: u32, was_suspended: bool)` | `add_to_allowlist` — a 5-tuple carrying both the new state and the prior state, so an indexer can distinguish a fresh approval from a reinstatement/re-classification without a separate read |
| `suspend` | `(suspend, address: Address)` | `()` | `suspend` |
| `removed` | `(removed, address: Address)` | `()` | `remove` |
| `blockjur` | `(blockjur,)` | `jurisdiction: String` | `block_jurisdiction` |
| `unblkjur` | `(unblkjur,)` | `jurisdiction: String` | `unblock_jurisdiction` |
| `expired` *(not listed in this issue, but real — see below)* | `(expired, address: Address)` | `expires_at: u32` | Emitted from **two** places: (1) inside `is_allowed` the first time an expired record is detected on a read, and (2) inside `prune_expired`, once per record it removes |

`suspend` and `removed` both publish an empty data payload `()` — the address
is only in the topic tuple, unlike `approved`/`blockjur`/`unblkjur` which
carry a value in the data slot.

## dividend

| Event | Topics | Data | When |
|---|---|---|---|
| `init` | `(init,)` | `admin: Address` | `initialize` |
| `created` | `(created, admin: Address)` | `(distribution_id: u64, total_amount: i128)` | `create_distribution` |
| `claim` | `(claim, holder: Address)` | `(distribution_id: u64, amount: i128)` | `claim` |

## registry

| Event | Topics | Data | When |
|---|---|---|---|
| `init` | `(init,)` | `admin: Address` | `initialize` |
| `register` | `(register, issuer: Address)` | `asset_id: u64` | `register_asset` |
| `deactvate` | `(deactvate,)` | `asset_id: u64` | `deactivate_asset` — note the address is *not* in the topic tuple here, unlike `register` |

## Reading these tables

- `symbol_short!("name")` topics are Soroban `Symbol`s, max 9 characters —
  that's why some names are abbreviated (`deactvate`, `unblkjur`, `blockjur`).
- A topic tuple of `(name,)` (trailing comma, one element) has no subject —
  the event isn't scoped to a particular address/id at the protocol level,
  though the data payload usually still identifies one.
- "Data" is whatever `soroban_sdk::Env::events().publish(topics, data)`'s
  second argument was; a bare scalar (not a 1-tuple) when there's exactly one
  value, per each call site above.
