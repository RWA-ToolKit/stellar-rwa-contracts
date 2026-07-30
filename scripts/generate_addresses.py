#!/usr/bin/env python3
"""
Generate the addresses section in DEPLOYMENTS.md and update docs/*.md
from a deployments.json file to avoid drift between docs and deployments.

Usage: scripts/generate_addresses.py [path/to/deployments.json]
"""
from pathlib import Path
import json
import re
import sys
from datetime import date


def load_deployments(path: Path):
    if not path.exists():
        print(f"error: deployments file not found: {path}")
        sys.exit(1)
    with path.open() as f:
        return json.load(f)


def find_value(obj, candidates):
    # Try direct keys (case-insensitive), then startswith, then fallback
    lower = {k.lower(): k for k in obj.keys()}
    for c in candidates:
        if c.lower() in lower:
            return obj[lower[c.lower()]]
    # startswith
    for c in candidates:
        for k in obj.keys():
            if k.lower().startswith(c.lower()):
                return obj[k]
    return None


def make_table(entries, deployed_date):
    lines = []
    lines.append('## Testnet')
    lines.append('')
    lines.append('Network: `Test SDF Network ; September 2015`')
    lines.append(f'Deployed: {deployed_date}')
    lines.append('')
    lines.append('| Contract    | Contract ID                                                | Explorer |')
    lines.append('|-------------|------------------------------------------------------------|----------|')
    for name, cid in entries:
        if not cid:
            cid = '`<missing>`'
            link = ''
        else:
            cid = f'`{cid}`'
            link = f'[view](https://stellar.expert/explorer/testnet/contract/{cid.strip("`")})'
        lines.append(f'| {name:<11} | {cid:<58} | {link} |')
    lines.append('')
    lines.append('**Admin / issuer account:** `<admin>`')
    lines.append('')
    lines.append('### Sample asset')
    lines.append('')
    lines.append('The deployment script registers one sample asset for demonstration:')
    lines.append('')
    return "\n".join(lines)


def update_deployments_md(root: Path, table_md: str):
    md = root / 'DEPLOYMENTS.md'
    text = md.read_text()
    # Replace the section starting at ## Testnet up to the next header that starts with '## '
    new_text, n = re.subn(r"(?s)(## Testnet.*?)(\n## \w)", lambda m: table_md + m.group(2), text, count=1)
    if n == 0:
        # If we couldn't find the trailing header, try to replace from ## Testnet to end
        new_text, n = re.subn(r"(?s)## Testnet.*", table_md, text, count=1)
    if n == 0:
        print("warning: could not locate '## Testnet' section in DEPLOYMENTS.md; skipping update")
        return
    md.write_text(new_text)
    print(f"updated: {md}")


def update_docs(root: Path, mapping: dict):
    docs = (root / 'docs')
    for doc in docs.glob('*.md'):
        text = doc.read_text()
        # replace first occurrence of '- Testnet: `...`' with mapped contract id if available
        contract_key = None
        name = doc.stem  # e.g., 'asset-token' -> maps to asset-token
        # map file names to expected deployment keys
        if name == 'asset-token':
            contract_key = 'asset-token'
        else:
            contract_key = name
        # fallbacks: also try underscore versions
        cid = mapping.get(contract_key) or mapping.get(contract_key.replace('-', '_')) or mapping.get(contract_key.replace('-', ''))
        if not cid:
            # try scanning mapping for keys containing contract_key
            for k, v in mapping.items():
                if contract_key in k:
                    cid = v
                    break
        if not cid:
            print(f"skip {doc.name}: no id for {contract_key}")
            continue
        new_text, n = re.subn(r"^- Testnet: `[^`]+`", f"- Testnet: `{cid}`", text, count=1, flags=re.M)
        if n:
            doc.write_text(new_text)
            print(f"updated: {doc}")


def main():
    root = Path(__file__).resolve().parent.parent
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else root / 'deployments.json'
    data = load_deployments(path)

    # attempt to extract contract ids
    mapping = {}
    mapping['compliance'] = find_value(data, ['compliance', 'compliance_id'])
    mapping['registry'] = find_value(data, ['registry', 'registry_id'])
    mapping['dividend'] = find_value(data, ['dividend', 'dividend_id'])
    mapping['asset-token'] = find_value(data, ['asset-token', 'asset_token', 'asset', 'asset_id'])
    admin = find_value(data, ['admin', 'issuer', 'admin_address', 'issuer_address'])
    deployed = find_value(data, ['deployed', 'deployed_at', 'date']) or date.today().isoformat()

    entries = [
        ('compliance', mapping.get('compliance')),
        ('registry', mapping.get('registry')),
        ('dividend', mapping.get('dividend')),
        ('asset-token', mapping.get('asset-token')),
    ]

    table_md = make_table(entries, deployed)
    # replace placeholder admin if we found one
    if admin:
        table_md = table_md.replace('`<admin>`', admin)

    update_deployments_md(root, table_md)
    update_docs(root, mapping)


if __name__ == '__main__':
    main()
