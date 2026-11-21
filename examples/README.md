# Stellar CLI examples

Copy-pasteable `stellar contract invoke` scripts for the four main workflows.
They target Testnet by default; set `NETWORK` to another configured network to
reuse them against a local network. Requires the Stellar CLI (>= 22) and a
funded identity (`rwa-admin` by default).

Deploy the contracts first (`NETWORK=testnet IDENTITY=rwa-admin ./scripts/deploy.sh`)
and export the printed ids:

```bash
export COMPLIANCE_ID=C... REGISTRY_ID=C... DIVIDEND_ID=C... ASSET_ID=C...
export INVESTOR_IDENTITY=alice INVESTOR_ADDR=G...   # stellar keys generate --network testnet --fund alice
export PAYMENT_TOKEN_ID=C...                        # a SAC/SEP-41 token for dividends
```

| Script | Workflow | Functions demonstrated |
|--------|----------|------------------------|
| `1-register-asset.sh` | Initialize and tokenize/register a new asset | `initialize`, `register_asset` |
| `2-kyc-investor.sh` | Approve an investor on compliance | `add_to_allowlist`, `add_to_allowlist_batch`, `is_allowed` |
| `3-transfer.sh` | Mint and transfer (single and batch) | `mint`, `transfer`, `transfer_batch`, `balance` |
| `4-distribute-and-claim.sh` | Fund a dividend distribution and claim | `create_distribution`, `claimable`, `claim`, `has_claimed` |

Complex arguments use JSON because that is how the CLI encodes Soroban
structs/tuples:

- `Vec<AllowlistEntry>` → `[{"address":"G...","jurisdiction":"US","expires_at":0}]`
- `Vec<(Address, i128)>` → `[["G...",1000]]`
- `expires_at: 0` and `deadline: 0` mean "no expiry" — see `docs/compliance.md`
  and `docs/dividend.md`.

Run from any directory, e.g. `./examples/2-kyc-investor.sh`.
