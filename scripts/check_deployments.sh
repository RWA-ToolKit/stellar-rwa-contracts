#!/usr/bin/env bash
#
# Verify that the contract ids recorded in DEPLOYMENTS.md still exist on
# Testnet and still serve the wasm this repo currently builds. Intended to
# run on a schedule so a redeploy that forgot to update DEPLOYMENTS.md (or an
# id that stopped resolving) is caught instead of the doc silently drifting.
#
# Usage: scripts/check_deployments.sh [path/to/DEPLOYMENTS.md]
#
# Requires: stellar CLI, and the contracts already built to
# target/wasm32v1-none/release/*.wasm (`cargo build --release --target
# wasm32v1-none`, or `make build`).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEPLOYMENTS_MD="${1:-$ROOT/DEPLOYMENTS.md}"
WASM_DIR="$ROOT/target/wasm32v1-none/release"
NETWORK="testnet"

status=0

# Pull "| name | `CONTRACT_ID` | ... |" rows out of the Testnet table. A row
# is recognized by its id column being a valid contract StrKey (C + 55 chars);
# this naturally skips the header and separator rows without hardcoding them.
rows="$(sed -n '/^## Testnet/,/^## /p' "$DEPLOYMENTS_MD" | grep '^|')"

while IFS='|' read -r _ name cid _; do
  name="$(echo "$name" | xargs)"
  cid="$(echo "$cid" | tr -d '`' | xargs)"

  [[ "$cid" =~ ^C[A-Z0-9]{55}$ ]] || continue

  wasm_name="${name//-/_}"
  wasm_path="$WASM_DIR/${wasm_name}.wasm"
  if [ ! -f "$wasm_path" ]; then
    echo "::error::no local build found for '$name' at $wasm_path (build the contracts first)"
    status=1
    continue
  fi
  expected_hash="$(sha256sum "$wasm_path" | awk '{print $1}')"

  echo "==> Checking $name ($cid) on $NETWORK..."
  live_wasm="$(mktemp)"
  if ! stellar contract fetch --id "$cid" --network "$NETWORK" --out-file "$live_wasm" >/dev/null 2>&1; then
    echo "::error::contract '$name' ($cid) could not be fetched from $NETWORK — it may no longer exist"
    status=1
    rm -f "$live_wasm"
    continue
  fi
  live_hash="$(sha256sum "$live_wasm" | awk '{print $1}')"
  rm -f "$live_wasm"

  if [ "$live_hash" != "$expected_hash" ]; then
    echo "::error::contract '$name' ($cid) on $NETWORK does not match the wasm this repo builds — DEPLOYMENTS.md is likely stale after a redeploy"
    status=1
  else
    echo "    ok: $name matches the deployed wasm"
  fi
done <<< "$rows"

exit $status
