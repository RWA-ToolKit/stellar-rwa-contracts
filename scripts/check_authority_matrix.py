#!/usr/bin/env python3
"""Check docs/AUTHORITY_MATRIX.md stays consistently formatted (issue #714).

The matrix is hand-maintained, so over many PRs its tables drift: a missing
trailing pipe, a padded cell, a row with one column too few, the same function
listed twice. Each is small, but they make diffs noisy and can hide a real
content change. Row *order* is deliberately not enforced: tables list
`initialize` first and then follow the contract's own grouping.

Rules, per table:
  1. every row starts with "| " and ends with " |" (cells padded by one space);
  2. the separator row is exactly "|---|---|..." with one "---" per column;
  3. every row has the header's column count;
  4. in "Function" tables, the first cell names a function in backticks and
     no function row appears twice.

It also cross-checks the code against the matrix (issue #669): every
privileged `pub fn` in the four contracts (one taking an `admin: Address`,
`caller: Address`, or similar privileged Address argument) must have a
backticked row in that contract's section of the matrix. This catches the
matrix silently drifting as new admin-gated functions are added.

Usage: python3 scripts/check_authority_matrix.py [path]   (exit 1 on problems)
"""
import os
import re
import sys

path = sys.argv[1] if len(sys.argv) > 1 else "docs/AUTHORITY_MATRIX.md"

# Contract name -> source files that may declare privileged entry points.
CONTRACT_SOURCES = {
    "asset-token": ["contracts/asset-token/src/lib.rs"],
    "compliance": [
        "contracts/compliance/src/lib.rs",
        "contracts/compliance/src/admin.rs",
    ],
    "registry": ["contracts/registry/src/lib.rs"],
    "dividend": ["contracts/dividend/src/lib.rs"],
}

# Parameter names that mark an entry point as privileged.
PRIVILEGED_PARAMS = {"admin", "caller", "operator", "owner", "issuer", "guardian"}


def cells(line):
    # Split on pipes that are not inside backticks, e.g. `a | b` in code.
    parts, buf, in_code = [], "", False
    for ch in line.strip()[1:-1]:
        if ch == "`":
            in_code = not in_code
        if ch == "|" and not in_code:
            parts.append(buf)
            buf = ""
        else:
            buf += ch
    parts.append(buf)
    return parts


problems = []
lines = open(path, encoding="utf-8").read().split("\n")
i = 0
while i < len(lines):
    if not lines[i].startswith("|"):
        i += 1
        continue
    start = i
    table = []
    while i < len(lines) and lines[i].startswith("|"):
        table.append((i + 1, lines[i]))
        i += 1
    header_no, header = table[0]
    columns = len(cells(header))
    if len(table) < 2:
        problems.append(f"{path}:{header_no}: table has a header but no separator row")
        continue
    sep_no, sep = table[1]
    expected_sep = "|" + "---|" * columns
    if sep != expected_sep:
        problems.append(f"{path}:{sep_no}: separator should be {expected_sep!r}, got {sep!r}")
    is_function_table = cells(header)[0].strip() == "Function"
    seen = {}
    for no, row in [table[0]] + table[2:]:
        if not (row.startswith("| ") and row.endswith(" |")):
            problems.append(f"{path}:{no}: row must start with '| ' and end with ' |'")
        row_cells = cells(row)
        if len(row_cells) != columns:
            problems.append(f"{path}:{no}: {len(row_cells)} columns, header has {columns}")
        for c in row_cells:
            if c != " " and (not c.startswith(" ") or not c.endswith(" ") or c.startswith("  ") or c.endswith("  ")):
                problems.append(f"{path}:{no}: cell {c.strip()!r} should be padded by exactly one space")
                break
        if is_function_table and no != header_no:
            first = row_cells[0].strip()
            if not re.match(r"^`[a-z_][a-z0-9_]*`", first):
                problems.append(f"{path}:{no}: first cell should name a function in backticks, got {first!r}")
            if first in seen:
                problems.append(f"{path}:{no}: {first} is already listed on line {seen[first]}")
            seen.setdefault(first, no)

# ---------------------------------------------------------------------------
# Code-vs-matrix cross-check (issue #669).
# ---------------------------------------------------------------------------

# Map each contract section heading in the matrix to the contract name.
SECTION_RE = re.compile(r"^#+\s*(.+?)\s*$")


def matrix_functions(text):
    """Return {contract: set(function names)} from backticked first cells.

    Rows are attributed to the most recent heading that names a known
    contract; rows before any such heading are ignored.
    """
    documented = {name: set() for name in CONTRACT_SOURCES}
    current = None
    for line in text.split("\n"):
        heading = SECTION_RE.match(line)
        if heading and not line.startswith("|"):
            title = heading.group(1).lower()
            current = None
            for name in CONTRACT_SOURCES:
                if name in title:
                    current = name
                    break
            continue
        if current and line.startswith("|"):
            first = cells(line)[0].strip()
            m = re.match(r"^`([a-z_][a-z0-9_]*)`", first)
            if m:
                documented[current].add(m.group(1))
    return documented


def privileged_functions(source):
    """Return names of `pub fn`s whose signature takes a privileged Address."""
    try:
        text = open(source, encoding="utf-8").read()
    except FileNotFoundError:
        return set()
    found = set()
    # Match `pub fn name(` and capture the parameter list up to the closing
    # paren of the signature (balanced across newlines).
    for m in re.finditer(r"pub\s+fn\s+([a-z_][a-z0-9_]*)\s*\(", text):
        name = m.group(1)
        depth, i = 1, m.end()
        while i < len(text) and depth:
            if text[i] == "(":
                depth += 1
            elif text[i] == ")":
                depth -= 1
            i += 1
        params = text[m.end():i - 1]
        for param in params.split(","):
            pname = param.split(":")[0].strip()
            if pname in PRIVILEGED_PARAMS and "Address" in param:
                found.add(name)
                break
    return found


root = os.path.dirname(os.path.dirname(os.path.abspath(path)))
documented = matrix_functions(open(path, encoding="utf-8").read())
for contract, sources in CONTRACT_SOURCES.items():
    in_code = set()
    for src in sources:
        in_code |= privileged_functions(os.path.join(root, src))
    missing = sorted(in_code - documented.get(contract, set()))
    for fn in missing:
        problems.append(
            f"{path}: privileged function `{fn}` in {contract} is not documented "
            f"in the {contract} section"
        )

if problems:
    print("\n".join(problems))
    print(f"\n{len(problems)} problem(s) in {path}.")
    sys.exit(1)
print(f"{path}: all tables consistently formatted and privileged functions documented.")
