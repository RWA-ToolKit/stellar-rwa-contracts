#!/usr/bin/env bash
# Workflow 4: create a dividend distribution and let a holder claim it.
# Usage: PAYMENT_TOKEN_ID=C... INVESTOR_IDENTITY=alice INVESTOR_ADDR=G... ./examples/4-distribute-and-claim.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
. "$ROOT/examples/common.sh"

: "${PAYMENT_TOKEN_ID:?Set PAYMENT_TOKEN_ID to a deployed SAC/SEP-41 payment token}"
: "${INVESTOR_IDENTITY:?Set INVESTOR_IDENTITY to a local stellar identity, e.g. alice}"
: "${INVESTOR_ADDR:?Set INVESTOR_ADDR to the public key of that identity (G...)}"
TOTAL_AMOUNT="${TOTAL_AMOUNT:-100000}"
# Vec<(Address, i128)> snapshot: balances here must mirror the asset token.
ELIGIBLE="$(printf '[["%s","%s"],["%s","%s"]]' \
  "$INVESTOR_ADDR" "$TOTAL_AMOUNT" "$ADMIN_ADDR" "$TOTAL_AMOUNT")"

echo "Creating distribution..."
DIST_ID="$(invoke "$DIVIDEND_ID" create_distribution \
  --admin "$ADMIN_ADDR" \
  --asset_token "$ASSET_ID" \
  --payment_token "$PAYMENT_TOKEN_ID" \
  --total_amount "$TOTAL_AMOUNT" \
  --eligible "$ELIGIBLE")"
echo "Distribution id: $DIST_ID"

echo "Claimable: $(invoke "$DIVIDEND_ID" claimable --distribution_id "$DIST_ID" --holder "$INVESTOR_ADDR")"

# The holder signs their own claim.
stellar contract invoke \
  --id "$DIVIDEND_ID" \
  --source "$INVESTOR_IDENTITY" \
  --network "$NETWORK" \
  -- claim --distribution_id "$DIST_ID" --holder "$INVESTOR_ADDR"

echo "has_claimed: $(invoke "$DIVIDEND_ID" has_claimed --distribution_id "$DIST_ID" --holder "$INVESTOR_ADDR")"
