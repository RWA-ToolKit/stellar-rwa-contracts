#!/usr/bin/env python3
"""
Generate a `Storage / TTL` section for each contract doc by parsing the
contract's `DataKey` enum and scan for instance/persistent usage and TTL
extensions to keep docs in sync with the code.

Usage: scripts/generate_storage_docs.py
"""
from pathlib import Path
import re
import sys


def parse_datakey(src: str):
    m = re.search(r"enum\s+DataKey\s*\{([\s\S]*?)\}", src)
    if not m:
        return []
    body = m.group(1)
    variants = []
    for line in body.splitlines():
        line = line.strip().rstrip(',')
        if not line:
            continue
        # Capture variant and optional type
        vm = re.match(r"([A-Za-z0-9_]+)\s*(?:\(([^)]+)\))?", line)
        if vm:
            name = vm.group(1)
            typ = vm.group(2) if vm.group(2) else ''
            variants.append((name, typ.strip()))
    return variants


def analyze_usage(src: str, variant: str):
    info = {
        'instance_set': False,
        'persistent_set': False,
        'instance_get': False,
        'persistent_get': False,
        'extend_ttl': False,
    }
    # patterns for DataKey::Variant or DataKey::Variant(
    pat = re.escape(f"DataKey::{variant}")
    if re.search(rf"\.instance\(\)\.[^.]*\bset\s*\(\s*&\s*{pat}", src):
        info['instance_set'] = True
    if re.search(rf"\.persistent\(\)\.[^.]*\bset\s*\(\s*&\s*{pat}", src):
        info['persistent_set'] = True
    if re.search(rf"\.instance\(\)\.[^.]*\bget\s*\(\s*&\s*{pat}", src):
        info['instance_get'] = True
    if re.search(rf"\.persistent\(\)\.[^.]*\bget\s*\(\s*&\s*{pat}", src):
        info['persistent_get'] = True
    if re.search(rf"extend_ttl\s*\(\s*&?\s*{pat}", src):
        info['extend_ttl'] = True
    return info


def build_section(name: str, variants_info: list):
    lines = []
    lines.append('## Storage / TTL')
    lines.append('')
    lines.append('Listing of the contract `DataKey` variants and their storage behaviour.')
    lines.append('')
    lines.append('| Key | Payload | Storage | TTL / Notes |')
    lines.append('|-----|---------|---------|-------------|')
    for var, typ, info in variants_info:
        payload = typ if typ else '-'
        storage = []
        if info['instance_set'] or info['instance_get']:
            storage.append('instance')
        if info['persistent_set'] or info['persistent_get']:
            storage.append('persistent')
        storage = ', '.join(storage) if storage else 'unknown'
        ttl = 'extended via instance()' if info['extend_ttl'] else ('per-key TTL' if 'Dist' in var or 'Asset' in var or 'Record' in var or 'Claimed' in var else '')
        if not ttl:
            ttl = '-'
        lines.append(f'| `{var}` | {payload} | {storage} | {ttl} |')
    lines.append('')
    return '\n'.join(lines)


def update_doc(doc: Path, section: str):
    text = doc.read_text()
    if '## Storage / TTL' in text:
        new_text, n = re.subn(r"(?s)## Storage / TTL.*?(\n## |\Z)", lambda m: section + (m.group(1) if m.group(1).startswith('\n## ') else ''), text, count=1)
        if n:
            doc.write_text(new_text)
            print(f'updated: {doc}')
            return
    # Insert before "## Security considerations" if present
    if '## Security considerations' in text:
        new_text = text.replace('## Security considerations', section + '\n## Security considerations')
        doc.write_text(new_text)
        print(f'inserted: {doc} (before Security considerations)')
        return
    # Otherwise append at end
    doc.write_text(text + '\n' + section)
    print(f'appended: {doc}')


def main():
    root = Path(__file__).resolve().parent.parent
    contracts = root / 'contracts'
    docs = root / 'docs'
    for c in contracts.iterdir():
        if not c.is_dir():
            continue
        src = c / 'src' / 'lib.rs'
        doc = docs / f"{c.name}.md"
        if not src.exists() or not doc.exists():
            continue
        src_text = src.read_text()
        variants = parse_datakey(src_text)
        variants_info = []
        for name, typ in variants:
            info = analyze_usage(src_text, name)
            variants_info.append((name, typ, info))
        section = build_section(c.name, variants_info)
        update_doc(doc, section)


if __name__ == '__main__':
    main()
