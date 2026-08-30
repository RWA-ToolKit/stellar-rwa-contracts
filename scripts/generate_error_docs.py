#!/usr/bin/env python3
"""
Generate the `## Errors` table for each contract doc by parsing the
contract's `#[contracterror] pub enum Error` and its doc comments, so the
documented error codes cannot drift from the source (companion to
`generate_storage_docs.py`, which does the same for `DataKey`).

Usage: scripts/generate_error_docs.py
"""
from pathlib import Path
import re


def parse_errors(src: str):
    """Return [(name, code, cause), ...] from the `#[contracterror] enum Error`."""
    m = re.search(r"#\[contracterror\][\s\S]*?enum\s+Error\s*\{([\s\S]*?)\n\}", src)
    if not m:
        return []
    body = m.group(1)
    errors = []
    pending_doc = []
    for raw_line in body.splitlines():
        line = raw_line.strip()
        if not line:
            continue
        doc_m = re.match(r"///\s?(.*)", line)
        if doc_m:
            pending_doc.append(doc_m.group(1))
            continue
        var_m = re.match(r"([A-Za-z0-9_]+)\s*=\s*(\d+)", line.rstrip(','))
        if var_m:
            name = var_m.group(1)
            code = int(var_m.group(2))
            cause = ' '.join(pending_doc) if pending_doc else '-'
            errors.append((name, code, cause))
        pending_doc = []
    return errors


def build_section(errors: list):
    lines = ['## Errors', '']
    lines.append('| Code | Name | Cause |')
    lines.append('|------|------|-------|')
    for name, code, cause in errors:
        lines.append(f'| {code} | {name} | {cause} |')
    lines.append('')
    return '\n'.join(lines)


def update_doc(doc: Path, section: str):
    text = doc.read_text()
    if '## Errors' in text:
        new_text, n = re.subn(
            r"(?s)## Errors.*?(\n## |\Z)",
            lambda m: section + (m.group(1) if m.group(1).startswith('\n## ') else ''),
            text,
            count=1,
        )
        if n:
            if new_text != text:
                doc.write_text(new_text)
                print(f'updated: {doc}')
            else:
                print(f'up to date: {doc}')
            return
    if '## Events' in text:
        doc.write_text(text.replace('## Events', section + '\n## Events'))
        print(f'inserted: {doc} (before Events)')
        return
    doc.write_text(text + '\n' + section)
    print(f'appended: {doc}')


def main():
    root = Path(__file__).resolve().parent.parent
    contracts = root / 'contracts'
    docs = root / 'docs'
    for c in sorted(contracts.iterdir()):
        if not c.is_dir():
            continue
        src = c / 'src' / 'lib.rs'
        doc = docs / f"{c.name}.md"
        if not src.exists() or not doc.exists():
            continue
        errors = parse_errors(src.read_text())
        if not errors:
            continue
        update_doc(doc, build_section(errors))


if __name__ == '__main__':
    main()
