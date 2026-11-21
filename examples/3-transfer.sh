#!/usr/bin/env bash
# Workflow 3: mint to an approved investor and transfer tokens.
# Usage: INVESTOR_IDENTITY=alice INVESTOR_ADDR=G... ASSET_ID=C... ./examples/3-transfer.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
. "$ROOT/examples/common.sh"

: "${INVESTOR_IDENTITY:?Set INVESTOR_IDENTITY to a local stellar identity, e.g. alice}"
: "${INVESTOR_ADDR:?Set INVESTOR_ADDR to the public key of that identity (G...)}"
AMOUNT="${AMOUNT:-1000}"

# Mint requires the asset-token admin; the recipient must pass compliance.
invoke "$ASSET_ID" mint \
  --admin "$ADMIN_ADDR" --to "$INVESTOR_ADDR" --amount "$AMOUNT"

# The investor signs the transfer with their own identity.
stellar contract invoke \
  --id "$ASSET_ID" \
  --source "$INVESTOR_IDENTITY" \
  --network "$NETWORK" \
  -- transfer --from "$INVESTOR_ADDR" --to "$ADMIN_ADDR" --amount "$AMOUNT"

# Batch form: Vec<(Address, i128)> is a JSON array of 2-element arrays.
BATCH="$(printf '[["%s","%s"]]' "$INVESTOR_ADDR" "$AMOUNT")"
stellar contract invoke \
  --id "$ASSET_ID" \
  --source "$INVESTOR_IDENTITY" \
  --network "$NETWORK" \
  -- transfer_batch --from "$INVESTOR_ADDR" --transfers "$BATCH"

echo "alice balance: $(invoke "$ASSET_ID" balance --id "$INVESTOR_ADDR")"
