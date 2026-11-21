#!/usr/bin/env bash
# Shared configuration and helper for the examples. Source this from each script.

NETWORK="${NETWORK:-testnet}"
IDENTITY="${IDENTITY:-rwa-admin}"

: "${COMPLIANCE_ID:?Set COMPLIANCE_ID to the deployed compliance contract id}"
: "${REGISTRY_ID:?Set REGISTRY_ID to the deployed registry contract id}"
: "${DIVIDEND_ID:?Set DIVIDEND_ID to the deployed dividend contract id}"
: "${ASSET_ID:?Set ASSET_ID to the deployed asset-token contract id}"

ADMIN_ADDR="$(stellar keys address "$IDENTITY")"

# invoke <contract-id> <function> [args...]
invoke() {
  local id="$1"; shift
  stellar contract invoke \
    --id "$id" \
    --source "$IDENTITY" \
    --network "$NETWORK" \
    -- "$@"
}
