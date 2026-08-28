# Dividend Contract

Distributes yield/dividends to asset-token holders in proportion to their
holdings. An issuer funds a distribution with a payment token; each holder then
claims their share, paid from escrow held by this contract.

- Testnet: `CAR4XY3CEBQWFOL27JEWFW34KXSIZA7RFKDQMEIV7ZU723RWY37I2SYX`

## Proportional formula

At claim time a holder can claim:

```
claimable = total_amount * snapshot_balance(holder) / snapshot_supply
```

where `snapshot_balance`/`snapshot_supply` come from the frozen `eligible`
list passed to `create_distribution` at creation time — **not** read live
from the asset token at claim time. This means a holder who transfers away
their tokens after a distribution is created can still claim their original
share, and a wallet that only receives tokens after creation has a basis of
0 and cannot claim from that distribution. Integer division floors the
result, so a pool that doesn't divide evenly by the snapshot supply leaves a
small remainder permanently unclaimable (`completed` never becomes `true` in
that case). Each holder can claim a given distribution **once**.

## `Distribution`

| Field            | Type      | Meaning                              |
|------------------|-----------|--------------------------------------|
| `id`             | `u64`     | Distribution id (1-based)            |
| `asset_token`    | `Address` | Token whose holders are paid         |
| `payment_token`  | `Address` | Token used to pay (e.g. a SAC)       |
| `total_amount`   | `i128`    | Total escrowed for the distribution  |
| `distributed`    | `i128`    | Amount claimed so far                |
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
- `create_distribution(admin, asset_token, payment_token, total_amount, eligible) -> u64` —
  admin auth; pulls `total_amount` of `payment_token` from the admin into the
  contract's escrow, freezes `eligible: Vec<(Address, i128)>` as the payout
  snapshot (its balances summed become the snapshot supply used by
  `claimable`), and records the distribution. `InvalidAmount (#5)` if
  `total_amount <= 0`, `ZeroSupply (#8)` if `asset_token`'s live
  `total_supply()` is `<= 0` at creation time.
- `claimable(distribution_id, holder) -> i128` — the holder's remaining share
  (0 if already claimed / holds nothing / empty supply). Never panics.
- `claim(distribution_id, holder)` — holder auth; pays the claimable amount from
  escrow, marks claimed, updates `distributed`/`completed`. Errors:
  `AlreadyClaimed (#7)`, `NothingToClaim (#6)`.
- `get_distribution(distribution_id) -> Distribution` — `DistributionNotFound (#4)`.
- `get_distributions_for_asset(asset_token) -> Vec<Distribution>`
- `has_claimed(distribution_id, holder) -> bool`
- `get_admin() -> Address`

## Usage examples

Fund a distribution against a frozen snapshot and let two holders claim:

```rust
client.initialize(&admin);

// Freeze balances as of right now: h1=300, h2=200 (snapshot supply = 500).
let mut eligible = Vec::new(&env);
eligible.push_back((h1.clone(), 300));
eligible.push_back((h2.clone(), 200));

// Pulls 1_000 of `payment_token` from `admin` into escrow.
let dist_id = client.create_distribution(&admin, &asset_token, &payment_token, &1_000, &eligible);

assert_eq!(client.claimable(&dist_id, &h1), 600); // 1000 * 300 / 500
assert_eq!(client.claimable(&dist_id, &h2), 400); // 1000 * 200 / 500

client.claim(&dist_id, &h1);
assert_eq!(client.claimable(&dist_id, &h1), 0);
assert!(client.has_claimed(&dist_id, &h1));
```

Check a distribution's progress without claiming:

```rust
let d = client.get_distribution(&dist_id);
assert_eq!(d.total_amount, 1_000);
assert_eq!(d.distributed, 600); // after h1's claim above
assert!(!d.completed);          // h2 hasn't claimed yet
```

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
| 8    | ZeroSupply           | `asset_token.total_supply()` is `<= 0` at creation — no holder could ever claim |
| 9    | OverDistributed      | internal guard: total claimed would exceed `total_amount` |

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
