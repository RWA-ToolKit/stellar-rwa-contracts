# Compliance Contract Swap Safety

## What was implemented

- `contracts/asset-token/src/lib.rs`: `set_compliance` already probed the
  new target for the `is_allowed` method (via `ComplianceInterface`) before
  accepting it — an address that doesn't implement the interface, or that
  rejects the admin, causes the call to fail instead of bricking every
  future transfer. Made that intent explicit with a doc comment.
- The `setcomp` event now publishes `(old_compliance, new_compliance)`
  instead of just the new address, so subscribers can see exactly what
  changed rather than inferring the previous value from a separate read.

## Why probe before committing state

The probe call happens before `meta.compliance_contract` is overwritten and
before the new metadata is stored, so a failing probe leaves the token
pointed at the previously-working compliance contract. This is what
prevents a misconfigured swap from bricking every subsequent transfer.

## Tests

- `test_set_compliance_rejects_non_conforming_target` — a plain address (no
  `is_allowed` implementation) is rejected.
- `test_set_compliance_rejects_contract_that_blocks_admin` (pre-existing) —
  a contract that does implement `is_allowed` but denies the admin is
  rejected with `Error::InvalidCompliance`.
- `test_set_compliance_emits_old_and_new_addresses` — a successful swap
  emits an event and updates metadata to the new address.
- `test_set_compliance_switches_gate` (pre-existing) — the happy path.
