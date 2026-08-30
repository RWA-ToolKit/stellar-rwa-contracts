# Changelog

All notable changes to the Stellar RWA contracts are documented here. The format
is based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added
- `upgrade(admin, new_wasm_hash)` on all four contracts, deploying new Wasm
  behind the existing contract id in place (`VERSION` bumped on each).
- `scripts/deploy.sh` accepts `COMPLIANCE_ID`/`REGISTRY_ID`/`DIVIDEND_ID`/
  `ASSET_ID` env vars to reuse existing contract ids instead of always
  deploying fresh instances, and a `--upgrade` flag to upgrade those ids in
  place via the new `upgrade` function.
- `scripts/generate_error_docs.py` regenerates each contract's `## Errors`
  table in `docs/` from its `#[contracterror]` enum, mirroring
  `scripts/generate_storage_docs.py` for `DataKey` (`make update-doc-errors`).
- `deny.toml` + a `deny` CI job (`cargo-deny`) checking dependency advisories,
  license compatibility, and banned/duplicate crates.
- `.github/PULL_REQUEST_TEMPLATE.md` with a contract-change checklist
  (tests, events, storage/`VERSION`, generated docs, changelog), referenced
  from `CONTRIBUTING.md`.

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
