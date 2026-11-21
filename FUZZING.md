# Fuzzing Guide

This directory contains fuzz targets for the RWA contracts. The targets focus on:

- Arithmetic paths that handle `i128` overflow/underflow and boundary conditions
  (issue #378: `fuzz_dividend_arithmetic`)
- Compliance contract edge cases across jurisdiction blocking, expiry handling,
  batch operations, and the minimum-holding-period rule (issue #456)
- Asset-token `transfer_batch` summed-amount arithmetic and per-recipient
  compliance gating, with edge-case amounts (issue #493:
  `asset_token_transfer_batch`)

## Lockfile

`fuzz/` is listed as a workspace member in the root `Cargo.toml`
(`members = ["contracts/*", "fuzz"]`), so it shares the single root
`Cargo.lock` file. There is no separate `fuzz/Cargo.lock`. Both the main
test suite and the fuzz targets always build against the same resolved
dependency versions; there is no drift risk between the two.

If you are evaluating whether to use `cargo-fuzz` (which historically set
up `fuzz/` as its own independent workspace), note that this repository's
fuzz targets run as ordinary Rust binaries instead, which lets them live
inside the shared workspace without a separate lockfile.

## Fuzz crate layout

```
fuzz/
  Cargo.toml
  fuzz_targets/
    compliance_batch_allowlist.rs     # batch operations with valid/invalid inputs
    compliance_expiry.rs              # expiry edge cases across ledger sequences
    compliance_jurisdiction_race.rs   # jurisdiction block + KYC approval interactions
    compliance_is_allowed.rs          # is_allowed with all reachable state combinations
    asset_token_transfer_batch.rs     # transfer_batch with edge-case amounts and compliance gating
```

The targets run as ordinary Rust binaries (`fn main`) using the `arbitrary`
crate for structured input generation. They are designed to be driven by a
fuzzing engine (AFL++, libFuzzer via `cargo-fuzz`) but can also be run as
deterministic smoke tests.

## Building and Running

### Prerequisites

```bash
cargo install cargo-fuzz
```

### Compliance fuzz targets (issue #456)

Build and run any compliance target:

```bash
cd fuzz
# replace <target> with one of the names below
cargo run --bin <target>
```

To run with libFuzzer (nightly only):
```bash
cargo +nightly fuzz run <target> -- -max_total_time=600
```

Available `<target>` names:

| Binary                          | What it covers                                        |
|---------------------------------|-------------------------------------------------------|
| `compliance_batch_allowlist`    | Batch add with random valid/invalid entries; counter invariants |
| `compliance_expiry`             | Off-by-one in expiry, sentinel `0`, integer overflow  |
| `compliance_jurisdiction_race`  | Block/unblock vs approve/suspend interaction; list invariants |
| `compliance_is_allowed`         | All branches of `is_allowed` including holding period |
| `asset_token_transfer_batch`    | `transfer_batch` amount checks, balance conservation, compliance gates, self-transfers |

Minimum recommended CI runtime: **10 minutes per target** (`-max_total_time=600`).

### Asset-token transfer_batch target (issue #493)

Models `AssetToken::transfer_batch` (`contracts/asset-token/src/lib.rs`) over an
in-memory ledger. Each run generates 100,000 random batches, including amounts
of `0`, `1`, `-1`, `i128::MIN`, `i128::MAX` and arbitrary `i128` values,
repeated recipients, self-transfers, and non-compliant recipients.

```bash
cd fuzz
cargo run --bin asset_token_transfer_batch           # default seed
cargo run --bin asset_token_transfer_batch -- 12345  # reproduce a specific seed
```

### Dividend arithmetic target (issue #378)

```bash
# From the repo root — verify the fuzz crate compiles:
cargo build -p dividend-fuzz

# Run the dividend arithmetic fuzzer (from repo root):
cargo fuzz run --manifest-path fuzz/Cargo.toml fuzz_dividend_arithmetic

# Or equivalently from the fuzz/ subdirectory:
cd fuzz
cargo run --bin fuzz_dividend_arithmetic
```

Or with libFuzzer:
```bash
cargo +nightly fuzz run fuzz_dividend_arithmetic -- -max_len=1000 -max_total_time=300
```

## Interpreting Results

Each target asserts one or more invariants (documented in the source file's
top-level doc comment). A violated invariant produces an `assert!` panic with
a descriptive message. A crash file is saved to
`artifacts/<target-name>/crash-*` when using `cargo fuzz`.

To replay a crash:
```bash
cargo +nightly fuzz run <target> artifacts/<target-name>/crash-<hash>
```

## Filing Findings

If the fuzzer discovers a crash or invariant violation:
1. Minimise the reproducer: `cargo fuzz tmin <target> artifacts/<target-name>/crash-<hash>`
2. Open a new issue with:
   - Target name and a description of the violated invariant
   - The minimised reproducer (attach the crash file)
   - Steps to reproduce (`cargo fuzz run ...`)
   - Expected vs actual behaviour
3. Fix the underlying bug before merging the PR that adds the reproducer to
   `fuzz/corpus/<target>/`.

## Current Coverage

| Target                         | Contract   | Invariants verified |
|--------------------------------|------------|---------------------|
| `fuzz_dividend_arithmetic`     | dividend   | claim total <= pool; overflow/underflow detection; dust |
| `compliance_batch_allowlist`   | compliance | counter == list len; atomic batch commit/revert |
| `compliance_expiry`            | compliance | expiry boundary semantics; sentinel `0` never lapses |
| `compliance_jurisdiction_race` | compliance | blocked jur always denies; blocked list <-> flag consistency |
| `compliance_is_allowed`        | compliance | correct boolean for all status x expiry x block x hold-period combinations |
| `asset_token_transfer_batch`   | asset-token | success implies positive amounts, compliant recipients, unpaused token; total supply conserved; no negative balances; self-transfer batches are no-ops |
