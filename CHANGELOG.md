# Changelog

All notable changes to the Stellar RWA contracts are documented here. The format
is based on [Keep a Changelog](https://keepachangelog.com/).

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
