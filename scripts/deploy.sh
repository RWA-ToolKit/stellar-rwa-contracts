#!/usr/bin/env bash
#
# Deploy the Stellar RWA contracts to a Soroban network (Testnet by default)
# and initialize them into a working configuration.
#
# Usage:
#   ./scripts/deploy.sh                 # deploy to testnet with identity "rwa-admin"
#   NETWORK=testnet IDENTITY=rwa-admin ./scripts/deploy.sh
#
#   # Reuse existing contract ids instead of deploying fresh instances, e.g.
#   # to point a new asset-token at an already-deployed compliance contract,
#   # or to re-run after a partial failure without orphaning what already
#   # succeeded. Copy ids from DEPLOYMENTS.md. Any id left unset is still
#   # deployed (and initialized) fresh, as before. A reused id is left
#   # exactly as-is: not re-initialized, and (for asset-token) not
#   # re-registered in the registry.
#   COMPLIANCE_ID=C... REGISTRY_ID=C... DIVIDEND_ID=C... ./scripts/deploy.sh
#
#   # Upgrade the Wasm behind existing ids in place (same contract id, same
#   # storage) via each contract's own `upgrade(admin, new_wasm_hash)`,
#   # instead of deploying new instances. Every id you want upgraded must be
#   # passed; anything left unset is left untouched.
#   COMPLIANCE_ID=C... ASSET_ID=C... ./scripts/deploy.sh --upgrade
#
# Requirements: stellar CLI (>= 22), a funded identity on the target network.
# The script is idempotent about building.

set -euo pipefail

NETWORK="${NETWORK:-testnet}"
IDENTITY="${IDENTITY:-rwa-admin}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WASM_DIR="$ROOT/target/wasm32v1-none/release"

UPGRADE=0
for arg in "$@"; do
  case "$arg" in
    --upgrade) UPGRADE=1 ;;
    -h|--help)
      sed -n '2,26p' "${BASH_SOURCE[0]}"
      exit 0
      ;;
    *)
      echo "Unknown argument: $arg" >&2
      exit 1
      ;;
  esac
done

# Existing ids to reuse or (with --upgrade) upgrade in place. Left unset,
# each contract is deployed fresh and initialized as a new instance.
COMPLIANCE_ID="${COMPLIANCE_ID:-}"
REGISTRY_ID="${REGISTRY_ID:-}"
DIVIDEND_ID="${DIVIDEND_ID:-}"
ASSET_ID="${ASSET_ID:-}"

echo "==> Network:  $NETWORK"
echo "==> Identity: $IDENTITY"
[ "$UPGRADE" = 1 ] && echo "==> Mode:     upgrade (reusing ids, deploying new Wasm in place)"

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
  # deploy <wasm-name> -> echoes the new contract id
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

upgrade_in_place() {
  # upgrade_in_place <contract-id> <wasm-name> -> installs the new wasm and
  # invokes the contract's own `upgrade`, so the id and storage are kept.
  local id="$1" wasm="$2"
  local hash
  hash="$(stellar contract install \
    --wasm "$WASM_DIR/${wasm}.wasm" \
    --source "$IDENTITY" \
    --network "$NETWORK")"
  invoke "$id" upgrade --admin "$ADMIN_ADDR" --new_wasm_hash "$hash"
}

# deploy_or_reuse <id-var> <wasm-name> <label>
#   - id-var already set, --upgrade not passed : reuse as-is.
#   - id-var already set, --upgrade passed     : upgrade that id in place.
#   - id-var unset                             : deploy a fresh instance.
# Leaves the resulting id in <id-var> and whether it was freshly deployed
# (and therefore still needs `initialize`) in `<id-var>_FRESH` (0/1).
deploy_or_reuse() {
  local id_var="$1" wasm="$2" label="$3"
  local current="${!id_var}"
  if [ -n "$current" ]; then
    if [ "$UPGRADE" = 1 ]; then
      echo "==> Upgrading $label ($current)..."
      upgrade_in_place "$current" "$wasm"
    else
      echo "==> Reusing $label: $current"
    fi
    printf -v "${id_var}_FRESH" '0'
  else
    if [ "$UPGRADE" = 1 ]; then
      echo "error: --upgrade requires an existing id for $label (set ${id_var})" >&2
      exit 1
    fi
    echo "==> Deploying $label..."
    printf -v "$id_var" '%s' "$(deploy "$wasm")"
    echo "    $label: ${!id_var}"
    printf -v "${id_var}_FRESH" '1'
  fi
}

deploy_or_reuse COMPLIANCE_ID compliance "compliance"
[ "$COMPLIANCE_ID_FRESH" = 1 ] && invoke "$COMPLIANCE_ID" initialize --admin "$ADMIN_ADDR"

deploy_or_reuse REGISTRY_ID registry "registry"
[ "$REGISTRY_ID_FRESH" = 1 ] && invoke "$REGISTRY_ID" initialize --admin "$ADMIN_ADDR"

deploy_or_reuse DIVIDEND_ID dividend "dividend"
[ "$DIVIDEND_ID_FRESH" = 1 ] && invoke "$DIVIDEND_ID" initialize --admin "$ADMIN_ADDR"

# The sample asset-token below mints its initial supply to the admin, which
# requires the admin to already pass compliance. Only needed when we're about
# to freshly initialize an asset-token; a reused/upgraded one is presumably
# already set up.
if [ -z "$ASSET_ID" ] && [ "$UPGRADE" = 0 ]; then
  echo "==> Approving admin on compliance (so it can hold the sample asset)..."
  invoke "$COMPLIANCE_ID" add_to_allowlist \
    --admin "$ADMIN_ADDR" --address "$ADMIN_ADDR" --jurisdiction "US" --expires_at 0
fi

deploy_or_reuse ASSET_ID asset_token "asset-token (sample real-estate asset)"
if [ "$ASSET_ID_FRESH" = 1 ]; then
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
fi

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
