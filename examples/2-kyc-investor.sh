#!/usr/bin/env bash
# Workflow 2: KYC an investor (allowlist), including the struct/Vec encoding.
# Usage: INVESTOR_ADDR=G... COMPLIANCE_ID=C... ... ./examples/2-kyc-investor.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
. "$ROOT/examples/common.sh"

: "${INVESTOR_ADDR:?Set INVESTOR_ADDR to the investor public key (G...)}"
JURISDICTION="${JURISDICTION:-US}"
# 0 = never expires (see docs/compliance.md).
EXPIRES_AT="${EXPIRES_AT:-0}"

# Single approval.
invoke "$COMPLIANCE_ID" add_to_allowlist \
  --admin "$ADMIN_ADDR" \
  --address "$INVESTOR_ADDR" \
  --jurisdiction "$JURISDICTION" \
  --expires_at "$EXPIRES_AT"

# Batch form: Vec<AllowlistEntry> is passed as a JSON array. Each entry has
# address, jurisdiction and expires_at; 0 means permanent.
BATCH_ENTRIES="$(printf '[{"address":"%s","jurisdiction":"%s","expires_at":%s}]' \
  "$INVESTOR_ADDR" "$JURISDICTION" "$EXPIRES_AT")"
invoke "$COMPLIANCE_ID" add_to_allowlist_batch \
  --admin "$ADMIN_ADDR" \
  --entries "$BATCH_ENTRIES"

echo "is_allowed: $(invoke "$COMPLIANCE_ID" is_allowed --address "$INVESTOR_ADDR")"
