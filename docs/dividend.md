# Dividend Contract

Distributes yield/dividends to asset-token holders in proportion to their
holdings. An issuer funds a distribution with a payment token; each holder then
claims their share, paid from escrow held by this contract.

- Testnet: `CAR4XY3CEBQWFOL27JEWFW34KXSIZA7RFKDQMEIV7ZU723RWY37I2SYX`

## Proportional formula

A holder can claim:

```
claimable = total_amount * snapshot_balance(holder) / snapshot_supply
```

where `snapshot_balance` is the holder's balance as recorded in the snapshot
supplied at creation and `snapshot_supply` is the sum of all snapshotted
balances. Integer division floors the result. Each holder can claim a given
distribution **once**.

Live balances are never consulted. Moving tokens after a distribution is created
changes nothing: the recipient has no snapshot entry and so has nothing to claim.

## The holder snapshot

A Soroban contract cannot enumerate a token's holders on-chain, so the snapshot
is supplied by the admin as a `Vec<SnapshotEntry>` when the distribution is
created:

```rust
#[contracttype]
pub struct SnapshotEntry {
    pub holder: Address,
    pub balance: i128,
}
```

It is rejected (`InvalidSnapshot (#8)`) if it is empty, contains a non-positive
balance, repeats a holder, or claims more tokens in total than the asset's
`total_supply`. Because `snapshot_supply` is the sum of the entries rather than
the token's live supply, the shares can never add up to more than `total_amount`
even when the snapshot only covers some holders.

The snapshot has to fit in a single transaction, so very large holder sets
should be paid out over several distributions.

## `Distribution`

| Field             | Type      | Meaning                                   |
|-------------------|-----------|-------------------------------------------|
| `id`              | `u64`     | Distribution id (1-based)                 |
| `asset_token`     | `Address` | Token whose holders are paid              |
| `payment_token`   | `Address` | Token used to pay (e.g. a SAC)            |
| `total_amount`    | `i128`    | Total escrowed for the distribution       |
| `distributed`     | `i128`    | Amount claimed so far                     |
| `snapshot_supply` | `i128`    | Sum of snapshotted balances (denominator) |
| `allocated`       | `i128`    | Sum of the floored shares                 |
| `snapshot_ledger` | `u32`     | Ledger the snapshot was taken at          |
| `created_at`      | `u32`     | Ledger at creation                        |
| `completed`       | `bool`    | True once `distributed >= allocated`      |

`total_amount - allocated` is the rounding dust: flooring every share means the
shares almost never sum to the full amount. `completed` is compared against
`allocated` so that a fully claimed distribution actually closes, and the dust
is recovered with `reclaim_unclaimed`.

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
- `create_distribution(admin, asset_token, payment_token, total_amount, snapshot) -> u64` —
  admin auth; records the holder snapshot, then pulls `total_amount` of
  `payment_token` from the admin into the contract's escrow.
  `InvalidAmount (#5)` if `total_amount <= 0`, `InvalidSnapshot (#8)` if the
  snapshot is empty, malformed, or larger than the asset's supply.
- `claimable(distribution_id, holder) -> i128` — the holder's remaining share
  (0 if already claimed / not in the snapshot / distribution closed). Only
  panics on an unknown id (`DistributionNotFound (#4)`).
- `claim(distribution_id, holder)` — holder auth; pays the claimable amount from
  escrow, marks claimed, updates `distributed`/`completed`. Errors:
  `AlreadyClaimed (#7)`, `NothingToClaim (#6)`, `DistributionClosed (#11)`.
- `reclaim_unclaimed(admin, distribution_id) -> i128` — admin auth; sweeps the
  remaining escrow back to the admin and closes the distribution. Permitted once
  every allocated share has been claimed (only dust left) or once the 14-day
  claim window has elapsed. Errors: `ClaimWindowOpen (#9)`,
  `NothingToReclaim (#10)`.
- `snapshot_balance(distribution_id, holder) -> i128` — the holder's snapshotted
  balance, 0 if not included.
- `get_distribution(distribution_id) -> Distribution` — `DistributionNotFound (#4)`.
- `get_distributions_for_asset(asset_token) -> Vec<Distribution>`
- `has_claimed(distribution_id, holder) -> bool`
- `get_admin() -> Address`

## Errors

| Code | Name                 | Cause                                |
|------|----------------------|--------------------------------------|
| 1    | AlreadyInitialized   | double init                          |
| 2    | NotInitialized       | used before init                     |
| 3    | Unauthorized         | non-admin create                     |
| 4    | DistributionNotFound | unknown distribution id              |
| 5    | InvalidAmount        | `total_amount <= 0`                  |
| 6    | NothingToClaim       | claimable is zero                    |
| 7    | AlreadyClaimed       | holder already claimed this dist     |
| 8    | InvalidSnapshot      | empty/duplicate/oversized snapshot   |
| 9    | ClaimWindowOpen      | reclaim before the window closed     |
| 10   | NothingToReclaim     | escrow already empty                 |
| 11   | DistributionClosed   | claim on a closed distribution       |

## Events

| Topic     | Data                       | When                |
|-----------|----------------------------|---------------------|
| `init`    | admin                      | initialize          |
| `created` | (admin) → (id, total)      | distribution funded |
| `claim`   | (holder) → (id, amount)    | holder claims       |
| `reclaim` | (admin) → (id, amount)     | escrow swept        |

## Storage lifetime

Distributions, snapshot entries and claim markers are persistent entries whose
TTL is extended by `PERSISTENT_BUMP_AMOUNT` (60 days) on every write. This
matters most for the claim marker: if it were archived before its distribution,
`has_claimed` would read `false` again and the holder could claim a second time.
The claim window (14 days) is deliberately shorter than both that bump and the
instance bump, so nothing the contract needs can expire while claiming is open.

## Security considerations

- Funds are **escrowed** in the contract at creation, so payouts can't exceed
  what was funded.
- Shares are fixed against the **snapshot** taken at creation, so a holder can't
  claim, move the tokens to another approved address, and claim again.
- Double-claim is prevented by a per-`(distribution, holder)` claimed flag whose
  TTL is bumped on write so it cannot be archived out from under the check.
- `snapshot_supply` is the sum of the snapshot rather than the token's live
  supply, so a partial snapshot under-allocates instead of over-allocating.
- The snapshot is admin-supplied and therefore trusted: a dishonest admin can
  weight a distribution however they like. The contract bounds the damage to the
  escrowed `total_amount` and rejects snapshots exceeding the asset's supply.
