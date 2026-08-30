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
| `valuation`     | `i128`    | USD cents                        |
| `created_at`    | `u32`     | Ledger sequence at registration  |
| `active`        | `bool`    | Counted in TVL while true        |

## Functions

- `initialize(admin)` — sets admin. Once only. `AlreadyInitialized (#1)`.
- `register_asset(issuer, token_contract, name, asset_type, valuation) -> u64` —
  issuer auth; assigns and returns the id. `InvalidValuation (#5)` if negative.
- `update_valuation(issuer, asset_id, new_valuation)` — issuer auth; the *only*
  path for changing valuation post-registration. It cross-invokes the asset
  token's own `update_valuation` (which independently requires `issuer` to
  authorize as the token's admin) before updating the registry entry and TVL,
  so the registry's copy and the token's copy never observably disagree
  (issues #264, #265). `InvalidValuation (#5)` if negative, `AssetNotFound (#4)`
  if unknown, `Unauthorized (#3)` if `issuer` isn't the asset's issuer.
- `get_asset(asset_id) -> AssetEntry` — `AssetNotFound (#4)`.
- `get_assets_by_issuer(issuer) -> Vec<AssetEntry>`
- `get_assets_by_type(asset_type) -> Vec<AssetEntry>`
- `get_all_assets() -> Vec<AssetEntry>`
- `deactivate_asset(admin, asset_id)` — admin auth; sets `active=false`.
- `total_value_locked() -> i128` — sum of `valuation` over active assets.
- `asset_count() -> u64`
- `get_admin() -> Address`

## Errors

| Code | Name               | Cause                          |
|------|--------------------|--------------------------------|
| 1    | AlreadyInitialized | double init                    |
| 2    | NotInitialized     | used before init               |
| 3    | Unauthorized       | non-admin deactivation         |
| 4    | AssetNotFound      | unknown id                     |
| 5    | InvalidValuation   | negative valuation             |

## Events

| Topic       | Data              | When              |
|-------------|-------------------|-------------------|
| `init`      | admin             | initialize        |
| `register`  | (issuer) → id     | asset registered  |
| `deactvate` | asset_id          | asset deactivated |
| `valuation` | (asset_id) → new_valuation | valuation updated |

## Storage / TTL

Listing of the contract `DataKey` variants and their storage behaviour.

| Key | Payload | Storage | TTL / Notes |
|-----|---------|---------|-------------|
| `Admin` | - | instance | - |
| `Counter` | - | instance | - |
| `Ids` | - | unknown | - |
| `Asset` | u64 | persistent | extended via instance() |

## Security considerations

- Registration requires the **issuer** to authorize; anyone can register their
  own asset, but only the admin can deactivate entries.
- `update_valuation` requires the **issuer**, and the cross-call into the
  token independently requires that same address to be the token's admin —
  so an issuer who isn't the token's admin can't desync the two by calling
  only one side.
- TVL is computed from active entries only, so deactivating an asset removes it
  from platform totals immediately.
