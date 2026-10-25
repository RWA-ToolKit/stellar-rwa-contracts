# Changelog

All notable changes to the Stellar RWA contracts are documented here. The format
is based on [Keep a Changelog](https://keepachangelog.com/).

Every change that touches contract source (`contracts/**`) must add an entry
under the `Unreleased` section below. See [CONTRIBUTING.md](CONTRIBUTING.md) for
the full convention; CI flags contract changes that do not update this file.

## [Unreleased]

### Added
- **registry**: define and test zero-sized `get_assets_by_type_page` requests as empty results (#557).
- **admin handover**: all four contracts now test that only the proposed successor can accept, cancellation clears the pending address, and a new proposal replaces the previous nominee; added an end-to-end CLI walkthrough.
- **asset-token**: `mint_batch` mints to multiple compliance-approved recipients in one call. Admin only; each `(recipient, amount)` pair is checked individually and the whole call reverts if any recipient fails compliance.
- **compliance**: `add_to_allowlist_batch` adds many addresses at once, with the same expiry check, jurisdiction normalisation, audit-trail capture and per-address `approved` event as `add_to_allowlist`. A failing entry reverts the call.
- **compliance**: `status_of` returns an address's stored KYC status, distinguishing "never seen" (`None`) from an approved record. Deprecated `Pending`/`Rejected` variants are never written and remain only for ABI stability.
- **compliance**: `reinstate` flips a `Suspended` record back to `Approved` without resupplying or overwriting `jurisdiction`, `verified_at`, or `expires_at`. Admin only; rejects non-suspended addresses with `NotSuspended (#8)`. Does not restart the minimum holding-period clock.
- **compliance**: `get_allowlist_count` returns the current number of allowlist entries in O(1) via a maintained counter. Includes suspended addresses; unaffected by jurisdiction blocks or expiry. Returns `0` on a fresh contract.
- **compliance**: `get_allowlist_page` pages through the allowlist by `offset` and `limit` without transferring the whole list. `limit` is clamped to `MAX_ALLOWLIST_PAGE_SIZE` (200); passing `0` or an oversized value returns up to the maximum. Returns an empty `Vec` past the end of the list.
- **compliance**: `propose_admin` / `accept_admin` / `cancel_admin_proposal` implement a two-step admin handover so a mistyped successor address cannot permanently brick administration. `accept_admin` is called by the proposed successor; `cancel_admin_proposal` is called by the current admin. `get_pending_admin` exposes the in-flight nominee.
- **compliance**: `block_jurisdiction` / `unblock_jurisdiction` / `get_blocked_jurisdictions` manage the blocked-jurisdiction set. `get_blocked_jurisdictions` returns an ordered list (insertion order) so the full set can be read directly rather than inferred off-chain.
- **compliance**: `set_min_holding_period` / `record_acquisition` / `get_min_holding_period` / `get_first_acquired_ledger` implement per-address lock-up windows. The clock starts at the ledger recorded by `record_acquisition` (write-once; top-ups do not restart it). `is_allowed` fails-open if no acquisition ledger has been recorded, preserving backwards compatibility for existing holders.
- **compliance**: `prune_expired` removes lapsed KYC records from storage and allowlist pages. `max_records = 0` means unbounded; any positive value bounds a single call so large lists can be pruned incrementally. Returns the count of entries left unexamined.
- **registry**: `reactivate_asset` returns a deactivated asset to the active set in place, restoring it to TVL and `active_count` without re-registering it under a new id. A no-op if the asset is already active.
- **registry**: added a CPU and memory cost baseline test for `reactivate_asset` (#564).
- **dividend**: `cancel_distribution` cancels a distribution and returns the escrowed funds to the issuer. Only possible while nothing has been claimed; admin only.
- All four contracts expose a `VERSION` constant.
- `scripts/deploy.sh` now sources a local `.env` when present while preserving explicit caller-provided values and prompting before a mainnet deployment.
- **compliance** (tests): `test_add_to_allowlist_batch_resource_cost_baseline` measures and records CPU-instruction and memory-byte cost for `add_to_allowlist_batch` at batch sizes 1, 10, and 50; asserts costs remain within Soroban per-transaction limits and scale monotonically with batch size.
- **compliance** (tests): `test_reinstate_with_zero_expiry_sentinel_is_deliberate` pins the documented behaviour that `reinstate` preserves the `expires_at = 0` (never-expires) sentinel and that `is_allowed` returns `true` immediately after reinstating such a record.
- **compliance** (tests): `test_reinstate_unauthorized_caller_rejected` establishes that `reinstate` is admin-only and rejects any unprivileged caller with `Unauthorized (#5)`; documents that this is intentional and not an open entry point.

### Changed
- **registry**: `get_all_assets` is now paginated via `start_id` and `limit`, returning ids `[start_id, start_id + limit)` and bounding per-call cost. `limit` is clamped to `MAX_PAGE_SIZE`; registries small enough to request the whole set in one call keep working.
- `ComplianceStatus::Pending` and `ComplianceStatus::Rejected` are explicitly documented as deprecated ABI-only values retained for compatibility.
- Asset-scoped dividend and registry operations now allow either the contract admin or the asset's admin to act.

### Fixed
- Registry asset-name validation now enforces the same 64-byte cap as the asset-token metadata checks and documents the UTF-8 byte-based rule.
- `deploy.sh` no longer tries to friendbot on `mainnet` and avoids silently ignoring a checked-in `.env` file.

## [0.1.0] - 2026-07-08

### Added
- **compliance** contract: KYC allowlist, per-address records with jurisdiction
  and ledger-based expiry, suspend/remove, jurisdiction blocking, and the total
  `is_allowed` gate.
- **asset-token** contract: compliant RWA token whose `transfer` and `mint`
  enforce compliance on both parties via cross-contract calls; mint/burn/pause,
  valuation updates, and swappable compliance contract.
- **registry** contract: index of tokenized assets with lookups by
  id/issuer/type, deactivation, and total value locked.
- **dividend** contract: proportional dividend distribution with escrow and
  one-claim-per-holder enforcement.
- 48 unit tests across the four contracts, including the cross-contract
  compliance and proportional-claim paths.
- `scripts/deploy.sh` for Testnet build + deploy + init.
- Per-contract docs, README, CONTRIBUTING, MIT license, CI, and Makefile.
- All four contracts deployed and initialized on Stellar Testnet
  (see `DEPLOYMENTS.md`).
