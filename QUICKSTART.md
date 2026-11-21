# Quickstart: Clone to Working Asset

From a fresh clone to a fully deployed and exercised asset on testnet in
~10 minutes.

## Prerequisites

- **Rust** (stable) with the `wasm32v1-none` target:
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  rustup target add wasm32v1-none
  ```
- **Stellar CLI** (>= 26, matching the `soroban-sdk = "26"` pin in
  `Cargo.toml`; includes Soroban support):
  ```bash
  cargo install --locked stellar-cli
  stellar --version   # expect >= 26
  ```

> **Note:** this repo uses `wasm32v1-none` (not `wasm32-unknown-unknown`),
> and the Stellar CLI (not `soroban-cli`). Commands from older guides or
> tutorials that reference either of those will not work here.

## Steps

### 1. Clone and configure testnet

```bash
git clone https://github.com/RWA-ToolKit/stellar-rwa-contracts.git
cd stellar-rwa-contracts

# Register the Testnet network with the Stellar CLI
stellar network add testnet \
  --rpc-url https://soroban-testnet.stellar.org \
  --network-passphrase "Test SDF Network ; September 2015"

# Generate and fund a keypair (Friendbot covers the fee)
stellar keys generate rwa-admin --network testnet --fund
```

### 2. Deploy all contracts

The repository ships `scripts/deploy.sh`, which performs the full correct
sequence: compliance → registry → dividend → allowlist admin → asset-token →
register asset.

```bash
NETWORK=testnet IDENTITY=rwa-admin ./scripts/deploy.sh
```

`deploy.sh` builds the contracts with `stellar contract build` (which targets
`wasm32v1-none`) and deploys each one with `stellar contract deploy --wasm
<path> --source rwa-admin --network testnet`.

When it finishes it prints four contract ids. Copy them for the steps below,
or paste them into `DEPLOYMENTS.md`.

```
compliance:  C...
registry:    C...
dividend:    C...
asset-token: C...
```

### 3. Approve a second address (so it can receive tokens)

Every holder must be approved on the compliance contract before they can
receive or transfer the asset. The `rwa-admin` identity was automatically
allowlisted during deploy.

```bash
export COMPLIANCE_ID=<compliance-contract-id>
export ASSET_ID=<asset-token-contract-id>
ADMIN_ADDR="$(stellar keys address rwa-admin)"
ALICE_ADDR="<alice-public-key>"

stellar contract invoke \
  --id "$COMPLIANCE_ID" \
  --source rwa-admin \
  --network testnet \
  -- add_to_allowlist \
  --admin "$ADMIN_ADDR" \
  --address "$ALICE_ADDR" \
  --jurisdiction "US" \
  --expires_at 0
```

> **Note:** the `--` separator is required — everything after it is passed to
> the contract function, so `--admin`, `--address`, etc. are function
> arguments, not CLI flags.
```

### 4. Mint tokens to Alice

```bash
stellar contract invoke \
  --id "$ASSET_ID" \
  --source rwa-admin \
  --network testnet \
  -- mint \
  --admin "$ADMIN_ADDR" \
  --to "$ALICE_ADDR" \
  --amount 50000
```

### 5. Transfer tokens from Alice to admin

```bash
stellar contract invoke \
  --id "$ASSET_ID" \
  --source "$ALICE_ADDR" \
  --network testnet \
  -- transfer \
  --from "$ALICE_ADDR" \
  --to "$ADMIN_ADDR" \
  --amount 1000
```

> **Note:** `--source` takes a Stellar CLI identity name (or secret key), not
> a public key. To sign as Alice, import her secret with
> `stellar keys add alice --secret-key <alice-secret>` and use
> `--source alice`.

### 6. Hand the asset-token admin role to Alice

Admin changes are two-step: the current admin proposes a successor, and the
successor accepts. The role does not move until acceptance, and after this
step `rwa-admin` no longer administers the asset token.

```bash
stellar contract invoke \
  --id "$ASSET_ID" \
  --source rwa-admin \
  --network testnet \
  -- propose_admin \
  --admin "$ADMIN_ADDR" \
  --new_admin "$ALICE_ADDR"

stellar contract invoke \
  --id "$ASSET_ID" \
  --source alice \
  --network testnet \
  -- accept_admin \
  --new_admin "$ALICE_ADDR"
```

Done! Asset deployed, minted, transferred, and admin handed over on testnet.

## Keeping the quickstart accurate

A CI script (`scripts/check_quickstart.sh`) greps `QUICKSTART.md` for
`stellar contract invoke` calls and verifies that the function name passed
after the `--` separator (e.g. `add_to_allowlist`, `mint`, `transfer`,
`propose_admin`, `accept_admin`) matches a function that exists in the
corresponding contract's interface spec (under `target/interface-specs/`).
The CI job `check-quickstart-params` runs this script on every pull request,
so a drift between the docs and the actual contract signatures will fail CI
before it can be merged.

## Integrating from TypeScript

For wallets and dApps, use the generated Soroban TypeScript client instead of
raw CLI calls. A complete, runnable example (generate bindings for
`asset-token`, then call `balance` and `transfer` against a local network) is
in [`examples/typescript-client/`](examples/typescript-client/README.md).

See `docs/` for deeper per-contract guides.
