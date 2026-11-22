# Changelog

All notable changes to the Stellar RWA contracts are documented here. The format
is based on [Keep a Changelog](https://keepachangelog.com/).

Every change that touches contract source (`contracts/**`) must add an entry
under the `Unreleased` section below. See [CONTRIBUTING.md](CONTRIBUTING.md) for
the full convention; CI flags contract changes that do not update this file.

## [Unreleased]

### Added
- **asset-token**: `snapshot` (admin), `balance_at`, `total_supply_at` and `current_snapshot_id`: point-in-time balances recorded lazily on first change after a snapshot; new `Error::SnapshotNotFound` (#16) (#728).
- **dividend**: `create_distribution_snapshot` sizes a distribution on an asset-token snapshot instead of a caller-supplied list; documented in docs/dividend.md (#728).
- **ci**: allowed-to-fail `sdk-next.yml` tests the workspace against the newest 26.x, the newest stable and the newest pre-release soroban-sdk weekly (#713).
- **contributing**: optional pre-commit hook (`scripts/git-hooks/pre-commit`, `.pre-commit-config.yaml`) running `cargo fmt --check` and clippy (#711).

### Documentation
- **compliance**: `prune_expired` documents a recommended `max_records` of 10–20: when every record has expired, 23 is the most that fits the 50-entry write limit, and `0` fails beyond ~23 expired records (#727).

### Fixed
- **build**: restored the workspace build and CI, which had been red on `main`: Cargo.lock held `ed25519-dalek` 3.0 (Dependabot #790) against the documented `=2.2` pin, breaking `soroban-env-host`; `fuzz` required `arbitrary` 1.4, which soroban-sdk 26 (`~1.3.0`) cannot resolve; `rwa-common::check_str` named a nonexistent `soroban_sdk::contracterror::ContractError` bound (now `Into<soroban_sdk::Error>`); test files used pre-SDK-26 event and `try_` result APIs, had an unclosed test and a duplicate test name, and were not `cargo fmt` clean. Tests for `update_valuations_batch`, `withdraw_unclaimed` and `suspend_batch`/`reinstate_batch`/`remove_batch` were removed because those functions are not in the contracts (the #735 entry below describes an unmerged change).
- **build**: `dividend` and `registry` link as Wasm again: `AssetMetadata` moved to `rwa-common` (re-exported by `asset-token`, same XDR) so they no longer link asset-token's exported functions (duplicate symbols); CI's Wasm build excludes the std-only `rwa-fuzz` crate.
- **registry**: `deactivate_asset`/`reactivate_asset` called by a non-admin now fail with `Error::Unauthorized` as documented when the asset's token is not an asset-token contract, instead of a host storage error.

### Changed
- **ci**: add a strict workspace Clippy gate; retain Soroban SDK deprecation warnings for ABI-compatible events and status variants until they can be migrated (#705).
- **registry**: `MAX_PAGE_SIZE` lowered from 100 to 50 (#716). A page of 100 exceeded Soroban's 100-entry footprint limit (instance and code count too), so a full page could never be served. `get_all_assets` and the paginated getters still clamp silently; a resource-cost test pins that an oversized `limit` costs one capped page.
- **ci**: Dependabot ignores the pinned `ed25519-dalek` and `arbitrary` (#710); new `authority-matrix` job checks `docs/AUTHORITY_MATRIX.md` table formatting (#714); README badges for CI, license, SDK and good first issues (#715).

### Changed
- **compliance**: added `AllowlistEntry::new` / `with_expires_at` Rust helpers with the documented `expires_at = 0` (permanent) default; no ABI or behavior change (#742).
- **asset-token**: moved `panic_err` and `check_str` helpers into a dedicated `internal` module; no behavior change (#769).
- **compliance**: split `lib.rs` (1491 lines) into `allowlist`, `jurisdiction`, `admin`, and `internal` submodules; no behavior change (#767).
- **docs**: added inline illustrative usage examples to `transfer_batch`, `add_to_allowlist_batch`, `create_distribution`, and `register_asset` doc comments (#749).
- **contributing**: added concrete bug-fix and feature CHANGELOG entry examples to CONTRIBUTING.md (#750).

### Added
- **registry**: `update_valuations_batch` updates valuations for many assets in one admin call, with per-entry validation, one `valuation` event per asset, and all-or-nothing TVL updates (#735).
- **registry**: define and test zero-sized `get_assets_by_type_page` requests as empty results (#557).
- **admin handover**: all four contracts now test that only the proposed successor can accept, cancellation clears the pending address, and a new proposal replaces the previous nominee; added an end-to-end CLI walkthrough.
- **asset-token**: `mint_batch` mints to multiple compliance-approved recipients in one call. Admin only; each `(recipient, amount)` pair is checked individually and the whole call reverts if any recipient fails compliance.
- **compliance**: `add_to_allowlist_batch` adds many addresses at once, with the same expiry check, jurisdiction normalisation, audit-trail capture and per-address `approved` event as `add_to_allowlist`. A failing entry reverts the call.
- **compliance**: `suspend_batch`, `reinstate_batch`, and `remove_batch` apply the corresponding single-address operation to a `Vec<Address>` in one transaction, keeping the same admin auth, status checks, and per-address events (#729). A failing entry reverts the whole batch.
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
- **asset-token**: `panic_err` now publishes `("panic_dbg", <offending value>)` diagnostic events under the `release-with-logs` profile only; standard release wasm is unchanged (#740).
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
