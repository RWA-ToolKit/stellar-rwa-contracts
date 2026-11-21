#!/usr/bin/env bash
#
# Issue #776: enforce that a VERSION constant bump is accompanied by a
# CHANGELOG.md entry that references the new version number.
#
# If any `pub const VERSION: u32 = N;` line changes between BASE and HEAD, the
# new value N must appear as a literal in the CHANGELOG.md's Unreleased section.
#
# Usage: check_version_bump.sh <base-ref> [head-ref]
#   Exits 0 when no version bump occurred, or one is documented.
#   Exits 1 when a version bump is not reflected in CHANGELOG.md.

set -euo pipefail

BASE="${1:?usage: check_version_bump.sh <base-ref> [head-ref]}"
HEAD="${2:-HEAD}"
CHANGELOG=CHANGELOG.md

# Collect changed contract lib.rs files.
changed_libs="$(git diff --name-only "$BASE...$HEAD" \
  | grep -E '^contracts/[^/]+/src/lib\.rs$' || true)"

if [ -z "$changed_libs" ]; then
  echo "No contract lib.rs files changed; version-bump check skipped."
  exit 0
fi

# Extract the head CHANGELOG Unreleased section.
if ! git cat-file -e "$HEAD:$CHANGELOG" 2>/dev/null; then
  echo "$CHANGELOG not found at HEAD; skipping version-bump check."
  exit 0
fi
unreleased="$(git show "$HEAD:$CHANGELOG" \
  | awk '/^## \[Unreleased\]/{inside=1; next} /^## /{inside=0} inside')"

fail=0
while IFS= read -r lib; do
  # Read the new VERSION value from HEAD.
  new_line="$(git show "$HEAD:$lib" 2>/dev/null \
    | grep -E '^pub const VERSION: u32 = [0-9]+;' || true)"
  old_line="$(git show "$BASE:$lib" 2>/dev/null \
    | grep -E '^pub const VERSION: u32 = [0-9]+;' || true)"

  [ "$new_line" = "$old_line" ] && continue  # unchanged

  new_ver="$(printf '%s' "$new_line" | grep -oE '[0-9]+;$' | tr -d ';')"
  if [ -z "$new_ver" ]; then
    continue
  fi

  # Check that the new version number appears in the Unreleased section.
  if ! printf '%s' "$unreleased" | grep -qF "$new_ver"; then
    printf '::error file=%s::VERSION bumped to %s but no matching entry found in the Unreleased section of %s. Add a CHANGELOG entry that mentions version %s.\n' \
      "$lib" "$new_ver" "$CHANGELOG" "$new_ver" >&2
    fail=1
  else
    printf '%s: VERSION bumped to %s — CHANGELOG entry found.\n' "$lib" "$new_ver"
  fi
done <<< "$changed_libs"

exit "$fail"
