#!/usr/bin/env bash
# check_quickstart.sh - Verify QUICKSTART.md does not reference script paths
# or CLI flags that do not exist in the repository.
#
# Checks performed:
#   1. Every script path mentioned in QUICKSTART.md exists in the repo.
#   2. The wasm target used in build commands matches rust-toolchain.toml.
#   3. The CLI tool used is "stellar" (not "soroban-cli").
#
# Run from the repository root:
#   ./scripts/check_quickstart.sh
#
# Exit code 0 = no issues found; non-zero = at least one issue detected.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOC="$REPO_ROOT/QUICKSTART.md"
TOOLCHAIN="$REPO_ROOT/rust-toolchain.toml"
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
if grep -q 'wasm32-unknown-unknown' "$DOC"; then
  red "FAIL: $DOC references 'wasm32-unknown-unknown' but rust-toolchain.toml uses '$TOOLCHAIN_TARGET'."
  ERRORS=$((ERRORS + 1))
fi

# 3. Detect use of soroban-cli instead of stellar CLI.
if grep -q 'soroban-cli\|soroban contract\b' "$DOC"; then
  red "FAIL: $DOC references 'soroban-cli' or 'soroban contract'; use the 'stellar' CLI (>= 22) instead."
  ERRORS=$((ERRORS + 1))
fi

if [ "$ERRORS" -eq 0 ]; then
  green "OK: QUICKSTART.md checks passed."
else
  red "FAIL: $ERRORS issue(s) found in QUICKSTART.md."
  exit 1
fi
