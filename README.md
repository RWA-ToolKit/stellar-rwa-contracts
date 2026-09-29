# Soroban Contracts

A collection of Soroban smart contracts for tokenized real-world assets, including an
asset token, compliance rules, dividend distribution, and a registry.

## Contracts

| Contract | Description |
| --- | --- |
| `asset-token` | Fungible asset token with transfer restrictions |
| `compliance` | Compliance rules and allow/deny lists |
| `dividend` | Dividend distribution to token holders |
| `registry` | Registry of assets and their metadata |

Every privileged (admin- or self-authorized) function across all four
contracts, the role it requires, and the error raised for the wrong caller
is documented in the [Authority Matrix](docs/AUTHORITY_MATRIX.md).

## How compliance gating works

```
transfer(from, to, amount)
  ├─ from.require_auth()
  ├─ assert !paused
  ├─ compliance.is_allowed(from)   ── cross-contract call ──►  compliance contract
  ├─ compliance.is_allowed(to)     ── cross-contract call ──►  compliance contract
  └─ move balances + emit event
```

## Building

```sh
make build
```

This runs `stellar contract build` and produces the wasm for each contract under
`target/wasm32v1-none/release/`.

## Interface specifications

The build also emits a machine-readable interface specification per contract. These
specs describe the public contract interface (functions, arguments, and return types)
and are published as a consumable artifact so downstream consumers do not have to
hand-maintain their own view of the contract interfaces.

```sh
make interface-specs
```

Specs are written to `target/interface-specs/`, one file per contract:

```
target/interface-specs/asset-token.json
target/interface-specs/compliance.json
target/interface-specs/dividend.json
target/interface-specs/registry.json
```

Each spec is generated from the contract's wasm using
`stellar contract info interface --wasm <wasm> --output json`, so it always reflects
the interface of the built contract. This requires the Stellar CLI to be installed.

### Consuming the specs

CI attaches the generated specs to every run as the `interface-specs` artifact. To
consume them:

1. Download the `interface-specs` artifact from the desired CI run (or run
   `make interface-specs` locally).
2. Generate bindings for the web app and API from the spec files instead of
   hand-maintaining the interface definitions, e.g.:

   ```sh
   stellar contract bindings typescript \
       --wasm target/wasm32v1-none/release/asset_token.wasm \
     --output-dir packages/asset-token-bindings
   ```

   The same approach works for the other contracts by pointing `--wasm` at the
   corresponding wasm file.
3. Check the generated bindings into the consuming project (or generate them as part
   of its build) so the web app and API stay in sync with the published interface.

## Testing

```sh
cargo test
```

Runs the full test suite with no network access.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for local Soroban setup. Please keep
`cargo fmt` clean — CI enforces it.

## Sister repos

- **Web app:** https://github.com/RWA-ToolKit/stellar-rwa-web
- **API + Docs:** https://github.com/RWA-ToolKit/stellar-rwa-api-docs

## License

MIT — see [LICENSE](LICENSE).

## Handsoff notes

<!-- handsoff-issue-390 -->
- #390: Test concurrent claims against the same distribution

<!-- handsoff-issue-400 -->
- #400: Add an integration test exercising all four contracts together

<!-- handsoff-issue-391 -->
- #391: Test empty and oversized string inputs
