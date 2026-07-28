# Changelog

All notable changes to the Stellar RWA contracts are documented here. The format
is based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Security
- **dividend**: shares are now computed from a holder snapshot recorded at
  distribution creation instead of the holder's live balance at claim time.
  Previously a holder could claim, transfer the tokens to another approved
  address, and claim again, draining the escrow (#5).
- **dividend**: claim markers, distributions and snapshot entries have their TTL
  extended on every write. An archived claim marker made `has_claimed` read
  `false` again and allowed a second claim (#7).

### Added
- **dividend**: `reclaim_unclaimed(admin, distribution_id)` sweeps leftover
  escrow back to the admin, either once every allocated share has been claimed
  or once the 14-day claim window has elapsed. Rounding dust is no longer locked
  in the contract forever (#6).
- **dividend**: `snapshot_balance(distribution_id, holder)` view, and
  `snapshot_supply` / `allocated` fields on `Distribution`.

### Changed
- **dividend**: `create_distribution` takes an additional
  `snapshot: Vec<SnapshotEntry>` argument. **Breaking API change.**
- **dividend**: `completed` is now reached when `distributed >= allocated`
  rather than `>= total_amount`, so a fully claimed distribution actually closes
  despite the rounding dust (#6).
- **dividend**: new errors `InvalidSnapshot (8)`, `ClaimWindowOpen (9)`,
  `NothingToReclaim (10)`, `DistributionClosed (11)`.

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
