#!/usr/bin/env python3
"""Bulk-import KYC allowlist entries from a CSV.

Each row must have the columns ``address,jurisdiction,expiry``:

* ``address``     - a Stellar G... account address.
* ``jurisdiction`` - ISO-3166-1 alpha-2 country code (case-insensitive).
* ``expiry``      - ledger sequence at which approval expires; 0 never expires.

Rows are submitted in batches of ``--batch-size`` through the compliance
contract's ``add_to_allowlist_batch`` entry point using the Stellar CLI.
"""

import argparse
import csv
import json
import subprocess
import sys

ALLOWLIST_HEADER = ["address", "jurisdiction", "expiry"]


def read_entries(path):
    entries = []
    with open(path, newline="", encoding="utf-8-sig") as handle:
        reader = csv.DictReader(handle)
        if reader.fieldnames is None:
            raise ValueError("CSV is empty")
        header = [name.strip().lower() for name in reader.fieldnames]
        if header != ALLOWLIST_HEADER:
            raise ValueError(
                f"expected header {','.join(ALLOWLIST_HEADER)}, got {','.join(header)}"
            )
        for line, row in enumerate(reader, start=2):
            address = (row["address"] or "").strip()
            jurisdiction = (row["jurisdiction"] or "").strip().upper()
            expiry = (row["expiry"] or "").strip()
            if not address or not jurisdiction or not expiry:
                raise ValueError(f"line {line}: all columns are required")
            if len(jurisdiction) != 2 or not jurisdiction.isalpha():
                raise ValueError(f"line {line}: jurisdiction must be a 2-letter code")
            try:
                expires_at = int(expiry)
            except ValueError as exc:
                raise ValueError(f"line {line}: expiry must be an integer") from exc
            if expires_at < 0:
                raise ValueError(f"line {line}: expiry must be >= 0")
            entries.append(
                {
                    "address": address,
                    "jurisdiction": jurisdiction,
                    "expires_at": expires_at,
                }
            )
    if not entries:
        raise ValueError("CSV contains no data rows")
    return entries


def chunks(items, size):
    for index in range(0, len(items), size):
        yield items[index : index + size]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("csv_file", help="CSV file with address,jurisdiction,expiry rows")
    parser.add_argument("--contract-id", required=True, help="compliance contract ID")
    parser.add_argument("--admin", help="admin address argument (defaults to --source)")
    parser.add_argument("--source", default="admin", help="CLI source/identity (default: admin)")
    parser.add_argument("--network", default="testnet", help="network (default: testnet)")
    parser.add_argument("--batch-size", type=int, default=50, help="entries per transaction")
    parser.add_argument(
        "--dry-run", action="store_true", help="print the CLI commands without executing them"
    )
    args = parser.parse_args()

    if args.batch_size < 1:
        parser.error("--batch-size must be >= 1")

    try:
        entries = read_entries(args.csv_file)
    except (OSError, ValueError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1

    admin = args.admin or args.source
    for batch in chunks(entries, args.batch_size):
        command = [
            "stellar",
            "contract",
            "invoke",
            "--id",
            args.contract_id,
            "--source",
            args.source,
            "--network",
            args.network,
            "--",
            "add_to_allowlist_batch",
            "--admin",
            admin,
            "--entries",
            json.dumps(batch, separators=(",", ":")),
        ]
        print(" ".join(command))
        if not args.dry_run:
            subprocess.run(command, check=True)

    print(f"submitted {len(entries)} entries", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
