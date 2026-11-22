# Contributing

Thanks for helping build the Stellar RWA Toolkit contracts. This guide covers
local setup, testing, deployment, the compliance model, and how to extend it.

## Your First PR

A literal walkthrough for a small, self-contained "good first issue" (for
example a docs typo or a missing test). Every command below works on a clean
checkout; run them from your terminal in order.

1. **Fork** the upstream repository with the GitHub "Fork" button, or:
   ```bash
   gh repo fork RWA-ToolKit/stellar-rwa-contracts --clone=false
   ```
2. **Clone your fork** (replace `<your-user>` with your GitHub username) and
   show the toolchain selected by the committed `rust-toolchain.toml`. rustup
   reads that file automatically when you enter the directory:
   ```bash
   git clone git@github.com:<your-user>/stellar-rwa-contracts.git
   cd stellar-rwa-contracts
   rustup show
   ```
   If the `wasm32v1-none` target is not installed, `rustup show` tells you what
   is missing; install it with `rustup target add wasm32v1-none`.
3. **Verify the baseline before changing anything**:
   ```bash
   cargo test --workspace
   ```
   Baseline caveat: on a clean checkout of `main`, dependency resolution
   currently fails because the separate `fuzz` crate requires `arbitrary ^1.4`
   while `soroban-sdk 26` requires `~1.3.0`. This is a pre-existing issue;
   `--exclude rwa-fuzz` does not bypass Cargo's workspace-wide resolution.
   Until it is fixed separately, the command above cannot be verified on this
   checkout. The fuzz targets are built separately as described in
   [FUZZING.md](FUZZING.md).
4. **Make a branch** named `fix/<issue-number>-<short-topic>` (or
   `feat/<issue-number>-<short-topic>` for a feature):
   ```bash
   git switch -c fix/743-first-pr-walkthrough
   ```
5. **Make the smallest change that resolves the issue**, then check formatting
   and rerun the full suite. If you changed `contracts/**`, add a `CHANGELOG.md`
   entry under `## [Unreleased]` as described below:
   ```bash
   cargo fmt --all -- --check
   cargo test --workspace
   git diff --check
   ```
6. **Commit and push** to your fork:
   ```bash
   git add <files-you-changed>
   git commit -m "docs: add first-PR walkthrough (#743)"
   git push -u origin fix/743-first-pr-walkthrough
   ```
7. **Open the PR against the upstream repository**, not your fork. `main` is
   the base branch; the head is the branch on your fork:
   ```bash
   gh pr create --repo RWA-ToolKit/stellar-rwa-contracts \
     --base main --head <your-user>:fix/743-first-pr-walkthrough
   ```
   PR description expectations: state what changed and why, how you verified it
   (paste the commands you ran), link the issue with `Closes #<issue-number>`,
   and note the `CHANGELOG.md` entry when contract source changed. Keep one
   logical change per PR. There is currently no `.github/PULL_REQUEST_TEMPLATE.md`,
   so the body you write here is the template future reviewers will read.

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

## Pre-PR checks

Run the core local checks before opening a pull request:

```bash
git fetch upstream main
make check
```

`make check` runs the formatting check, workspace Clippy with warnings denied,
the workspace test suite, and the changelog check against `upstream/main`.
Ensure the `upstream/main` ref is current; for a different PR base, override it
with `make check BASE_REF=upstream/<branch>`. CI also runs platform-specific
build, interface-spec, fuzz, and script checks.

> **Lockfile note:** the resolver may pull `ed25519-dalek 3.0.0`, which breaks
> `soroban-env-host`'s test utils. The committed `Cargo.lock` pins it to
> `2.2.0`. If you regenerate the lockfile, re-pin with:
> ```bash
> cargo update -p ed25519-dalek@3.0.0 --precise 2.2.0
> ```

### Debugging contract panics (release-with-logs profile)

The standard `release` profile compiles panic diagnostics out. To see which
value triggered a contract error, build with the `release-with-logs` profile:

```bash
stellar contract build --profile release-with-logs
```

In this profile (`debug-assertions = true` in `Cargo.toml`), `asset-token`'s
`panic_err` helpers publish a `("panic_dbg", <offending value>)` diagnostic
event with the error variant as data immediately before panicking. Examples of
published values include `(amount, balance)` for `InsufficientBalance`, the
failing address for compliance failures, and `(min, max, len)` for an invalid
string. The event is `cfg(debug_assertions)`-gated, so ordinary release wasm
size and cost are unchanged. Note that a panicking Soroban invocation may be
rolled back by the host; treat these events as a local/debugging aid, not an
indexed production signal.

## Pre-commit hook (optional, recommended)

CI rejects unformatted code. To catch that at commit time instead of after a
push, enable the repo's hook, which runs `cargo fmt --check` and `cargo clippy`
when a commit touches Rust files:

```bash
git config core.hooksPath scripts/git-hooks
```

If you use the [pre-commit](https://pre-commit.com) framework, `.pre-commit-config.yaml`
runs the same checks: `pre-commit install`. Skip once with `git commit --no-verify`.

## Fuzzing

The fuzz targets live in `fuzz/fuzz_targets/` and are separate from the
workspace. Full details are in [FUZZING.md](FUZZING.md).

The batch allowlist target (issue #492) drives `add_to_allowlist_batch` with
generated batches of valid and invalid entries (jurisdiction and expiry) and
checks that a batch reverts as a whole, that the allowlist counter matches the
list, and that every admitted address passes `is_allowed`:

```bash
cd fuzz
cargo run --bin compliance_batch_allowlist
```

Other compliance targets (`compliance_expiry`, `compliance_jurisdiction_race`,
`compliance_is_allowed`) run the same way with their binary name. To add a
target, create a file under `fuzz/fuzz_targets/`, register it as a `[[bin]]` in
`fuzz/Cargo.toml`, and list it in [FUZZING.md](FUZZING.md).

## Contract interface specs

The build emits a machine-readable interface specification per contract
(`asset-token`, `compliance`, `dividend`, `registry`) so downstream consumers
can generate bindings instead of hand-maintaining their view of the interfaces.

The specs are committed in `specs/interface/<contract>.json`. CI regenerates
them from the built contracts and fails if the committed copies are stale, so
any change to a public function must come with regenerated specs:

```bash
# regenerate specs/interface/ after changing a public contract function
make update-interface-specs

# verify the committed specs match the built contracts (what CI runs)
make check-interface-specs
```

CI also attaches the freshly generated specs to every workflow run as the
`interface-specs` artifact, so you can download the exact interfaces for a
given commit from the run's **Artifacts** section.

### Consuming the specs

Both the web app and the API generate their bindings from the committed specs
rather than duplicating the interface definitions:

```bash
# fetch the specs from a CI run (or use specs/interface/ from the repo)
gh run download <run-id> -n interface-specs -D target/interface-specs

# generate TypeScript bindings for the web app
stellar contract bindings typescript \
  --wasm target/wasm32v1-none/release/asset_token.wasm \
  --output-dir ../stellar-rwa-web/src/bindings/asset-token
```

Regenerate the bindings whenever a spec changes and commit the result in the
consuming repo. Treat the specs as the source of truth for the contract
interfaces; do not edit generated bindings by hand.

## Deploy to Testnet

```bash
NETWORK=testnet IDENTITY=rwa-admin ./scripts/deploy.sh
```

This funds the identity, builds, deploys and initializes all four contracts,
approves the issuer on compliance, deploys a sample asset, and registers it.
Copy the printed contract ids into [DEPLOYMENTS.md](DEPLOYMENTS.md).

## Changelog convention

Every change that touches contract source (`contracts/**`) must come with a
`CHANGELOG.md` entry — updating the changelog is part of the definition of done.
CI flags any pull request that modifies `contracts/**` without also modifying
`CHANGELOG.md`.

Add your entry under the `## [Unreleased]` heading, in the appropriate
subsection (`Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`),
using the [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format:

```markdown
## [Unreleased]

### Added
- `compliance`: block transfers to sanctioned jurisdictions (#123)
```

**Concrete examples** — copy the pattern that fits your PR:

Bug-fix PR:

```markdown
## [Unreleased]

### Fixed
- `asset-token`: division-by-zero in `transfer_batch` when amount is zero (#456)
```

Feature PR:

```markdown
## [Unreleased]

### Added
- `compliance`: `add_to_allowlist_batch` for bulk KYC approvals in one transaction (#338)
```

Refactor / docs-only PR (no contract source change — CI skip):

```markdown
## [Unreleased]

### Changed
- `compliance`: split `lib.rs` into submodules for readability (#767)
```

Guidelines:

- One bullet per user-visible change; reference the issue or PR number.
- Prefix the bullet with the affected contract or area (e.g. `asset-token:`).
- Keep entries concise and written for integrators, not for the diff.
- When a release is cut, move the `Unreleased` entries under a new version
  heading with the release date.

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
contract uses the same pattern to read `total_supply()` from the asset token and
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

## Failure signaling convention (audit: November 2026)

All four contracts use the **panic-with-typed-error-code** convention for every
public (ABI-exposed) contract function. Failures are raised with
`soroban_sdk::panic_with_error!`, usually through each contract's local
`panic_err(env, Error::...)` helper (`asset-token` and `compliance` also have a
`check_str!` helper that panics on invalid input). Errors are declared as
`#[contracterror]` enums. SDK/client code should therefore detect failure from
the invocation result / error code, not by branching on a `Result` value.

Audit of all `#[contract]` public entry points:

| Contract | Public functions returning `Result<T, E>` | Failure convention |
| --- | --- | --- |
| `asset-token` | 0 | panic with `Error` code |
| `compliance` | 0 | panic with `Error` code |
| `dividend` | 0 | panic with `Error` code |
| `registry` | 0 | panic with `Error` code |

Findings: no public function in any of the four contracts returns
`Result<T, Error>`. `Option` is used only as a successful return value (for
example `status_of` and `get_pending_admin`) and is not a failure channel. The
only `Result` in contract support code is the private helper
`rwa_common::admin::accept`, whose `Option<Result<(Address, Address), ()>>`
models an internal "no proposal / wrong nominee / accepted" state; every caller
translates it into the same panic-based errors (`NoPendingAdmin`,
`Unauthorized`) before returning to the client. It is not part of any contract
ABI. No inconsistencies were found, so no follow-up issues are needed for this
audit.

When adding a function: return the value directly on success and call
`panic_err!`/`panic_with_error!` with the appropriate `#[contracterror]` variant
on failure. Do not introduce `Result` returns in the public interface; if a new
internal helper needs a different shape, keep it private and map it to a typed
panic at the ABI boundary. State-changing operations are atomic: a panic reverts
the whole invocation (including events), which is why batch operations such as
`transfer_batch`, `mint_batch`, and `add_to_allowlist_batch` validate every
entry and revert as a unit.

## Resource cost tests

Paginated and loop-heavy functions can pass every correctness test and still
become unusable if they start consuming far more CPU or memory than the
network allows. The registry contract guards against that with **resource cost
tests**: tests that call a function and assert on the Soroban resource budget
it consumed. Existing examples:

- `contracts/registry/src/test_assets_by_type_page_resource_cost.rs`
- `contracts/registry/src/test_ochedev6789_issuer_page_resource_cost.rs`

The pattern is:

1. **Reset the budget** so the measurement covers only the call under test:
   ```rust
   env.cost_estimate().budget().reset_default();
   ```
2. **Call the function** exactly as a client would (for example
   `client.get_assets_by_type_page(&issuer, &asset_type, &start, &limit)`).
3. **Read the observed costs**:
   ```rust
   let cpu = env.cost_estimate().budget().cpu_instruction_cost();
   let mem = env.cost_estimate().budget().memory_bytes_cost();
   ```
4. **Print them** (`std::println!` or the test logger) so a failing run shows
   the actual numbers and reviewers can see how close the call is to the limit.
5. **Assert against generous ceilings** — either a comfortable multiple of the
   expected cost or the Soroban per-transaction hard limits. The goal is to
   catch order-of-magnitude regressions (an accidental O(n²) scan, an unbounded
   loop, a missing page limit), not to pin exact numbers. Tight assertions turn
   every unrelated SDK or compiler bump into a failing test.

Add a resource cost test when you:

- introduce a new paginated function (anything taking a `start`/`limit` pair or
  returning a page of results),
- add or change a loop that iterates over stored entries, or
- change the data layout or iteration order of an existing paginated function.

Keep the test next to the contract's other tests, name it
`test_<function>_resource_cost.rs`, and use a realistic worst case (a full
page, or the largest batch the function accepts) so the ceiling reflects the
most expensive legitimate call.

## Coding standards

- No `todo!()`, no `unwrap()`/`expect()` in contract code — use typed
  `#[contracterror]` variants via `panic_with_error!`.
- Every admin function calls `require_auth()` and verifies the caller is the
  stored admin.
- Emit events for all state changes.

## Sister repos

- **Web app:** https://github.com/RWA-ToolKit/stellar-rwa-web
- **API + Docs:** https://github.com/RWA-ToolKit/stellar-rwa-api-docs
