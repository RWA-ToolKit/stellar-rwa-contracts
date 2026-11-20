# Valuation Change Guard

## What was implemented

- `contracts/asset-token/src/lib.rs`: `update_valuation` now rejects any
  single call that would move the valuation by more than
  `MAX_VALUATION_CHANGE_BPS` (5,000 bps = 50%) of the previous value. A
  valuation of `0` is exempt since there is no prior magnitude to compare
  against. Violations panic with the new `Error::ValuationChangeTooLarge`
  (code 13).
- The `valuation` event now publishes `(old_valuation, new_valuation)`
  instead of just the new value, so subscribers (including the registry and
  the web app) can see the delta without an extra read.
- `contracts/registry/src/lib.rs`: added a doc comment on
  `total_value_locked` explaining why the guard exists — a runaway or
  mistyped valuation on one asset-token contract would otherwise be able to
  skew the registry's aggregate TVL for every downstream client.

## Why a percentage guard

A flat cap (e.g. "no more than $X per update") doesn't scale across assets
of very different sizes. A percentage-of-previous-value guard scales
naturally: legitimate re-appraisals rarely move a real-world asset's value
by more than half in one update, while a fat-fingered extra digit (a
10x+ change) is reliably caught. Admins who need a larger legitimate
change can phase it in across multiple calls.

## Tests

- `test_update_valuation_oversized_change_rejected` — a >50% jump panics
  with `Error::ValuationChangeTooLarge`.
- `test_update_valuation_emits_event` — a within-bounds update still
  succeeds and emits an event.
- Existing valuation tests (`test_update_valuation`,
  `test_get_metadata_reflects_all_mutations`) were adjusted to use
  in-bounds values so they keep passing under the new guard.
