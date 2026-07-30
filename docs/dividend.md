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

where `balance` and `total_supply` are read live from the asset token. Integer
division floors the result. Each holder can claim a given distribution **once**.

## `Distribution`

| Field            | Type      | Meaning                              |
|------------------|-----------|--------------------------------------|
| `id`             | `u64`     | Distribution id (1-based)            |
| `asset_token`    | `Address` | Token whose holders are paid         |
| `payment_token`  | `Address` | Token used to pay (e.g. a SAC)       |
| `total_amount`   | `i128`    | Total escrowed for the distribution  |
| `distributed`    | `i128`    | Amount claimed so far                |
| `snapshot_ledger`| `u32`     | Ledger at creation (reference)       |
| `created_at`     | `u32`     | Ledger at creation                   |
| `completed`      | `bool`    | True once `distributed >= total`     |

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
- `create_distribution(admin, asset_token, payment_token, total_amount) -> u64` —
  admin auth; pulls `total_amount` of `payment_token` from the admin into the
  contract's escrow and records the distribution. `InvalidAmount (#5)` if
  `total_amount <= 0`.
- `claimable(distribution_id, holder) -> i128` — the holder's remaining share
  (0 if already claimed / holds nothing / empty supply). Never panics.
- `claim(distribution_id, holder)` — holder auth; pays the claimable amount from
  escrow, marks claimed, updates `distributed`/`completed`. Errors:
  `AlreadyClaimed (#7)`, `NothingToClaim (#6)`.
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

## Events

| Topic     | Data                       | When                |
|-----------|----------------------------|---------------------|
| `init`    | admin                      | initialize          |
| `created` | (admin) → (id, total)      | distribution funded |
| `claim`   | (holder) → (id, amount)    | holder claims       |

## Storage / TTL

Listing of the contract `DataKey` variants and their storage behaviour.

| Key | Payload | Storage | TTL / Notes |
|-----|---------|---------|-------------|
| `Admin` | - | instance | - |
| `Counter` | - | instance | - |
| `Ids` | - | unknown | - |
| `Dist` | u64 | persistent | extended via instance() |
| `Claimed` | u64, Address | unknown | per-key TTL |

## Security considerations

- Funds are **escrowed** in the contract at creation, so payouts can't exceed
  what was funded.
- Double-claim is prevented by a per-`(distribution, holder)` claimed flag.
- Balances are read at claim time. For assets whose balances move a lot between
  creation and claiming, a future version can add a true balance snapshot; v1
  documents this trade-off explicitly.
