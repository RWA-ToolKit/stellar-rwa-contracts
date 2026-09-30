# Deployments

This file records the deployed contract ids for each network so that anyone
can audit a deployment against the repository. It is the single source of
truth: the web app and API repos link back here rather than maintaining their
own copies.

## Testnet

| Contract    | Id | Deploying commit | WASM sha256 |
| ----------- | -- | ---------------- | ----------- |
| compliance  | _TBD_ | _TBD_ | _TBD_ |
| registry    | _TBD_ | _TBD_ | _TBD_ |
| dividend    | _TBD_ | _TBD_ | _TBD_ |
| asset-token | _TBD_ | _TBD_ | _TBD_ |

> **Note for maintainers:** run `NETWORK=testnet IDENTITY=rwa-admin ./scripts/deploy.sh`
> to produce a fresh deployment, then fill in the ids, the git commit (`git rev-parse HEAD`),
> and the WASM sha256 (`sha256sum target/wasm32v1-none/release/<contract>.wasm`).
> Use `scripts/generate_addresses.py` to sync the ids to the sister repos.

Deployments are produced by `scripts/deploy.sh`; copy the printed ids into the
table above.

## Verifying a deployment

To confirm that a deployed contract matches a given commit of this repository,
use `scripts/verify_deployment.sh`. It checks out the requested commit in a
temporary worktree, builds the wasm, fetches the deployed wasm, and compares
their sha256 hashes.

```sh
# Verify a contract against the current HEAD
scripts/verify_deployment.sh HEAD <contract-id>

# Verify a contract against a specific commit or tag
scripts/verify_deployment.sh v1.2.3 <contract-id>

# Verify against a non-default network (third positional argument)
scripts/verify_deployment.sh v1.2.3 <contract-id> testnet
```

The script prints `MATCH` when the locally built wasm matches the deployed
contract and `MISMATCH` otherwise, exiting non-zero on a mismatch so it can be
used in CI or audit scripts.

Requirements: `stellar` CLI (>= 22), `git`, and `sha256sum` (or `shasum`).
