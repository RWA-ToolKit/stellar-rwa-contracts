# Soroban SDK Upgrade Plan (issue #752)

## Current state

| Component | Pinned version |
|-----------|---------------|
| `soroban-sdk` workspace pin | `26` |
| `ed25519-dalek` hard-pin | `= 2.2` (soroban-env-host testutils compat) |
| `rust-toolchain.toml` channel | see file |

## Watching for new releases

1. Subscribe to [`stellar/rs-soroban-sdk` releases](https://github.com/stellar/rs-soroban-sdk/releases) on GitHub (Watch → Custom → Releases).
2. Watch the [Stellar Developer Discord #soroban-dev channel](https://discord.gg/stellardev) for pre-release announcements.
3. Review the SDK changelog before starting an upgrade: `CHANGELOG.md` in the SDK repo lists breaking host-interface changes, storage API changes, and testutils additions.

## Upgrade checklist

Work through this list top-to-bottom for every SDK major-version bump.

### 1. Create a tracking issue

Open an issue titled `"Upgrade soroban-sdk to vXX"` referencing this document. All work goes in a branch off `main`.

### 2. Update `Cargo.toml`

```toml
# workspace Cargo.toml
[workspace.dependencies]
soroban-sdk = "XX"   # bump here
```

Then run:

```bash
cargo update
```

Re-pin `ed25519-dalek` if the new SDK still requires it:

```bash
cargo update -p ed25519-dalek --precise 2.2.0
```

Check whether the new SDK ships its own compatible dalek; if so, drop the pin.

### 3. Fix compiler errors

Address any renamed types, moved modules, or removed functions the compiler reports. Consult the SDK `CHANGELOG.md` for the migration path.

### 4. Run the full test suite

```bash
cargo test --workspace
```

All tests must pass with no behaviour change.

### 5. Re-run fuzz smoke tests

```bash
cd fuzz
cargo run --bin compliance_batch_allowlist -- -max_total_time=60
# repeat for other fuzz targets
```

No new crashes or panics should appear.

### 6. Check wasm binary sizes

```bash
stellar contract build
ls -lh target/wasm32-unknown-unknown/release/*.wasm
```

Compare sizes against the previous baseline. A significant increase (>10%)
warrants investigation before merging.

### 7. Re-verify resource-cost test assertions

Some tests assert specific ledger resource costs (CPU instructions, memory, bytes read/written). Re-run them and update any assertions that changed due to SDK host-function cost table updates. Document the delta in the PR description.

### 8. Update `CHANGELOG.md`

Add an entry under `## Unreleased` documenting the SDK version bump as a dependency change (not a breaking change unless the contracts' external ABI changed).

### 9. Open a PR and request review

The PR title should be `"chore: upgrade soroban-sdk to vXX"`. Paste the full checklist output into the PR description so reviewers can verify each step was completed.

## Version history

| SDK version | Date upgraded | PR | Notes |
|-------------|---------------|----|-------|
| 26 | (initial) | — | Baseline |
