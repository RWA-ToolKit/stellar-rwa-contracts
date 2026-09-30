# Fuzzing Guide

Issue #378 / Issue #433: Fuzzing for arithmetic-heavy paths

This directory contains fuzz targets for the RWA contracts, focusing on
arithmetic operations that handle i128 overflow/underflow and boundary
conditions.

> **Note (issue #433):** The previous version of this guide was broken.
> `fuzz/fuzz_targets/dividend_arithmetic.rs` did not exist; the `fuzz` crate
> pinned `soroban-sdk 21` while the workspace uses `soroban-sdk 26`; and
> `fuzz` was not a workspace member, so CI never caught the breakage.  All
> three issues are fixed in this revision.

## Building and Running Fuzz Tests

### Prerequisites

Install `cargo-fuzz`:
```bash
cargo install cargo-fuzz
```

`cargo fuzz` requires a nightly toolchain for the sanitizer flags it passes.
The workspace `rust-toolchain.toml` specifies the channel; if you have
`rustup` installed it will be set automatically.

### Running Fuzz Tests

Because `fuzz` is now a workspace member you can build it from the repo root:

```bash
# From the repo root — verify the fuzz crate compiles:
cargo build -p dividend-fuzz

# Run the dividend arithmetic fuzzer (from repo root):
cargo fuzz run --manifest-path fuzz/Cargo.toml fuzz_dividend_arithmetic

# Or equivalently from the fuzz/ subdirectory:
cd fuzz
cargo fuzz run fuzz_dividend_arithmetic
```

By default, `cargo fuzz run` runs indefinitely. To limit execution time or
input size:

```bash
cargo fuzz run fuzz_dividend_arithmetic -- -max_len=1024 -max_total_time=300
```

### What the fuzzer checks

The `fuzz_dividend_arithmetic` target replicates the on-chain
`claimable = total_amount * balance / supply` formula across random holder
distributions and asserts three invariants:

1. **Claim total bounds** — `sum(floor(total_amount * b_i / supply)) <= total_amount`
   for any distribution.
2. **Overflow detection** — when `total_amount * balance` would overflow
   i128 the target returns early (matching the contract's `ArithmeticOverflow`
   guard); it does **not** treat overflow as a crash.
3. **Dust behaviour** — `0 <= dust <= N - 1` where `N` is the number of
   holders and `dust = total_amount - sum(payouts)`.

### Interpreting Results

The fuzzer will:
1. Generate random distributions with varying holder balances.
2. Execute the arithmetic under the invariants above.
3. Report any assertion failures with a minimal reproducer.

### Filing Findings

If the fuzzer discovers a crash or invariant violation:
1. A crash file is saved to
   `fuzz/artifacts/fuzz_dividend_arithmetic/crash-*`
2. Create a new issue with:
   - The crash/violation description
   - The minimal reproducer (the crash file content)
   - Steps to reproduce
   - Expected vs. actual behaviour
