# Quickstart: Clone to Working Asset

From a fresh clone to a fully deployed and exercised asset on testnet in
~10 minutes.

## Prerequisites

- **Rust** (stable) with the `wasm32v1-none` target:
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  rustup target add wasm32v1-none
  ```
- **Stellar CLI** (>= 22, includes Soroban):
  ```bash
  cargo install --locked stellar-cli
  stellar --version   # expect >= 22
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
stellar keys generate --network testnet --fund rwa-admin
```

### 2. Deploy all contracts

The repository ships `scripts/deploy.sh`, which performs the full correct
sequence: compliance → registry → dividend → allowlist admin → asset-token →
register asset.

```bash
NETWORK=testnet IDENTITY=rwa-admin ./scripts/deploy.sh
```

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

Done! Asset deployed, minted, and transferred on testnet.

## Keeping the quickstart accurate

A CI script (`scripts/check_quickstart.sh`) greps `QUICKSTART.md` for
`stellar contract invoke` calls and verifies that every `--function-name`
argument matches a function that exists in the corresponding contract's
interface spec (under `target/interface-specs/`). The CI job
`check-quickstart-params` runs this script on every pull request, so a
drift between the docs and the actual contract signatures will fail CI
before it can be merged.

See `docs/` for deeper per-contract guides.
