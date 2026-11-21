# Pause Policy Coverage

## What was implemented

- `contracts/asset-token/src/lib.rs`: documented the `pause` policy
  explicitly in a doc comment — while paused, `transfer`, `transfer_from`,
  `mint`, `mint_batch`, `burn` and `approve` all revert with `Error::Paused`.
  Read-only calls (`balance`, `allowance`, `get_metadata`, `total_supply`)
  keep working. The policy is intentionally total: a pause is meant to
  freeze token state during an incident, not just block trading while other
  balance-changing admin actions continue.
- `contracts/asset-token/src/test.rs`: added tests for the paths that
  weren't previously covered on their own — `burn`, `approve`, and
  `transfer_from` while paused. (`transfer`, `mint` and `mint_batch` while
  paused were already covered by existing tests.)

## Why total rather than transfer-only

Before this change it was implicit, not explicit, that `pause` also blocked
mint/burn — the code already enforced it, but only `transfer` had a
dedicated test, so a regression in the mint/burn paths could have slipped
through. Making the policy total and documenting it removes the ambiguity
called out in the issue.

## Tests added

- `test_burn_blocked_when_paused`
- `test_approve_blocked_when_paused`
- `test_transfer_from_blocked_when_paused`

(`test_mint_blocked_when_paused` and `test_mint_batch_blocked_when_paused`
already existed in the suite.)
