#!/usr/bin/env bash
#
# Deploy the Stellar RWA contracts to a Soroban network (Testnet by default)
# and initialize them into a working configuration.
#
# Usage:
#   ./scripts/deploy.sh                 # deploy to testnet with identity "rwa-admin"
#   NETWORK=testnet IDENTITY=rwa-admin ./scripts/deploy.sh
#
# Requirements: stellar CLI (>= 22), a funded identity on the target network.
# The script is idempotent about building; deployment always creates fresh
# contract instances and prints their ids.

set -euo pipefail

NETWORK="${NETWORK:-testnet}"
IDENTITY="${IDENTITY:-rwa-admin}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WASM_DIR="$ROOT/target/wasm32v1-none/release"

echo "==> Network:  $NETWORK"
echo "==> Identity: $IDENTITY"

# 1. Ensure the deploying identity exists and is funded (Testnet friendbot).
if ! stellar keys address "$IDENTITY" >/dev/null 2>&1; then
  echo "==> Generating and funding identity '$IDENTITY'..."
  stellar keys generate --network "$NETWORK" --fund "$IDENTITY"
fi
ADMIN_ADDR="$(stellar keys address "$IDENTITY")"
echo "==> Admin address: $ADMIN_ADDR"

# 2. Build all contracts to wasm.
echo "==> Building contracts..."
stellar contract build >/dev/null

deploy() {
  # deploy <wasm-name> -> echoes the contract id
  local wasm="$1"
  stellar contract deploy \
    --wasm "$WASM_DIR/${wasm}.wasm" \
    --source "$IDENTITY" \
    --network "$NETWORK"
}

invoke() {
  # invoke <contract-id> <fn> [args...]
  local id="$1"; shift
  stellar contract invoke \
    --id "$id" \
    --source "$IDENTITY" \
    --network "$NETWORK" \
    -- "$@"
}

echo "==> Deploying compliance..."
COMPLIANCE_ID="$(deploy compliance)"
echo "    compliance: $COMPLIANCE_ID"
invoke "$COMPLIANCE_ID" initialize --admin "$ADMIN_ADDR"

echo "==> Deploying registry..."
REGISTRY_ID="$(deploy registry)"
echo "    registry:   $REGISTRY_ID"
invoke "$REGISTRY_ID" initialize --admin "$ADMIN_ADDR"

echo "==> Deploying dividend..."
DIVIDEND_ID="$(deploy dividend)"
echo "    dividend:   $DIVIDEND_ID"
invoke "$DIVIDEND_ID" initialize --admin "$ADMIN_ADDR"

echo "==> Approving admin on compliance (so it can hold the sample asset)..."
invoke "$COMPLIANCE_ID" add_to_allowlist \
  --admin "$ADMIN_ADDR" --address "$ADMIN_ADDR" --jurisdiction "US" --expires_at 0

echo "==> Deploying asset-token (sample real-estate asset)..."
ASSET_ID="$(deploy asset_token)"
echo "    asset-token: $ASSET_ID"
invoke "$ASSET_ID" initialize \
  --admin "$ADMIN_ADDR" \
  --name "Manhattan Loft" \
  --symbol "MLOFT" \
  --asset_type "real_estate" \
  --total_supply 1000000 \
  --decimals 2 \
  --compliance_contract "$COMPLIANCE_ID" \
  --asset_description "A tokenized loft in Manhattan" \
  --valuation 500000000

echo "==> Registering the asset in the registry..."
invoke "$REGISTRY_ID" register_asset \
  --issuer "$ADMIN_ADDR" \
  --token_contract "$ASSET_ID" \
  --name "Manhattan Loft" \
  --asset_type "real_estate" \
  --valuation 500000000

cat <<EOF

============================================================
  Deployment complete on $NETWORK
============================================================
  Admin:       $ADMIN_ADDR
  compliance:  $COMPLIANCE_ID
  registry:    $REGISTRY_ID
  dividend:    $DIVIDEND_ID
  asset-token: $ASSET_ID
============================================================
Copy these into DEPLOYMENTS.md.
EOF
