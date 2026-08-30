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
# Set DRY_RUN=1 to print the commands that would run without executing them.
# Deploying to any network other than "testnet" requires typing "yes" at an
# interactive confirmation prompt (skip it non-interactively with CONFIRM=1),
# since a mistyped NETWORK could otherwise spend real funds unattended.

set -euo pipefail

NETWORK="${NETWORK:-testnet}"
IDENTITY="${IDENTITY:-rwa-admin}"
DRY_RUN="${DRY_RUN:-0}"
CONFIRM="${CONFIRM:-0}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WASM_DIR="$ROOT/target/wasm32v1-none/release"

echo "==> Network:  $NETWORK"
echo "==> Identity: $IDENTITY"
if [ "$DRY_RUN" = "1" ]; then
  echo "==> DRY_RUN=1: commands will be printed, not executed."
fi

# 0. Require explicit confirmation before touching any non-testnet network.
if [ "$DRY_RUN" != "1" ] && [ "$NETWORK" != "testnet" ]; then
  if [ "$CONFIRM" = "1" ]; then
    echo "==> CONFIRM=1: skipping interactive confirmation for network '$NETWORK'."
  else
    read -r -p "About to deploy and initialize contracts on '$NETWORK', which may spend real funds. Type 'yes' to continue: " reply
    if [ "$reply" != "yes" ]; then
      echo "Aborted."
      exit 1
    fi
  fi
fi

# run <description> -- <command...>
# In DRY_RUN mode, prints the command instead of executing it.
run() {
  local desc="$1"; shift
  if [ "$DRY_RUN" = "1" ]; then
    echo "DRY_RUN: $desc: $*"
  else
    "$@"
  fi
}

# 1. Ensure the deploying identity exists and is funded (Testnet friendbot).
if [ "$DRY_RUN" = "1" ]; then
  echo "DRY_RUN: would ensure identity '$IDENTITY' exists and is funded on $NETWORK"
  ADMIN_ADDR="<admin-address>"
else
  if ! stellar keys address "$IDENTITY" >/dev/null 2>&1; then
    echo "==> Generating and funding identity '$IDENTITY'..."
    stellar keys generate --network "$NETWORK" --fund "$IDENTITY"
  fi
  ADMIN_ADDR="$(stellar keys address "$IDENTITY")"
fi
echo "==> Admin address: $ADMIN_ADDR"

# 2. Build all contracts to wasm.
echo "==> Building contracts..."
run "build contracts" stellar contract build >/dev/null

deploy() {
  # deploy <wasm-name> -> echoes the contract id
  local wasm="$1"
  if [ "$DRY_RUN" = "1" ]; then
    echo "DRY_RUN: stellar contract deploy --wasm $WASM_DIR/${wasm}.wasm --source $IDENTITY --network $NETWORK" >&2
    echo "<${wasm}-contract-id>"
    return
  fi
  stellar contract deploy \
    --wasm "$WASM_DIR/${wasm}.wasm" \
    --source "$IDENTITY" \
    --network "$NETWORK"
}

invoke() {
  # invoke <contract-id> <fn> [args...]
  local id="$1"; shift
  if [ "$DRY_RUN" = "1" ]; then
    echo "DRY_RUN: stellar contract invoke --id $id --source $IDENTITY --network $NETWORK -- $*"
    return
  fi
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
