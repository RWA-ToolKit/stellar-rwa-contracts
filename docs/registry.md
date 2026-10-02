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
  issuer auth; assigns and returns the id. `InvalidValuation (#5)` if negative,
  `InvalidInput (#7)` if `name` is empty, `name` exceeds 64 bytes, or
  `asset_type` is not one of the canonical [asset types](#asset-types).
  The 64-byte cap matches the asset-token contract's `MAX_NAME_LEN` and is
  counted in UTF-8 bytes, so multibyte characters consume more than one byte
  toward the limit. Rejects `token_contract` values already registered under
  another id with `DuplicateAsset (#9)` — see "Duplicate registration" below.
- `get_asset(asset_id) -> AssetEntry` — `AssetNotFound (#4)`.
- `get_assets_by_issuer(issuer) -> Vec<AssetEntry>` — returns the first
  page (up to `MAX_PAGE_SIZE` = 100 entries) of assets registered by this
  issuer. Use `get_assets_by_issuer_page` to page through larger result sets
  (issue #430).
- `get_assets_by_issuer_page(issuer, page, page_size) -> Vec<AssetEntry>` —
  paginated variant (issue #430). Returns up to `page_size` entries starting
  at offset `page × page_size` within the issuer's id list. `page_size` is
  clamped to `MAX_PAGE_SIZE` (100). Page through by incrementing `page` until
  the result is shorter than `page_size` (or empty).
- `get_assets_by_type(asset_type) -> Vec<AssetEntry>` — returns the first
  page (up to `MAX_PAGE_SIZE` = 100 entries) of assets with this type. See
  [matching rules](#asset-types). Use `get_assets_by_type_page` for larger
  result sets (issue #430).
- `get_assets_by_type_page(asset_type, page, page_size) -> Vec<AssetEntry>` —
  paginated variant (issue #430). Same paging semantics as
  `get_assets_by_issuer_page`. `page_size` clamped to `MAX_PAGE_SIZE`.
- `get_all_assets(start_id, limit) -> Vec<AssetEntry>` — returns ids
  `[start_id, start_id + limit)`, capped at the current counter and at
  `MAX_PAGE_SIZE` (100) regardless of the requested `limit`. Page through the
  full registry by calling again with `start_id + <count returned>`. A small
  registry that fits in one page keeps working with a single call
  (`start_id = 1`, a large `limit`).
- `deactivate_asset(admin, asset_id)` — admin auth; sets `active=false`;
  removes the asset's valuation from `total_value_locked()`. No-op (no event)
  if the asset is already inactive.
- `reactivate_asset(admin, asset_id)` — admin auth; sets `active=true`;
  restores the asset's valuation to `total_value_locked()`. No-op (no event)
  if the asset is already active. See [Deactivation is reversible](#deactivation-is-reversible).
- `update_valuation(admin, asset_id, new_valuation)` — admin auth; updates
  `AssetEntry.valuation`, adjusts TVL if the asset is `active`, and emits a
  `valuation` event (see Events below) so indexers observe the change without
  polling. `InvalidValuation (#5)` if negative; `AssetNotFound (#4)`.
- `total_value_locked() -> i128` — sum of `valuation` over active assets.
  O(1): maintained as a running total updated on register, deactivate and
  reactivate, never recomputed by iterating the registry.
- `asset_count() -> u64` — total registrations, active or not.
- `active_count() -> u64` — registrations currently active.
- `get_total_asset_count() -> u32` — same as `asset_count` but typed as `u32`;
  provided as a named companion for callers building paginated UIs alongside
  `get_assets_page` (issue #455).
- `get_assets_page(start_index: u32, page_size: u32) -> Vec<AssetEntry>` —
  zero-based index pagination over all registered assets ordered by id.
  `page_size` is capped at `MAX_PAGE_SIZE` (100). Returns empty when
  `start_index` is at or beyond the total count. See
  [Pagination helpers](#pagination-helpers-issue-455).
- `get_active_assets_page(start_index: u32, page_size: u32, active_only: bool) -> Vec<AssetEntry>` —
  same as `get_assets_page` but filters by the `active` flag before
  indexing, so `start_index` is a position within the *filtered* list.
  Costs scale with the full registry size (linear scan). See
  [Pagination helpers](#pagination-helpers-issue-455).
- `get_admin() -> Address`
- `propose_admin(admin, new_admin)` — admin auth; records a pending successor.
  The role does not move yet.
- `accept_admin(new_admin)` — pending successor's auth; completes the handover.
- `cancel_admin_proposal(admin)` — admin auth; clears the pending successor.
  See [issue #4](fixes/issue-4.md) for the rationale.

## Asset types

`asset_type` is validated against a fixed, canonical list at registration
time (`VALID_ASSET_TYPES` in `contracts/registry/src/lib.rs`):

`real_estate`, `invoice`, `commodity`, `bond`, `equity`, `fund`

Any other value — including a near-miss like a typo, different casing, or
extra whitespace — is rejected with `InvalidInput (#7)`. This prevents a
typo from silently creating a category that no filter will ever match.

**Matching rule for `get_assets_by_type`:** matching is byte-exact
(case-sensitive and whitespace-sensitive). The per-type index key is the
`asset_type` string exactly as it was stored at registration, so
`"real_estate"`, `"Real_Estate"` and `"real_estate "` are distinct keys.
Because `register_asset` only ever accepts the canonical lowercase strings
above, callers should always query with one of those exact strings — passing
a differently-cased or padded value returns an empty list, not an error.

## Deactivation is reversible

`deactivate_asset` is **not** a one-way operation. An asset can be
deactivated by mistake (wrong id, premature admin action), and forcing a
re-registration to "undo" it would assign a new id, breaking any external
references, the issuer index, and the type index that point at the original
id. Instead, `reactivate_asset` restores the same `AssetEntry` in place: it
flips `active` back to `true` and adds the valuation back into both
`active_count()` and `total_value_locked()`. Both operations are admin-only
and idempotent (repeating either one when already in that state is a no-op).

### Duplicate registration (issue #308)

`register_asset` maintains a reverse index from `token_contract` to its
assigned asset id. If the same token contract address is registered a second
time — under any issuer, name, or asset type — the call reverts with
`DuplicateAsset (#9)` before any state changes. This is intentional: allowing
the same token contract under two registry ids would let `total_value_locked`
and `get_all_assets` double-count it, inflating the TVL figure shown on the
landing page and producing duplicate entries on the explore page. Covered by
`test_duplicate_token_contract_registration_rejected` in
`contracts/registry/src/test.rs`.

### Pagination and max page size (issue #310, issue #430)

`get_all_assets` always enforces `MAX_PAGE_SIZE = 100` as an upper bound on
the number of entries returned in one call, independent of the `limit`
argument passed in. This keeps per-call cost bounded as the registry grows,
while existing callers that pass a large `limit` to fetch everything in one
shot keep working unchanged as long as the registry is smaller than the cap.
The final page of a paginated walk is partial once fewer than `limit` assets
remain; see `test_get_all_assets_final_partial_page` and
`test_get_all_assets_enforces_max_page_size` in
`contracts/registry/src/test.rs`.

`get_assets_by_issuer` and `get_assets_by_type` previously returned the full
index vector for an issuer or type (issue #430), which was unbounded as the
registry grew. Both functions are now capped at `MAX_PAGE_SIZE` per call, and
the new `get_assets_by_issuer_page` / `get_assets_by_type_page` variants
expose explicit `(page, page_size)` paging. Every registration still rewrites
the full index vector; that write cost is O(N) in the number of assets for
that issuer/type and is noted in the storage section below.

### Deactivation and TVL (issue #306)

`deactivate_asset` is intentionally **exclusive**: as soon as an asset is
deactivated, its `valuation` is subtracted from `TotalValuation` in the same
call, and it is excluded from every subsequent `total_value_locked()` read.
It is never re-added implicitly — an asset must be re-registered (as a new
id) to count again. This is the correct behavior for a headline TVL figure:
a deactivated asset (e.g. delisted, fraudulent, or redeemed) should not
inflate the number shown on the landing page. The web app's TVL display
reads `total_value_locked()` directly, so it reflects this automatically.
Covered by `test_deactivate_excludes_from_tvl` and `test_tvl_sums_only_active`
in `contracts/registry/src/test.rs`.

## Errors

| Code | Name               | Cause                                          |
|------|--------------------|-------------------------------------------------|
| 1    | AlreadyInitialized | double init                                    |
| 2    | NotInitialized     | used before init                               |
| 3    | Unauthorized       | non-admin deactivate/reactivate                |
| 4    | AssetNotFound      | unknown id                                     |
| 5    | InvalidValuation   | negative valuation                             |
| 6    | Overflow           | TVL running total overflowed i128              |
| 7    | InvalidInput       | empty name, or `asset_type` not in the canonical list |
| 8    | NoPendingAdmin     | `accept_admin`/`cancel_admin_proposal` with no pending proposal |
| 9    | DuplicateAsset     | token_contract already registered |

## Events

| Topic       | Data              | When                |
|-------------|-------------------|---------------------|
| `init`      | admin             | initialize          |
| `register`  | (issuer) → id     | asset registered    |
| `deactvate` | asset_id          | asset deactivated   |
| `reactvate` | asset_id          | asset reactivated   |
| `set_admin` | (old_admin) → new_admin | admin handed over |
| `valuation` | (asset_id) → (old_valuation, new_valuation) | valuation changed via `update_valuation` (issue #3) |

## Storage / TTL

Listing of the contract `DataKey` variants and their storage behaviour.

| Key | Payload | Storage | TTL / Notes |
|-----|---------|---------|-------------|
| `Admin` | - | instance | - |
| `Counter` | - | instance | monotonic id counter |
| `Ids` | - | - | legacy key, no longer written; kept for read-compat with old deployments |
| `Asset` | u64 | persistent | extended on read/write |
| `ActiveCount` | - | instance | count of currently-active assets |
| `IssuerIndex` | Address | persistent | ids registered by that issuer; extended on read/write |
| `TypeIndex` | String | persistent | ids of that exact `asset_type` string; extended on read/write |
| `TotalValuation` | - | instance | running TVL total; O(1) read, updated on register/deactivate/reactivate |

## Pagination helpers (issue #455)

The registry exposes three functions for efficient paginated access by API
consumers and the web app.

### `get_total_asset_count() -> u32`

Returns the total number of registered assets (active and inactive). Equivalent
to `asset_count()` but typed as `u32` to match the `u32` index parameters of
the pagination functions below.

### `get_assets_page(start_index, page_size) -> Vec<AssetEntry>`

Zero-based index pagination over all registered assets, ordered by id:

| Parameter     | Semantics |
|---------------|-----------|
| `start_index` | First entry to return (0-based). Returns empty when ≥ total count. |
| `page_size`   | Max entries to return. Capped at `MAX_PAGE_SIZE` (100). `0` also returns up to the cap. |

Typical walk:
```
page 0 → get_assets_page(0, 20)   → items 0–19
page 1 → get_assets_page(20, 20)  → items 20–39
...
last   → get_assets_page(N, 20)   → partial or empty
```

### `get_active_assets_page(start_index, page_size, active_only) -> Vec<AssetEntry>`

Same pagination semantics as `get_assets_page`, but filtered by `active`:

- `active_only = true` — returns only assets with `active == true`.
- `active_only = false` — returns only deactivated assets.

`start_index` counts positions within the **filtered** list, not within the
full registry. This function performs a linear scan over all registered ids on
every call; for large registries, prefer per-issuer or per-type index queries
when the filter is not required.

## Security considerations

- Registration requires the **issuer** to authorize; anyone can register their
  own asset, but only the admin can deactivate or reactivate entries.
- TVL is a running total over active entries, updated incrementally on
  register, deactivate and reactivate, so it reflects the current state
  immediately without iterating the registry.
