# Registry Contract

A canonical on-chain index of every tokenized asset on the platform. Each issuer
registers their asset-token contract; the registry assigns an incrementing id
and reports total value locked (TVL).

- Testnet: `CBX5SMLTXX6JP4HA5GQIO2V6QM7WCUGL2GZ6D4U773HMRI6RXISKPUR3`

## `AssetEntry`

| Field           | Type      | Meaning                          |
|-----------------|-----------|----------------------------------|
| `id`            | `u64`     | Registry id (1-based)            |
| `token_contract`| `Address` | The asset-token contract         |
| `issuer`        | `Address` | Who registered it                |
| `name`          | `String`  | Asset name                       |
| `asset_type`    | `String`  | `real_estate` / `invoice` / ...  |
| `valuation`     | `i128`    | USD cents, snapshot at registration (see below) |
| `created_at`    | `u32`     | Ledger sequence at registration  |
| `active`        | `bool`    | Counted in TVL while true        |

## Functions

- `initialize(admin)` — sets admin. Once only. `AlreadyInitialized (#1)`.
- `register_asset(issuer, token_contract, name, asset_type) -> u64` — issuer
  auth; also cross-calls `token_contract.get_metadata()` and requires
  `issuer` to be that token's admin (`Unauthorized (#3)` otherwise), so only
  the contract's real admin can list it. `valuation` is read from that same
  call, not caller-supplied. Assigns and returns the id.
- `get_asset(asset_id) -> AssetEntry` — `AssetNotFound (#4)`.
- `get_assets_by_issuer(issuer) -> Vec<AssetEntry>`
- `get_assets_by_type(asset_type) -> Vec<AssetEntry>`
- `get_all_assets() -> Vec<AssetEntry>`
- `deactivate_asset(admin, asset_id)` — admin auth; sets `active=false`.
- `total_value_locked() -> i128` — sum of each active asset's *live*
  `get_metadata().valuation`, not the registry's cached copy.
- `asset_count() -> u64`
- `get_admin() -> Address`

## Errors

| Code | Name               | Cause                          |
|------|--------------------|--------------------------------|
| 1    | AlreadyInitialized | double init                    |
| 2    | NotInitialized     | used before init               |
| 3    | Unauthorized       | non-admin deactivation, or issuer isn't the token's admin |
| 4    | AssetNotFound      | unknown id                     |
| 5    | InvalidValuation   | negative valuation             |

## Events

| Topic       | Data              | When              |
|-------------|-------------------|-------------------|
| `init`      | admin             | initialize        |
| `register`  | (issuer) → id     | asset registered  |
| `deactvate` | asset_id          | asset deactivated |

## Storage / TTL

Listing of the contract `DataKey` variants and their storage behaviour.

| Key | Payload | Storage | TTL / Notes |
|-----|---------|---------|-------------|
| `Admin` | - | instance | - |
| `Counter` | - | instance | - |
| `Ids` | - | unknown | - |
| `Asset` | u64 | persistent | extended via instance() |

## Security considerations

- Registration requires the **issuer** to authorize *and* to be the on-chain
  admin of `token_contract` (verified via cross-contract call), so an address
  can't list a contract it doesn't control (issue #169). Only the registry
  admin can deactivate entries.
- TVL is computed from active entries only, so deactivating an asset removes it
  from platform totals immediately. Valuations are read live from each token
  at TVL-computation time, not trusted from the registry's own copy
  (issue #170).
