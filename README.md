# Stellar RWA Contracts

Soroban smart contracts for tokenizing **real-world assets** (real estate,
invoices, commodities) on Stellar with built-in **compliance**: KYC allowlists,
transfer restrictions, jurisdiction rules, administrative pausing, and
proportional dividend distribution.

Real-world assets are represented as compliant tokens where **only verified
addresses** can hold or transfer them — the transfer gate is enforced on-chain
via a cross-contract call into the compliance contract.

## Contracts

| Contract | Purpose | Docs | Testnet |
|----------|---------|------|---------|
| **compliance** | KYC allowlist + jurisdiction rules; the transfer gate | [docs](docs/compliance.md) | `CBUERYDM7DXTZLLKDBRJKUBPFJ7M4OSUN4T7XKUARU345RLXNAIQD2IU` |
| **asset-token** | Compliant RWA token; transfers gated by compliance | [docs](docs/asset-token.md) | `CBMCWLSQSWUTLUJFCNBHNBSXMUM3XU7NAQ5TSNERW4HA4ZZBYHLG4ECZ` |
| **registry** | Index of all tokenized assets + TVL | [docs](docs/registry.md) | `CBX5SMLTXX6JP4HA5GQIO2V6QM7WCUGL2GZ6D4U773HMRI6RXISKPUR3` |
| **dividend** | Proportional yield/dividend distribution | [docs](docs/dividend.md) | `CAR4XY3CEBQWFOL27JEWFW34KXSIZA7RFKDQMEIV7ZU723RWY37I2SYX` |

Full addresses and the sample asset are in [DEPLOYMENTS.md](DEPLOYMENTS.md).

## How compliance gating works

```
transfer(from, to, amount)
  ├─ from.require_auth()
  ├─ assert !paused
  ├─ compliance.is_allowed(from)   ── cross-contract call ──►  compliance contract
  ├─ compliance.is_allowed(to)     ── cross-contract call ──►  compliance contract
  └─ move balances + emit event
```

If either party is not `Approved` (or is expired / suspended / in a blocked
jurisdiction), the transfer reverts. The asset token knows only the compliance
*interface* (`#[contractclient]`), so the concrete compliance contract can be
swapped with `set_compliance`.

## Tech stack

- Rust + [Soroban SDK](https://soroban.stellar.org) 26
- Cargo workspace, one member per contract
- 48 unit tests including the cross-contract compliance checks

## Quick start

```bash
# build all contracts to wasm
stellar contract build

# run the full test suite (48 tests)
cargo test

# deploy + initialize everything on Testnet
NETWORK=testnet IDENTITY=rwa-admin ./scripts/deploy.sh
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for local Soroban setup, the compliance
model, and how to add a new compliance rule.

## Sister repos

- **Web app:** https://github.com/RWA-ToolKit/stellar-rwa-web
- **API + Docs:** https://github.com/RWA-ToolKit/stellar-rwa-api-docs

## License

MIT — see [LICENSE](LICENSE).
