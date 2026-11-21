# Glossary of RWA & Compliance Terms

This glossary defines terms **as they are used specifically in this codebase**. Several of these words have broader meanings in the RWA/securities-compliance world; the definitions below describe the precise behavior implemented by the contracts in this repository. Where a term maps to a specific contract function, the function is named so you can read the source directly.

> Terminology note: "allowlist" (one word) is used throughout this codebase. You may see "whitelist" in older external material; they refer to the same concept here.

---

## Allowlist

The set of addresses permitted to hold or receive a token. An address that is **not** on the allowlist is rejected by transfer and mint paths.

- `add_to_allowlist` — adds an address to the allowlist.
- `remove_from_allowlist` — removes an address from the allowlist (see the distinction from *suspend* below).
- `get_allowlist` — returns the current allowlist.

Being on the allowlist is necessary but not always sufficient: an address may also be subject to a *jurisdiction block* or a *minimum holding period*.

---

## Suspend / Reinstate / Remove

These three operations are **not** interchangeable. The distinction matters because a suspended address and a removed address behave differently and are recovered differently.

- **Suspend** (`suspend`) — places an address into the `ComplianceStatus::Suspended` state. The address is temporarily barred from activity but **remains recorded** in the compliance system. Suspension is intended to be reversible and preserves the address's history/status so it can be restored without re-onboarding.
- **Reinstate** (`reinstate`) — returns a `Suspended` address to good standing. This is the counterpart to `suspend`; it does not re-add an address that was *removed*.
- **Remove** (`remove_from_allowlist`) — takes an address off the allowlist entirely. This is a stronger action than suspension: the address is no longer an allowlisted participant and must go through `add_to_allowlist` again to participate.

In short: **suspend** is a reversible hold on a still-known participant; **remove** de-allowlists the participant. Use `reinstate` to undo a suspension, and `add_to_allowlist` to undo a removal.

The `ComplianceStatus::Suspended` state is the on-chain marker that distinguishes a suspended address from an active or removed one.

---

## Jurisdiction Block

A restriction keyed by jurisdiction (rather than by individual address) that prevents participants associated with that jurisdiction from transacting. Jurisdiction blocks are managed separately from the per-address allowlist.

- `block_jurisdiction` — blocks a jurisdiction.
- `unblock_jurisdiction` — lifts a jurisdiction block.
- `get_blocked_jurisdictions` — returns the currently blocked jurisdictions.

A jurisdiction block applies in addition to allowlisting: an allowlisted address in a blocked jurisdiction is still restricted.

---

## Minimum Holding Period

A required minimum time (measured in ledgers) that a holder must retain tokens after acquisition before they may transfer them. This is a common lock-up/vesting-style constraint in RWA instruments.

- `set_min_holding_period` — configures the minimum holding period.
- `record_acquisition` — records when a holder acquired tokens, starting the holding-period clock.
- `get_first_acquired_ledger` — returns the ledger at which a holder first acquired tokens, which is used to compute whether the minimum holding period has elapsed.

The holding period is evaluated per holder based on the first-acquired ledger recorded via `record_acquisition`.

---

## Valuation Guard

A safety mechanism that constrains how the reported value of a token/asset may change, guarding against erroneous or malicious valuation updates (for example, an implausibly large jump in reported value).

- Asset-token: `update_valuation` — the entry point for updating valuation, subject to the guard.
- Registry: `total_value_locked` — aggregates value across assets and reflects the guarded valuations.

For the full rationale and mechanics, see [`docs/design-notes/valuation-guard.md`](design-notes/valuation-guard.md).

---

## Pause / Guardian

An emergency-stop mechanism. When a token is **paused**, state-changing operations are halted until it is unpaused. The **guardian** is the privileged role authorized to perform these emergency actions.

- `pause` — halts token operations.
- `unpause` — resumes token operations.
- `set_guardian` — sets the guardian role authorized to pause/unpause.

Pause is distinct from the compliance controls above: it is a global operational stop, whereas allowlist, jurisdiction, and holding-period rules are per-participant compliance constraints. For the policy governing when pausing is appropriate and who may invoke it, see [`docs/design-notes/pause-policy.md`](design-notes/pause-policy.md).

---

## Quick Reference

| Term | Primary function(s) | Notes |
| --- | --- | --- |
| Allowlist | `add_to_allowlist`, `remove_from_allowlist`, `get_allowlist` | Required to hold/receive |
| Suspend | `suspend` | Reversible; sets `ComplianceStatus::Suspended` |
| Reinstate | `reinstate` | Undoes a suspension |
| Remove | `remove_from_allowlist` | Stronger than suspend; re-add to restore |
| Jurisdiction block | `block_jurisdiction`, `unblock_jurisdiction`, `get_blocked_jurisdictions` | Applies in addition to allowlist |
| Minimum holding period | `set_min_holding_period`, `record_acquisition`, `get_first_acquired_ledger` | Per-holder lock-up in ledgers |
| Valuation guard | `update_valuation`, `total_value_locked` | See `docs/design-notes/valuation-guard.md` |
| Pause / Guardian | `pause`, `unpause`, `set_guardian` | Global emergency stop; see `docs/design-notes/pause-policy.md` |