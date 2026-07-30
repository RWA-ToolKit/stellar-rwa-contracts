#!/usr/bin/env bash
#
# Deploy the Stellar RWA contracts to a Soroban network (Testnet by default)
# and initialize them into a working configuration.
#
# Usage:
#   ./scripts/deploy.sh                 # deploy/reuse on testnet with identity "rwa-admin"
#   NETWORK=testnet IDENTITY=rwa-admin ./scripts/deploy.sh
#   FORCE_REDEPLOY=1 ./scripts/deploy.sh # ignore deployments.json, deploy fresh instances
#
# Requirements: stellar CLI (>= 22), jq, a funded identity on the target network.
#
# Idempotency (issue #100): re-running this script for a network that already
# has an entry in deployments.json reuses those contract ids (and skips their
# one-time `initialize` / registration calls) instead of deploying brand-new
# instances. Set FORCE_REDEPLOY=1 to force fresh instances regardless.
#
# Output: writes/updates deployments.json at the repo root with
#   { "<network>": { "compliance": "...", "registry": "...", "dividend": "...", "sample": "..." } }
# so the web and API repos can consume ids programmatically instead of them
# drifting out of sync with hardcoded copies. Also updates DEPLOYMENTS.md.

set -euo pipefail

NETWORK="${NETWORK:-testnet}"
IDENTITY="${IDENTITY:-rwa-admin}"
FORCE_REDEPLOY="${FORCE_REDEPLOY:-0}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WASM_DIR="$ROOT/target/wasm32v1-none/release"
DEPLOYMENTS_JSON="$ROOT/deployments.json"
DEPLOYMENTS_MD="$ROOT/DEPLOYMENTS.md"

echo "==> Network:  $NETWORK"
echo "==> Identity: $IDENTITY"

if ! command -v jq >/dev/null 2>&1; then
  echo "error: this script requires 'jq' (used to read/write deployments.json)" >&2
  exit 1
fi

[ -f "$DEPLOYMENTS_JSON" ] || echo '{}' >"$DEPLOYMENTS_JSON"

# existing_id <key> -> previously recorded contract id for $NETWORK, or empty.
existing_id() {
  jq -r --arg net "$NETWORK" --arg key "$1" '.[$net][$key] // empty' "$DEPLOYMENTS_JSON"
}

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

# is_reused <key> -> "1" if deployments.json already has an id for <key> on
# this network and FORCE_REDEPLOY isn't set, "0" otherwise. Must be checked
# *before* deploying (deployments.json itself isn't written until the end).
is_reused() {
  if [ "$FORCE_REDEPLOY" != "1" ] && [ -n "$(existing_id "$1")" ]; then
    echo 1
  else
    echo 0
  fi
}

echo "==> Deploying compliance..."
COMPLIANCE_REUSED="$(is_reused compliance)"
if [ "$COMPLIANCE_REUSED" = "1" ]; then
  COMPLIANCE_ID="$(existing_id compliance)"
  echo "    (reusing existing compliance: $COMPLIANCE_ID)"
else
  COMPLIANCE_ID="$(deploy compliance)"
  echo "    compliance: $COMPLIANCE_ID"
  invoke "$COMPLIANCE_ID" initialize --admin "$ADMIN_ADDR"
fi

echo "==> Deploying registry..."
REGISTRY_REUSED="$(is_reused registry)"
if [ "$REGISTRY_REUSED" = "1" ]; then
  REGISTRY_ID="$(existing_id registry)"
  echo "    (reusing existing registry: $REGISTRY_ID)"
else
  REGISTRY_ID="$(deploy registry)"
  echo "    registry:   $REGISTRY_ID"
  invoke "$REGISTRY_ID" initialize --admin "$ADMIN_ADDR"
fi

echo "==> Deploying dividend..."
DIVIDEND_REUSED="$(is_reused dividend)"
if [ "$DIVIDEND_REUSED" = "1" ]; then
  DIVIDEND_ID="$(existing_id dividend)"
  echo "    (reusing existing dividend: $DIVIDEND_ID)"
else
  DIVIDEND_ID="$(deploy dividend)"
  echo "    dividend:   $DIVIDEND_ID"
  invoke "$DIVIDEND_ID" initialize --admin "$ADMIN_ADDR"
fi

echo "==> Approving admin on compliance (so it can hold the sample asset)..."
# add_to_allowlist is a re-approve and safe to call every run.
invoke "$COMPLIANCE_ID" add_to_allowlist \
  --admin "$ADMIN_ADDR" --address "$ADMIN_ADDR" --jurisdiction "US" --expires_at 0

echo "==> Deploying asset-token (sample real-estate asset)..."
ASSET_REUSED="$(is_reused sample)"
if [ "$ASSET_REUSED" = "1" ]; then
  ASSET_ID="$(existing_id sample)"
  echo "    (reusing existing asset-token: $ASSET_ID)"
else
  ASSET_ID="$(deploy asset_token)"
  echo "    asset-token: $ASSET_ID"
fi
if [ "$ASSET_REUSED" = "0" ]; then
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
else
  echo "==> Skipping asset registration (sample asset reused, already registered)."
fi

# 3. Record ids in a machine-readable deployments.json for downstream repos.
TMP_JSON="$(mktemp)"
jq --arg net "$NETWORK" \
   --arg compliance "$COMPLIANCE_ID" \
   --arg registry "$REGISTRY_ID" \
   --arg dividend "$DIVIDEND_ID" \
   --arg sample "$ASSET_ID" \
   --arg admin "$ADMIN_ADDR" \
   '.[$net] = {compliance: $compliance, registry: $registry, dividend: $dividend, sample: $sample, admin: $admin}' \
   "$DEPLOYMENTS_JSON" >"$TMP_JSON"
mv "$TMP_JSON" "$DEPLOYMENTS_JSON"
echo "==> Wrote $DEPLOYMENTS_JSON"

# 4. Update the human-readable DEPLOYMENTS.md section for this network.
network_passphrase() {
  case "$NETWORK" in
    testnet) echo "Test SDF Network ; September 2015" ;;
    futurenet) echo "Test SDF Future Network ; October 2022" ;;
    mainnet|public) echo "Public Global Stellar Network ; September 2015" ;;
    *) echo "$NETWORK" ;;
  esac
}

update_deployments_md() {
  local heading start_marker end_marker section_file
  heading="$(tr '[:lower:]' '[:upper:]' <<<"${NETWORK:0:1}")${NETWORK:1}"
  start_marker="<!-- deployments:${NETWORK}:start -->"
  end_marker="<!-- deployments:${NETWORK}:end -->"
  section_file="$(mktemp)"
  {
    echo "$start_marker"
    echo "## $heading"
    echo
    echo "Network: \`$(network_passphrase)\`"
    echo "Deployed: $(date -u +%Y-%m-%d)"
    echo
    echo "| Contract    | Contract ID                                                | Explorer |"
    echo "|-------------|------------------------------------------------------------|----------|"
    echo "| compliance  | \`$COMPLIANCE_ID\` | [view](https://stellar.expert/explorer/${NETWORK}/contract/${COMPLIANCE_ID}) |"
    echo "| registry    | \`$REGISTRY_ID\` | [view](https://stellar.expert/explorer/${NETWORK}/contract/${REGISTRY_ID}) |"
    echo "| dividend    | \`$DIVIDEND_ID\` | [view](https://stellar.expert/explorer/${NETWORK}/contract/${DIVIDEND_ID}) |"
    echo "| asset-token | \`$ASSET_ID\` | [view](https://stellar.expert/explorer/${NETWORK}/contract/${ASSET_ID}) |"
    echo
    echo "**Admin / issuer account:** \`$ADMIN_ADDR\`"
    echo
    echo "### Sample asset"
    echo
    echo "The deployment script registers one sample asset for demonstration:"
    echo
    echo "- **Name:** Manhattan Loft (\`MLOFT\`)"
    echo "- **Type:** \`real_estate\`"
    echo "- **Total supply:** 1,000,000 (2 decimals)"
    echo "- **Valuation:** 500,000,000 USD cents (\$5,000,000)"
    echo "- **Compliance:** gated by the compliance contract above; the admin/issuer is"
    echo "  the only KYC-approved holder at deploy time."
    echo "$end_marker"
  } >"$section_file"

  if [ ! -f "$DEPLOYMENTS_MD" ]; then
    { echo "# Deployments"; echo; echo "Live contract addresses for the Stellar RWA Toolkit contracts."; echo; } >"$DEPLOYMENTS_MD"
  fi

  if grep -qF "$start_marker" "$DEPLOYMENTS_MD"; then
    awk -v start="$start_marker" -v end="$end_marker" -v section_file="$section_file" '
      $0 == start { skip = 1; while ((getline line < section_file) > 0) print line; close(section_file) }
      $0 == end { skip = 0; next }
      skip { next }
      { print }
    ' "$DEPLOYMENTS_MD" >"${DEPLOYMENTS_MD}.tmp"
  else
    # No markers for this network yet (a brand-new network, e.g. futurenet):
    # append the section to the end of the file.
    cp "$DEPLOYMENTS_MD" "${DEPLOYMENTS_MD}.tmp"
    printf '\n' >>"${DEPLOYMENTS_MD}.tmp"
    cat "$section_file" >>"${DEPLOYMENTS_MD}.tmp"
    printf '\n' >>"${DEPLOYMENTS_MD}.tmp"
  fi
  mv "${DEPLOYMENTS_MD}.tmp" "$DEPLOYMENTS_MD"
  rm -f "$section_file"
}

update_deployments_md
echo "==> Updated $DEPLOYMENTS_MD"

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
Ids recorded in deployments.json and DEPLOYMENTS.md.
EOF
