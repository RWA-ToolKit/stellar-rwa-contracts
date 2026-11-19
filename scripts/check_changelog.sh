#!/usr/bin/env bash
#
# Require a CHANGELOG entry when a pull request changes contract behaviour.
#
# Only contract sources and manifests count. Tests and regenerated ledger
# snapshots also live under contracts/ and CI requires those snapshots to be
# committed, so a test-only or snapshot-only change must not be forced to
# invent an entry.
#
# Requiring the Unreleased section to *gain a line* rather than merely to be
# touched means a whitespace edit no longer satisfies the gate, while a real
# contract change with no entry does not slip through.
#
# Usage: check_changelog.sh <base-ref> [head-ref]
#   Exits 0 when no entry is required or one was added, 1 otherwise.

set -euo pipefail

usage() {
  printf 'Usage: check_changelog.sh <base-ref> [head-ref]\n'
}

if [ "$#" -eq 1 ] && [ "$1" = "--help" ]; then
  usage
  exit 0
fi

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ] || [[ "$1" == -* ]]; then
  usage >&2
  exit 2
fi

BASE="$1"
HEAD="${2:-HEAD}"
for ref in "$BASE" "$HEAD"; do
  if ! git rev-parse --verify --quiet --end-of-options "${ref}^{commit}" >/dev/null; then
    printf 'Error: invalid commit ref: %s\n' "$ref" >&2
    usage >&2
    exit 2
  fi
done
CHANGELOG=CHANGELOG.md

# A behaviour change is a contract source or manifest edit. Any file under
# `src/` counts so that entry points moved into modules are still caught.
# `src/test.rs` and `src/test/**` are deliberately excluded, as is
# `test_snapshots/**`, which sits outside `src/`.
CONTRACT_CHANGE_RE='^contracts/[^/]+/(src/.+\.rs|Cargo\.toml)$'
CONTRACT_TEST_RE='^contracts/[^/]+/src/test(/|\.rs$)'

# Non-blank lines under `## [Unreleased]`, stopping at the next `## ` heading.
count_unreleased() {
  awk '/^## \[Unreleased\]/{inside=1; next} /^## /{inside=0} inside' "$1" \
    | grep -c '[^[:space:]]' || true
}

changed="$(git diff --name-only "$BASE...$HEAD")"
printf 'Changed files:\n%s\n' "$changed"

triggers="$(printf '%s\n' "$changed" | grep -E "$CONTRACT_CHANGE_RE" | grep -vE "$CONTRACT_TEST_RE" || true)"
if [ -z "$triggers" ]; then
  echo "No contract source or manifest changes; a CHANGELOG entry is not required."
  exit 0
fi
printf 'Contract changes requiring an entry:\n%s\n' "$triggers"

if ! git cat-file -e "$BASE:$CHANGELOG" 2>/dev/null; then
  echo "$CHANGELOG did not exist on $BASE; treating its contents as the entry."
  exit 0
fi

base_copy="$(mktemp)"
head_copy="$(mktemp)"
trap 'rm -f "$base_copy" "$head_copy"' EXIT
git show "$BASE:$CHANGELOG" >"$base_copy"
git show "$HEAD:$CHANGELOG" >"$head_copy"

before="$(count_unreleased "$base_copy")"
after="$(count_unreleased "$head_copy")"

if [ "$after" -le "$before" ]; then
  cat >&2 <<EOF
::error::Contract sources changed but the '## [Unreleased]' section of $CHANGELOG did not gain an entry. Add one under Unreleased (see CONTRIBUTING.md), or apply the 'skip-changelog' label if this change is internal.
EOF
  exit 1
fi

echo "CHANGELOG '## [Unreleased]' gained $((after - before)) line(s)."
