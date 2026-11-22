#!/usr/bin/env bash
# check_quickstart.sh - Verify QUICKSTART.md does not reference script paths
# or CLI flags that do not exist in the repository.
#
# Checks performed:
#   1. Every script path mentioned in QUICKSTART.md exists in the repo.
#   2. The wasm target used in build commands matches rust-toolchain.toml
#      (wasm32v1-none for Soroban SDK 26).
#   3. The CLI tool used is "stellar" (not "soroban-cli"/"soroban contract"),
#      and the documented minimum stellar-cli version is >= 26.
#   4. The admin handover flow (propose_admin -> accept_admin) is documented
#      and both functions exist on the asset-token contract.
#
# Run from the repository root:
#   ./scripts/check_quickstart.sh
#
# Exit code 0 = no issues found; non-zero = at least one issue detected.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOC="$REPO_ROOT/QUICKSTART.md"
TOOLCHAIN="$REPO_ROOT/rust-toolchain.toml"
ASSET_TOKEN_LIB="$REPO_ROOT/contracts/asset-token/src/lib.rs"
ERRORS=0

red()   { printf "\033[31m%s\033[0m\n" "$*"; }
green() { printf "\033[32m%s\033[0m\n" "$*"; }

# 1. Detect references to scripts/* and verify they exist.
while IFS= read -r script_path; do
  full_path="$REPO_ROOT/$script_path"
  if [ ! -f "$full_path" ]; then
    red "FAIL: $DOC references '$script_path' which does not exist in the repo."
    ERRORS=$((ERRORS + 1))
  fi
done < <(grep -oP 'scripts/[a-zA-Z0-9_\-]+\.sh' "$DOC" | sort -u)

# 2. Verify the wasm target mentioned in QUICKSTART.md matches rust-toolchain.toml.
TOOLCHAIN_TARGET="$(grep 'targets' "$TOOLCHAIN" | grep -oP 'wasm[^\s"]+' | head -1)"
if [ -z "$TOOLCHAIN_TARGET" ]; then
  red "FAIL: could not determine wasm target from $TOOLCHAIN."
  ERRORS=$((ERRORS + 1))
elif ! grep -q "$TOOLCHAIN_TARGET" "$DOC"; then
  red "FAIL: $DOC does not reference the wasm target '$TOOLCHAIN_TARGET' from rust-toolchain.toml."
  ERRORS=$((ERRORS + 1))
fi
if grep -q 'wasm32-unknown-unknown' "$DOC"; then
  red "FAIL: $DOC references 'wasm32-unknown-unknown' but rust-toolchain.toml uses '$TOOLCHAIN_TARGET'."
  ERRORS=$((ERRORS + 1))
fi

# 3. Detect use of soroban-cli instead of stellar CLI, and verify the
#    documented minimum stellar-cli version is >= 26 (Soroban SDK 26).
if grep -q 'soroban-cli\|soroban contract\b' "$DOC"; then
  red "FAIL: $DOC references 'soroban-cli' or 'soroban contract'; use the 'stellar' CLI (>= 26) instead."
  ERRORS=$((ERRORS + 1))
fi
if ! grep -q 'stellar' "$DOC"; then
  red "FAIL: $DOC does not reference the 'stellar' CLI."
  ERRORS=$((ERRORS + 1))
fi
MIN_CLI_VERSION="$(grep -oP 'stellar(?:-cli)?[^\n]*?\b(\d{2,})\b' "$DOC" | grep -oP '\b\d{2,}\b' | sort -n | head -1 || true)"
if [ -z "$MIN_CLI_VERSION" ]; then
  red "FAIL: $DOC does not state a minimum stellar-cli version (expected >= 26)."
  ERRORS=$((ERRORS + 1))
elif [ "$MIN_CLI_VERSION" -lt 26 ]; then
  red "FAIL: $DOC states minimum stellar-cli version $MIN_CLI_VERSION; Soroban SDK 26 requires >= 26."
  ERRORS=$((ERRORS + 1))
fi

# 4. Verify the admin handover flow is documented and implemented by the contract.
for fn_name in propose_admin accept_admin; do
  if ! grep -qP -- "-- ${fn_name}\b" "$DOC"; then
    red "FAIL: $DOC does not document the admin handover step '$fn_name'."
    ERRORS=$((ERRORS + 1))
  fi
  if ! grep -q "pub fn ${fn_name}(" "$ASSET_TOKEN_LIB"; then
    red "FAIL: $ASSET_TOKEN_LIB does not define '$fn_name'."
    ERRORS=$((ERRORS + 1))
  fi
done

if [ "$ERRORS" -eq 0 ]; then
  green "OK: QUICKSTART.md checks passed."
else
  red "FAIL: $ERRORS issue(s) found in QUICKSTART.md."
  exit 1
fi
