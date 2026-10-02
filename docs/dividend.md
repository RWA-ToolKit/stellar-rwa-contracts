# Dividend Contract

Distributes yield/dividends to asset-token holders in proportion to their
holdings. An issuer funds a distribution with a payment token; each holder then
claims their share, paid from escrow held by this contract.

- Testnet: `CAR4XY3CEBQWFOL27JEWFW34KXSIZA7RFKDQMEIV7ZU723RWY37I2SYX`

## Proportional formula

At claim time a holder can claim:

```
claimable = total_amount * balance(holder) / total_supply
```

where `balance` and `total_supply` are represented in the asset token's raw
integer units. Integer division floors the result. With an asset token using
zero decimals, fractional holdings cannot be represented and a small claim can
round to zero, making it unclaimable. Each holder can claim a given
distribution **once**.

### Rounding & dust (issue #4)

Flooring means each holder's payout can fall short of their exact
proportional share by a fractional remainder. These losses accumulate rather
than cancel. For `N` eligible holders, the total dust left stranded in
escrow after everyone claims is bounded by `0 <= dust <= N - 1` units of
`payment_token` (derived from `sum(total_amount * balance_i / supply) =
total_amount` exactly, so the sum of the `N` per-holder fractional losses is
< `N`, and — being an integer — is at most `N - 1`). This is expected,
documented behaviour: rounding some holder up instead would let total claims
exceed `total_amount`. See `reclaim_unclaimed` and `withdraw_unclaimed` for
mechanisms that can recover stranded funds, and
`test_uneven_distribution_leaves_dust` in `test.rs` for a worked example.

## `Distribution`

| Field            | Type      | Meaning                              |
|------------------|-----------|--------------------------------------|
| `id`             | `u64`     | Distribution id (1-based)            |
| `asset_token`    | `Address` | Token whose holders are paid         |
| `payment_token`  | `Address` | Token used to pay (e.g. a SAC)       |
| `total_amount`   | `i128`    | Total escrowed for the distribution  |
| `distributed`    | `i128`    | Amount paid out, including claims and admin recovery |
| `snapshot_ledger`| `u32`     | Ledger at creation (reference)       |
| `created_at`     | `u32`     | Ledger at creation                   |
| `completed`      | `bool`    | True once `distributed >= total` **or** the distribution was cancelled |
| `cancelled`      | `bool`    | True if `cancel_distribution` was called (issue #428). Distinguishes a cancelled distribution from a fully-paid one: both set `completed = true`, but only a cancelled distribution also sets this flag. Always `false` for distributions that reach completion through normal claims or `reclaim_unclaimed`. |
| `deadline`       | `u32`     | Ledger after which claims stop and admin may reclaim; `0` = no deadline |

## Claim deadline & reclaim policy (issue #2)

**Policy, decided up front:**

1. `deadline == 0` (the default, via `create_distribution`) means there is no
   deadline: the distribution is claimable forever and can never be
   reclaimed.
2. `create_distribution_deadline(..., deadline)` sets a ledger sequence
   number after which `claim` is rejected with `DeadlinePassed (#13)`. Up to
   and including that ledger, claiming works normally.
3. After the deadline passes, and **only** the contract **admin** (not the
   issuer, not any holder) may call `reclaim_unclaimed(admin, distribution_id)`
   to sweep `total_amount - distributed` out of escrow to the admin. This is
   restricted to admin because the admin is the party that funded the escrow;
   allowing any other role to reclaim would let a non-funder walk off with
   holder-entitled funds.
4. Reclaiming marks the distribution `completed` (as if fully claimed) and
   clears its snapshot/supply storage. Reclaiming twice, reclaiming before
   the deadline (`DeadlineNotReached (#14)`), or reclaiming a distribution
   with no deadline set (`NoDeadline (#15)`) are all rejected.

## Minimum-age withdrawal (issue #453)

The contract admin may call
`withdraw_unclaimed(admin, distribution_id, recipient, min_age_ledgers)` on a
distribution once `current_ledger - created_at >= min_age_ledgers`. This path
does not require a claim deadline; the caller supplies the minimum age for
each withdrawal. It transfers only `total_amount - distributed`, so prior
holder payouts are untouched. The distribution is marked completed and its
claim snapshot is removed to prevent further claims. Calls before the minimum
age fail with `DistributionTooYoung (#16)`; calls with no remaining balance
fail with `NothingToClaim (#6)`. A `withdraw` event records the admin, recipient,
distribution id, and amount. If a claim deadline was configured, withdrawal
also waits until that deadline passes (`DeadlineNotReached (#14)` otherwise).

## Cross-contract interfaces

```rust
#[contractclient(name = "AssetClient")]
pub trait AssetInterface {
    fn balance(env: Env, id: Address) -> i128;
    fn total_supply(env: Env) -> i128;
}

#[contractclient(name = "TokenClient")]
pub trait TokenInterface {
    fn transfer(env: Env, from: Address, to: Address, amount: i128);
}
```

## Functions

- `initialize(admin)` — sets admin. Once only.
- `create_distribution(admin, asset_token, payment_token, total_amount, eligible) -> u64` —
  admin auth; validates `eligible`, then pulls `total_amount` of `payment_token`
  from the admin into the contract's escrow and freezes `eligible` as the
  entitlement snapshot. `InvalidAmount (#5)` if `total_amount <= 0` or an
  `eligible` entry is negative; `ZeroSupply (#8)` if the asset has no supply;
  `DuplicateHolder (#11)` if an address is repeated in `eligible`. Validation
  runs before the escrow transfer, so a rejected call moves no funds.
  Equivalent to `create_distribution_deadline(..., 0)` — no deadline.
- `create_distribution_deadline(admin, asset_token, payment_token, total_amount, eligible, deadline) -> u64` —
  same as above, plus sets the claim `deadline` (see policy above).
- `claimable(distribution_id, holder) -> i128` — the holder's remaining share
  (0 if already claimed / holds nothing / empty supply).
  **Panics** in two cases:
  - `DistributionNotFound (#4)` — if `distribution_id` does not exist.
  - `ArithmeticOverflow (#10)` — if `total_amount * snapshot_balance` does not
    fit in an `i128` (i.e. the product overflows 128-bit signed arithmetic).
    Integrators must ensure that `total_amount * max_holder_balance < i128::MAX`
    before creating a distribution; see `test_claimable_overflow_guarded` in
    `contracts/dividend/src/test.rs` for the concrete assertion.
- `claim(distribution_id, holder)` — holder auth; pays the claimable amount from
  escrow, marks claimed, updates `distributed`/`completed`. Errors:
  `AlreadyClaimed (#7)`, `NothingToClaim (#6)`, `DeadlinePassed (#13)`.
- `reclaim_unclaimed(admin, distribution_id) -> i128` — admin auth; after the
  deadline, sweeps `total_amount - distributed` to the admin. Errors:
  `NoDeadline (#15)`, `DeadlineNotReached (#14)`, `NothingToClaim (#6)`.
- `withdraw_unclaimed(admin, distribution_id, recipient, min_age_ledgers) -> i128` —
  admin auth; after the requested number of ledgers since creation, transfers
  the remaining amount to `recipient`. Errors: `DistributionTooYoung (#16)` or
  `NothingToClaim (#6)`.
- `cancel_distribution(admin, distribution_id)` — admin auth; returns escrowed
  funds to the issuer. Only works while nothing has been claimed (`distributed == 0`).
  Errors: `InvalidAmount (#5)` if any claim has been made.
- `get_distribution(distribution_id) -> Distribution` — `DistributionNotFound (#4)`.
- `get_distributions_for_asset(asset_token) -> Vec<Distribution>`
- `has_claimed(distribution_id, holder) -> bool`
- `get_admin() -> Address`
- `propose_admin(admin, new_admin)` — admin auth; records a pending successor.
  The role does not move yet.
- `accept_admin(new_admin)` — pending successor's auth; completes the handover.
- `cancel_admin_proposal(admin)` — admin auth; clears the pending successor.
  See [issue #4](fixes/issue-4.md) for the rationale.

## Errors

| Code | Name                 | Cause                                |
|------|----------------------|--------------------------------------|
| 1    | AlreadyInitialized   | double init                          |
| 2    | NotInitialized       | used before init                     |
| 3    | Unauthorized         | non-admin create                     |
| 4    | DistributionNotFound | unknown distribution id              |
| 5    | InvalidAmount        | `total_amount <= 0`, or an `eligible` entry has a negative balance |
| 6    | NothingToClaim       | claimable is zero                    |
| 7    | AlreadyClaimed       | holder already claimed this dist     |
| 8    | ZeroSupply           | `asset_token` total supply is `<= 0` |
| 9    | OverDistributed      | a claim would push `distributed` past `total_amount` |
| 10   | ArithmeticOverflow   | `total_amount * balance`, the snapshot total, or the running `distributed` total would overflow `i128` |
| 11   | DuplicateHolder      | the same address appears more than once in `eligible` |
| 12   | NoPendingAdmin       | admin handover operation has no pending proposal |
| 13   | DeadlinePassed       | claim attempted after the distribution deadline |
| 14   | DeadlineNotReached   | recovery attempted before the distribution deadline |
| 15   | NoDeadline           | reclaim attempted when the distribution has no deadline |
| 16   | DistributionTooYoung | withdrawal attempted before the minimum age |

## Events

| Topic     | Data                       | When                |
|-----------|----------------------------|---------------------|
| `init`    | admin                      | initialize          |
| `created` | (admin) → (id, total)      | distribution funded |
| `claim`   | (holder) → (id, amount)    | holder claims       |
| `withdraw` | (admin) → (id, recipient, amount) | remaining escrow withdrawn |
| `cancel`  | (admin) → distribution_id  | distribution cancelled |
| `set_admin` | (old_admin) → new_admin | admin handed over    |

## Storage / TTL

Listing of the contract `DataKey` variants and their storage behaviour. This
table is generated from the `DataKey` enum in `contracts/dividend/src/lib.rs`
via `scripts/generate_storage_docs.py` and reflects the snapshot-based
distribution model (issue #163): `Snapshot` and `Supply` freeze the
entitlement basis at `create_distribution` time, and `AssetIds` indexes
distributions per asset token (issue #166).

| Key | Payload | Storage | TTL / Notes |
|-----|---------|---------|-------------|
| `Admin` | - | instance | set once in `initialize`; never removed |
| `Counter` | - | instance | monotonically increasing distribution id |
| `Ids` | - | unused | legacy variant kept in the enum for ABI/storage-key stability; not read or written by any function |
| `Dist(u64)` | distribution id | persistent | `extend_ttl` on create and on every `claim`/`complete` update |
| `Claimed(u64, Address)` | distribution id, holder | persistent | set once per `(distribution, holder)` on `claim`; no explicit `extend_ttl` call |
| `AssetIds(Address)` | asset token | persistent | `Vec<u64>` of distribution ids for that asset token; appended and TTL-extended on every `create_distribution` |
| `Supply(u64)` | distribution id | persistent | snapshot total supply (the `claimable` denominator), frozen at creation; TTL-extended on create; removed once the distribution completes |
| `Snapshot(u64)` | distribution id | persistent | the frozen `eligible: Vec<(Address, i128)>` entitlement list; TTL-extended on create; removed once the distribution completes |

## Security considerations

- Funds are **escrowed** in the contract at creation, so payouts can't exceed
  what was funded.
- Double-claim is prevented by a per-`(distribution, holder)` claimed flag.
- Entitlements come from a **creation-time snapshot**: `create_distribution`
  takes an admin-supplied `eligible: Vec<(Address, i128)>` and freezes it, and
  every holder's share is `total_amount * snapshot_balance / sum(snapshot)`.
  Post-creation asset-token transfers cannot inflate or dilute a share.
- **Trust assumption (issue #293):** the `eligible` list is *not* cross-checked
  against `AssetClient::balance`. A malicious or buggy admin can assign shares
  to addresses/amounts unrelated to real holdings. Building an `eligible` list
  that mirrors the asset token's holders is the caller's responsibility. The
  contract only enforces structural sanity: each entry's balance must be
  non-negative (issue #291) and no address may appear twice (issue #290) — a
  duplicate would otherwise be counted in the denominator but be unclaimable,
  permanently stranding that slice of the escrow.
