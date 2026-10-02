# Asset Token — SEP-41 Conformance Audit

This document tracks conformance of `contracts/asset-token/src/lib.rs` against
the [SEP-41](https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0041.md)
fungible token interface, as required for the web app's allowance-reading
distribution flow.

## Method-by-method audit

| SEP-41 method | Present | Signature match | Notes |
|---|---|---|---|
| `allowance(from, spender) -> i128` | Yes | Yes | Returns `0` once `expiration_ledger` has passed instead of a stale positive value. |
| `approve(from, spender, amount, expiration_ledger)` | Yes | Yes | Additionally rejected while `paused` (divergence, see below). |
| `balance(id) -> i128` | Yes | Yes | Matches spec. |
| `transfer(from, to, amount)` | Yes | Yes | Additionally gated on the compliance contract for both parties (divergence, see below). |
| `transfer_from(spender, from, to, amount)` | Yes | Yes | Same compliance gating as `transfer`. |
| `burn(from, amount)` | Yes | Yes | Matches spec. |
| `burn_from(spender, from, amount)` | No | — | Not implemented. The web app's distribution flow only reads `allowance`/`transfer_from`; this is a known gap, not audited further here. |
| `decimals() -> u32` | Partial | — | Exposed via `get_metadata().decimals` rather than a top-level `decimals()` fn. Divergence: metadata bundles decimals with other asset fields the web app already reads in one call. |
| `name() -> String` | Partial | — | Exposed via `get_metadata().name`, same reasoning as `decimals`. |
| `symbol() -> String` | Partial | — | Exposed via `get_metadata().symbol`, same reasoning as `decimals`. |

## Documented divergences

1. **Compliance gating on `transfer`/`transfer_from`.** SEP-41 does not
   define a compliance hook. This contract requires both the sender and
   recipient to pass `is_allowed` on the configured compliance contract
   before any balance moves. This is intentional: the asset represents a
   real-world asset subject to KYC/AML restrictions, and the spec's
   `transfer`/`transfer_from` signatures are otherwise preserved unchanged.
2. **`approve` rejected while paused.** The spec does not require this, but
   allowing new approvals during a pause would let `transfer_from` calls
   queue up and fire the instant the token is unpaused, defeating the
   purpose of pausing. `allowance` reads still work while paused.
3. **`name`/`symbol`/`decimals` are metadata fields, not top-level
   functions.** The web app already fetches `get_metadata()` once per asset;
   splitting these into separate calls would only add round trips.
4. **`burn_from` is not implemented.** No caller in this codebase currently
   needs delegated burning. Adding it is a follow-up if that changes.

## Test coverage

`contracts/asset-token/src/test.rs` covers:
- `approve` followed by `transfer_from` moving the approved amount.
- `transfer_from` rejected once the approved amount is exceeded.
- `transfer_from` rejected once `expiration_ledger` has passed (expiry).
- `allowance` reading back `0` for an expired approval.

## `AssetMetadata`

| Field                 | Type      | Meaning                              |
|-----------------------|-----------|--------------------------------------|
| `name` / `symbol`     | `String`  | Display name and ticker              |
| `asset_type`          | `String`  | `real_estate`, `invoice`, `commodity`|
| `total_supply`        | `i128`    | Current supply (base units)          |
| `decimals`            | `u32`     | Token decimals                       |
| `admin`               | `Address` | Controls mint/pause/valuation        |
| `compliance_contract` | `Address` | Gate consulted on transfer/mint      |
| `asset_description`   | `String`  | Free-text description                |
| `valuation`           | `i128`    | Asset value in **USD cents**         |
| `paused`              | `bool`    | When true, transfers/mints revert    |

## Functions

- `initialize(admin, name, symbol, asset_type, total_supply, decimals, compliance_contract, asset_description, valuation)` —
  stores metadata and mints `total_supply` to `admin`. The admin must already be
  compliance-approved. Admin auth. Once only.
- `transfer(from, to, amount)` — `from` auth; not paused; both parties compliant;
  `from` has balance; moves tokens.
- `transfer_batch(from, transfers: Vec<(Address, i128)>)` — the batch
  counterpart to `transfer`, following the same pattern as `mint_batch`.
  `from` auth; not paused; `from` compliant; then each `(recipient, amount)`
  pair is validated in turn. **Atomic (all-or-nothing):** if any entry fails
  compliance, has a non-positive amount, or exceeds the sender's remaining
  balance, the entire call reverts and no balance moves — there is no
  partial-transfer outcome, so callers wanting best-effort behaviour must
  filter the list themselves. The sender's auth, the pause check and the
  sender-side compliance check each run once, before the loop, since none
  depends on the batch contents. Balances are re-read per entry, so entries
  spending the same balance accumulate correctly, a repeated recipient
  accumulates, and a self-transfer (`from == to`) is a no-op that emits its
  event without moving balances. One `transfer` event per entry. Like
  `mint_batch` there is no cap on the number of entries, and each entry
  costs one cross-contract compliance call.
- `mint(admin, to, amount)` — admin auth; not paused; `to` compliant; increases
  supply.
- `mint_batch(admin, recipients: Vec<(Address, i128)>)` — admin auth; not
  paused; mints each `(recipient, amount)` pair. **Atomic (all-or-nothing):**
  if any recipient in the vector fails compliance, has an invalid amount, or
  would overflow total supply, the entire call reverts — no recipient
  processed earlier in the vector keeps a credited balance, and total supply
  is unchanged. There is no partial-mint outcome; callers who want a
  best-effort mint must filter `recipients` against the compliance contract
  themselves before calling.
- `burn(from, amount)` — `from` auth; reduces caller balance and supply.
- `balance(id) -> i128`
- `total_supply() -> i128`
- `pause(caller)` — admin auth, or the optional guardian's; `unpause(admin)` — admin auth only.
- `get_metadata() -> AssetMetadata`
- `update_valuation(admin, new_valuation)` — admin auth.
- `set_compliance(admin, compliance)` — admin auth; repoints the gate.
- `propose_admin(admin, new_admin)` — admin auth; records a pending successor.
  The role does not move yet.
- `accept_admin(new_admin)` — pending successor's auth; completes the handover.
- `cancel_admin_proposal(admin)` — admin auth; clears the pending successor.
  See [issue #4](fixes/issue-4.md) for the rationale.

## Swapping compliance mid-life

`set_compliance` repoints the gate at a different contract. The token stores only
the compliance *address*; it does not snapshot or migrate any approval state.
Because approvals live in the compliance contract, not in the token, the set of
addresses that pass `is_allowed` is entirely determined by whichever contract is
currently referenced. Repointing the gate therefore **silently changes who can
transact**:

- Addresses approved under the old contract may not be approved under the new
  one. Their existing balances remain, but their `transfer`/`mint` calls will
  start reverting with `SenderNotCompliant` (#7) or `RecipientNotCompliant` (#8).
- Addresses that were *not* approved under the old contract may become approved
  under the new one, gaining the ability to receive or move the asset.
- The change takes effect immediately for the next `transfer`/`mint`; there is no
  grace period and no per-address migration. `burn` is unaffected (it does not
  consult compliance).
- The swap is not reversible in terms of state: repointing back to the old
  contract restores the old approval set only if that contract's state is
  unchanged.

### Recommended migration procedure

1. **Stage the new compliance contract** and populate it with the intended
   approval set (KYC/allow-list) before touching the token.
2. **Diff the approval sets** off-chain: compute the addresses approved under the
   old contract and under the new one, and identify addresses that would lose
   approval.
3. **Notify affected holders** and complete any required re-approval (KYC) so
   they are approved under the new contract *before* the swap.
4. **Pause the token** (`pause`) to halt transfers/mints while the gate is being
   changed, avoiding a window where some holders are unexpectedly blocked.
5. **Call `set_compliance(admin, new)`** (admin auth). This emits `setcomp`.
6. **Verify** by checking `get_metadata().compliance_contract` and probing a few
   known addresses with the new contract's `is_allowed`.
7. **Unpause** (`unpause`) once the new gate is confirmed correct.

Keep the old compliance contract deployed and unchanged until the migration is
confirmed, so the swap can be rolled back by repointing to it if needed.

## Errors

| Code | Name                   | Cause                                |
|------|------------------------|--------------------------------------|
| 1    | AlreadyInitialized     | double init                          |
| 2    | NotInitialized         | used before init                     |
| 3    | Unauthorized           | non-admin admin-only call            |
| 4    | InsufficientBalance    | transfer/burn over balance           |
| 5    | InvalidAmount          | amount <= 0 (or negative supply/val) |
| 6    | Paused                 | transfer/mint while paused           |
| 7    | SenderNotCompliant     | sender fails `is_allowed`            |
| 8    | RecipientNotCompliant  | recipient fails `is_allowed`         |
| 9    | Overflow               | supply overflow on mint              |
| 10   | InvalidInput           | invalid argument                     |
| 11   | InvalidCompliance      | compliance contract rejects the admin |
| 12   | NoPendingAdmin         | `accept_admin`/`cancel_admin_proposal` with nothing pending |
| 13   | InsufficientAllowance  | `transfer_from` over allowance / expired |
| 14   | ValuationChangeTooLarge | single `update_valuation` moves value beyond the per-update cap |

## Events

| Topic       | Data                    | When         |
|-------------|-------------------------|--------------|
| `mint`      | (to) → amount           | mint / init  |
| `transfer`  | (from, to) → amount     | transfer     |
| `burn`      | (from) → amount         | burn         |
| `pause`     | admin                   | pause        |
| `unpause`   | admin                   | unpause      |
| `valuation` | new valuation           | valuation up |
| `setcomp`   | compliance address      | gate changed |
| `set_admin` | (old_admin) → new_admin | admin handed over |

## Storage / TTL

Listing of the contract `DataKey` variants and their storage behaviour.

| Key | Payload | Storage | TTL / Notes |
|-----|---------|---------|-------------|
| `Metadata` | - | instance | - |
| `Balance` | Address | unknown | - |

## Security considerations

- Compliance is enforced **inside** `transfer`/`mint`; it cannot be bypassed by
  calling the token directly.
- Amounts must be strictly positive; zero/negative amounts revert.
- `mint` overflow is checked; supply cannot wrap.
- Only the admin can pause, mint, change valuation, or repoint compliance.
- Repointing compliance with `set_compliance` changes the effective approval set
  immediately; see "Swapping compliance mid-life" above for the operational
  consequences and the recommended migration procedure.

## Failure mode: unreachable compliance contract (issue #305)

`transfer`, `mint`, `mint_batch`, `burn`, and `set_compliance` all call into
`compliance_contract` via the generated `ComplianceClient`. This is a real
cross-contract call, not a local check, so it inherits the failure modes of
any Soroban invocation:

- **No contract deployed at that address** — the host cannot resolve the
  call and **traps**, aborting the entire transaction. No balance, supply, or
  metadata change is applied.
- **The callee traps internally** (e.g. it panics on unexpected input) —
  same result: the trap propagates up, the whole transaction rolls back.
- **The callee returns a value but not `bool`** — this cannot happen without
  bypassing the SDK's type-checked client; if it somehow did, decoding would
  itself trap.

This is the deliberate, and only sane, behavior: the token contract has no
way to distinguish "compliance said no" from "compliance is broken," so it
treats an unreachable or malfunctioning gate as a hard failure rather than
either failing open (allowing the transfer) or silently no-opping. Operators
must ensure `compliance_contract` always points at a live, correctly
implemented contract; `set_compliance` mitigates this somewhat by calling the
new gate before switching to it, but does not protect against the gate later
being removed or bricked.

Proven by test: `test_gate_traps_when_compliance_address_has_no_contract` and
`test_transfer_traps_when_compliance_contract_is_unreachable` in
`contracts/asset-token/src/test.rs`.

## Edge-case policy decisions

These behaviours were previously implicit/accidental; they are now
deliberate and pinned by tests in `contracts/asset-token/src/test.rs`.

| Case | Decision | Rationale | Test |
|------|----------|-----------|------|
| Zero-amount `transfer`/`mint`/`burn` | **Rejected** with `InvalidAmount` (#5) | A no-op call that still emits an event and costs fees is misleading; callers must skip the call instead. | `test_zero_amount_rejected` |
| Self-transfer (`from == to`) | **Allowed**, short-circuited to a true no-op (balances untouched, event still emitted) | Rejecting it forces callers to special-case an address match themselves; a no-op is safe and simpler, and avoids a double-apply bug in naive debit/credit code. | `test_self_transfer_no_inflation`, `test_self_transfer_exceeding_balance_fails`, `test_self_transfer_by_suspended_holder_fails` |
| Burn by a suspended/non-compliant holder | **Rejected** with `SenderNotCompliant` (#7) | `burn` still mutates balance and total supply, so it is gated exactly like the `from` side of a `transfer`; suspension cannot be bypassed via self-burn. | `test_burn_blocked_when_holder_not_compliant`, `test_burn_blocked_when_holder_suspended` |
| Mint to a non-compliant recipient | **Rejected** with `RecipientNotCompliant` (#8) | Minting is the only way new supply enters circulation; leaving it ungated would let tokens reach an address no `transfer` could ever reach. `mint_batch` applies the same check per recipient. | `test_mint_to_noncompliant_fails`, `test_mint_batch_reverts_entirely_on_noncompliant_recipient` |
