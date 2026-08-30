# Contributing

Thanks for helping build the Stellar RWA Toolkit contracts. This guide covers
local setup, testing, deployment, the compliance model, and how to extend it.

## Soroban local dev setup

1. Install Rust and the wasm target:
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   rustup target add wasm32v1-none
   ```
2. Install the Stellar CLI (includes Soroban):
   ```bash
   cargo install --locked stellar-cli
   stellar --version   # expect >= 22
   ```
3. Configure Testnet and an identity:
   ```bash
   stellar network add testnet \
     --rpc-url https://soroban-testnet.stellar.org \
     --network-passphrase "Test SDF Network ; September 2015"
   stellar keys generate --network testnet --fund rwa-admin
   ```

## Build and test

```bash
# compile all contracts to wasm
stellar contract build

# run the full workspace test suite (48 tests)
cargo test

# run a single contract's tests
cargo test -p compliance
```

> **Lockfile note:** the resolver may pull `ed25519-dalek 3.0.0`, which breaks
> `soroban-env-host`'s test utils. The committed `Cargo.lock` pins it to
> `2.2.0`. If you regenerate the lockfile, re-pin with:
> ```bash
> cargo update -p ed25519-dalek@3.0.0 --precise 2.2.0
> ```

## Deploy to Testnet

```bash
NETWORK=testnet IDENTITY=rwa-admin ./scripts/deploy.sh
```

This funds the identity, builds, deploys and initializes all four contracts,
approves the issuer on compliance, deploys a sample asset, and registers it.
Copy the printed contract ids into [DEPLOYMENTS.md](DEPLOYMENTS.md).

## The compliance model & cross-contract calls

The **compliance** contract is the source of truth for who may hold or transfer
an asset. It stores a `KycRecord` per address (status, jurisdiction, expiry) and
a set of blocked jurisdictions. Its `is_allowed(address) -> bool` returns `true`
only for `Approved`, non-expired, non-blocked addresses.

The **asset-token** contract never trusts the caller for compliance. On every
`transfer` it makes two **cross-contract calls**:

```rust
#[contractclient(name = "ComplianceClient")]
pub trait ComplianceInterface {
    fn is_allowed(env: Env, address: Address) -> bool;
}

let gate = ComplianceClient::new(&env, &meta.compliance_contract);
if !gate.is_allowed(&from) { /* SenderNotCompliant */ }
if !gate.is_allowed(&to)   { /* RecipientNotCompliant */ }
```

Using `#[contractclient]` on a trait means the token depends only on the
compliance *interface*, not the compliance *crate*. The concrete address lives
in the token's metadata and is swappable via `set_compliance`. The **dividend**
contract uses the same pattern to read balances/supply from the asset token and
to move payment tokens.

## How to add a new compliance rule

Compliance rules live entirely in the `compliance` contract; the asset token
needs **no changes** because it only calls `is_allowed`.

1. **Add state** if needed — extend the `DataKey` enum (e.g. a new
   `MinInvestment(Address)` key) and a setter guarded by `require_admin`.
2. **Enforce it in `is_allowed`** — add a check that returns `false` when the
   rule is violated. Keep `is_allowed` total (never panic).
3. **Emit an event** for the admin action so indexers can track it.
4. **Add tests** — a happy path plus a rejection path (see
   `contracts/compliance/src/test.rs` for the pattern, e.g.
   `test_block_jurisdiction_denies_approved`).
5. **Document** the rule and any new error code in `docs/compliance.md`.

Because the gate is a single boolean, arbitrarily complex policies compose
behind one interface without touching the token.

## Coding standards

- No `todo!()`, no `unwrap()`/`expect()` in contract code — use typed
  `#[contracterror]` variants via `panic_with_error!`.
- Every admin function calls `require_auth()` and verifies the caller is the
  stored admin.
- Emit events for all state changes.

## Pull request checklist

Every PR that touches `contracts/` opens with the checklist in
[`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md):
tests added, new/changed events documented, storage layout unchanged (or
`VERSION` bumped), generated docs (`scripts/generate_error_docs.py`,
`scripts/generate_storage_docs.py`) re-run if their source changed, and
`CHANGELOG.md` updated. CI also runs `cargo deny check` (see `deny.toml`) to
catch dependency advisories and license issues.

## Sister repos

- **Web app:** https://github.com/RWA-ToolKit/stellar-rwa-web
- **API + Docs:** https://github.com/RWA-ToolKit/stellar-rwa-api-docs
