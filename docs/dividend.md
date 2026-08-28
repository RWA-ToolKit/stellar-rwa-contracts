# Dividend Contract

Distributes yield/dividends to asset-token holders in proportion to their
holdings. An issuer funds a distribution with a payment token; each holder then
claims their share, paid from escrow held by this contract.

- Testnet: `CAR4XY3CEBQWFOL27JEWFW34KXSIZA7RFKDQMEIV7ZU723RWY37I2SYX`

## Proportional formula

At claim time a holder can claim:

```
claimable = total_amount * basis(holder) / supply
```

`basis` and `supply` come from a holder-balance snapshot (`eligible`) taken at
`create_distribution` time, not a live read of the asset token — this freezes
entitlement so moving tokens to another wallet after creation can't be used to
claim twice. Wallets absent from the snapshot have a basis of 0. Integer
division floors the result, and the multiplication is overflow-checked
(`ArithmeticOverflow`). Each holder can claim a given distribution **once**.

## `Distribution`

| Field           | Type      | Meaning                              |
|-----------------|-----------|---------------------------------------|
| `id`            | `u64`     | Distribution id (1-based)            |
| `asset_token`   | `Address` | Token whose holders are paid         |
| `payment_token` | `Address` | Token used to pay (e.g. a SAC)       |
| `total_amount`  | `i128`    | Total escrowed for the distribution  |
| `distributed`   | `i128`    | Amount claimed so far                |
| `created_at`    | `u32`     | Ledger at creation                   |
| `completed`     | `bool`    | True once `distributed >= total`     |

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
  contract's escrow, freezes `eligible` (a `Vec<(Address, i128)>` balance
  snapshot) as the entitlement basis, and records the distribution.
  `InvalidAmount (#5)` if `total_amount <= 0`, `ZeroSupply (#8)` if the token's
  live supply or the snapshot sum is `<= 0`.
- `claimable(distribution_id, holder) -> i128` — the holder's remaining share
  (0 if already claimed / not in the snapshot / empty supply). Read-only;
  never panics except on overflow.
- `claim(distribution_id, holder)` — holder auth; pays the claimable amount from
  escrow, marks claimed, updates `distributed`/`completed`. Errors:
  `AlreadyClaimed (#7)`, `NothingToClaim (#6)`, `OverDistributed (#9)`.
- `get_distribution(distribution_id) -> Distribution` — `DistributionNotFound (#4)`.
  Read-only.
- `get_distributions_for_asset(asset_token) -> Vec<Distribution>` — walks the
  per-asset id index rather than the global counter. Read-only.
- `has_claimed(distribution_id, holder) -> bool`
- `get_admin() -> Address`

## Errors

| Code | Name                 | Cause                                |
|------|----------------------|---------------------------------------|
| 1    | AlreadyInitialized   | double init                          |
| 2    | NotInitialized       | used before init                     |
| 3    | Unauthorized         | non-admin create                     |
| 4    | DistributionNotFound | unknown distribution id              |
| 5    | InvalidAmount        | `total_amount <= 0`                  |
| 6    | NothingToClaim       | claimable is zero                    |
| 7    | AlreadyClaimed       | holder already claimed this dist     |
| 8    | ZeroSupply           | asset/snapshot supply is `<= 0`      |
| 9    | OverDistributed      | `distributed` would exceed `total_amount` |
| 10   | ArithmeticOverflow   | `total_amount * basis` overflowed `i128` |

## Events

| Topic     | Data                       | When                |
|-----------|----------------------------|---------------------|
| `init`    | admin                      | initialize          |
| `created` | (admin) → (id, total)      | distribution funded |
| `claim`   | (holder) → (id, amount)    | holder claims       |

## Storage / TTL

Listing of the contract `DataKey` variants and their storage behaviour. Only
state-changing entry points (`create_distribution`, `claim`) extend TTLs;
read-only views (`load`'s callers, `get_distributions_for_asset`) never touch
TTL so they stay eligible as free simulated reads.

| Key         | Payload      | Storage    | TTL / Notes                    |
|-------------|--------------|------------|---------------------------------|
| `Admin`     | -            | instance   | -                               |
| `Counter`   | -            | instance   | -                               |
| `Dist`      | u64          | persistent | extended on create/claim only  |
| `Claimed`   | u64, Address | persistent | not extended after write       |
| `Snapshot`  | u64          | persistent | extended on create only        |
| `Supply`    | u64          | persistent | extended on create only        |
| `AssetIds`  | Address      | persistent | extended on create only        |

## Security considerations

- Funds are **escrowed** in the contract at creation, so payouts can't exceed
  what was funded, and `OverDistributed` guards the escrow from underflowing.
- Double-claim is prevented by a per-`(distribution, holder)` claimed flag.
- Entitlement is frozen via the `eligible` snapshot at creation, so moving
  tokens to a fresh wallet after a distribution is created cannot be used to
  claim a second time.
- Read-only functions (`get_distribution`, `claimable`,
  `get_distributions_for_asset`) never call `extend_ttl`, so they remain cheap
  simulated reads instead of mutating state on every call.
