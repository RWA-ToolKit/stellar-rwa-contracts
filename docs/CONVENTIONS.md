# Units & Conventions

Valuation, token amounts and time each use a different representation across
these contracts. This page collects the three conventions in one place, with
worked examples, so they don't have to be pieced together from scattered doc
comments (issue #266).

## Valuation — USD cents (`i128`)

`valuation` (on `AssetMetadata` in **asset-token** and `AssetEntry` in
**registry**) is an integer count of **USD cents**, not dollars and not a
fixed-point/decimal type.

```
$1,234.56  →  valuation = 123_456
$50,000.00 →  valuation = 5_000_000
```

To render a stored valuation as dollars: `dollars = valuation / 100`,
`cents = valuation % 100`.

The registry's copy of `valuation` is a cache of the asset token's own copy,
kept in sync by routing every change through `RegistryContract::update_valuation`,
which cross-invokes the token's `update_valuation` in the same call
(see [registry.md](registry.md)). Reading `AssetEntry.valuation` or
`total_value_locked()` never requires a separate call to the token.

## Token amounts — base units scaled by `decimals`

Token amounts (`balance`, `total_supply`, `mint`/`burn`/`transfer` amounts,
and dividend `total_amount`/`distributed`) are integers in the token's own
base unit, scaled by its `decimals` field (set once at `initialize` and
returned by `get_metadata`). There is no on-chain concept of a "whole token" —
that scaling is a display-layer convention, the same as Stroops for XLM
(1 XLM = 10,000,000 stroops, i.e. 7 decimals).

```
decimals = 2, "100.00 shares" → amount = 10_000
decimals = 7, "1.5000000 XLM" → amount = 15_000_000
```

To render a stored amount for a human: `whole = amount / 10^decimals`,
`fraction = amount % 10^decimals`. Always do this scaling in the client —
never divide by `10^decimals` on-chain, since Soroban has no floating point
and integer division would floor/truncate the stored value itself.

## Time — ledger sequence numbers, not wall-clock

Every timestamp-shaped field (`created_at` on `AssetEntry` and `Distribution`,
`claim_deadline` on `Distribution`, compliance expiry) is a **ledger sequence
number** — a monotonically increasing counter of closed Stellar ledgers, not a
Unix timestamp or calendar date. Ledgers close roughly every 5–6 seconds, so a
rough wall-clock duration converts to a ledger count as:

```
seconds_from_now / ~5.5  ≈  ledgers_from_now
30 days   → ~17,280 * 30 = 518,400 ledgers   (see DAY_IN_LEDGERS in each contract)
```

This is an approximation, not a guarantee — ledger close time drifts with
network conditions. Don't rely on it for anything that must expire at an exact
wall-clock instant; use it only for "roughly N days" style deadlines like
`claim_deadline`. To convert a sequence number back to an estimated date,
read the current ledger's close time from Soroban RPC and extrapolate from
the *current* sequence, not a fixed genesis offset.

## See also

- [asset-token.md](asset-token.md), [registry.md](registry.md),
  [dividend.md](dividend.md), [compliance.md](compliance.md) — per-contract
  field and function reference.
