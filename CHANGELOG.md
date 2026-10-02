# Changelog

All notable changes to the Stellar RWA contracts are documented here. The format
is based on [Keep a Changelog](https://keepachangelog.com/).

Every change that touches contract source (`contracts/**`) must add an entry
under the `Unreleased` section below. See [CONTRIBUTING.md](CONTRIBUTING.md) for
the full convention; CI flags contract changes that do not update this file.

## [Unreleased]

### Added
- **asset-token**: `mint_batch` mints to multiple compliance-approved recipients in one call. Admin only; each `(recipient, amount)` pair is checked individually and the whole call reverts if any recipient fails compliance.
- **compliance**: `add_to_allowlist_batch` adds many addresses at once, with the same expiry check, jurisdiction normalisation, audit-trail capture and per-address `approved` event as `add_to_allowlist`. A failing entry reverts the call.
- **compliance**: `status_of` returns an address's stored KYC status, distinguishing "never seen" (`None`) from an approved record. Deprecated `Pending`/`Rejected` variants are never written and remain only for ABI stability.
- **registry**: `reactivate_asset` returns a deactivated asset to the active set in place, restoring it to TVL and `active_count` without re-registering it under a new id. A no-op if the asset is already active.
- **dividend**: `cancel_distribution` cancels a distribution and returns the escrowed funds to the issuer. Only possible while nothing has been claimed; admin only.
- All four contracts expose a `VERSION` constant.
- `scripts/deploy.sh` now sources a local `.env` when present while preserving explicit caller-provided values and prompting before a mainnet deployment.

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
