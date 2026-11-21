#!/usr/bin/env bash
# Workflow 1: initialize the contracts and register/tokenize an asset.
# Usage: COMPLIANCE_ID=C... REGISTRY_ID=C... DIVIDEND_ID=C... ASSET_ID=C... ./examples/1-register-asset.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
. "$ROOT/examples/common.sh"

echo "Initializing compliance, registry, dividend and asset-token..."
invoke "$COMPLIANCE_ID" initialize --admin "$ADMIN_ADDR"
invoke "$REGISTRY_ID" initialize --admin "$ADMIN_ADDR"
invoke "$DIVIDEND_ID" initialize --admin "$ADMIN_ADDR"
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

# register_asset returns the numeric asset id (u64) used by the registry.
ASSET_INDEX="$(invoke "$REGISTRY_ID" register_asset \
  --issuer "$ADMIN_ADDR" \
  --token_contract "$ASSET_ID" \
  --name "Manhattan Loft" \
  --asset_type "real_estate" \
  --valuation 500000000)"
echo "Registered asset #$ASSET_INDEX"
