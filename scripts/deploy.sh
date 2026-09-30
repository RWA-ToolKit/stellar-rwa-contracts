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
#
# Note (issue #3): this script initializes the compliance contract and the
# asset-token contract with the same $ADMIN_ADDR. That is a convenience
# default for standing up a single-operator demo/testnet deployment in one
# shot — it is NOT a requirement of the contracts. The compliance admin and
# the asset-token admin are independent, unrelated pieces of state (see the
# module docs in contracts/compliance/src/lib.rs and
# contracts/asset-token/src/lib.rs), so a real issuer can run
# `invoke "$COMPLIANCE_ID" initialize --admin "$COMPLIANCE_OFFICER_ADDR"` with
# a separate compliance-officer address instead.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXPLICIT_NETWORK="${NETWORK:-}"
EXPLICIT_IDENTITY="${IDENTITY:-}"

if [[ -f "$ROOT/.env" ]]; then
  set -a
  . "$ROOT/.env"
  set +a
fi

NETWORK="${EXPLICIT_NETWORK:-${NETWORK:-testnet}}"
IDENTITY="${EXPLICIT_IDENTITY:-${IDENTITY:-rwa-admin}}"
WASM_DIR="$ROOT/target/wasm32v1-none/release"

echo "==> Network:  $NETWORK"
echo "==> Identity: $IDENTITY"

if [[ "$NETWORK" == "mainnet" ]]; then
  echo "==> WARNING: This deploy targets mainnet and will spend live fees."
  read -r -p "Type 'mainnet' to continue: " confirmation
  if [[ "$confirmation" != "mainnet" ]]; then
    echo "==> Aborting mainnet deployment without explicit confirmation."
    exit 1
  fi
fi

# 1. Ensure the deploying identity exists and is funded on non-mainnet test networks.
if ! stellar keys address "$IDENTITY" >/dev/null 2>&1; then
  if [[ "$NETWORK" == "mainnet" ]]; then
    echo "==> Identity '$IDENTITY' was not found locally; use an existing funded mainnet identity."
    exit 1
  fi
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
