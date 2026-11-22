# Resource budgets

Measured costs of the most frequently called contract functions (#720), so
integrators can estimate fees and reviewers can see when a change makes a hot
path more expensive.

Measured 2026-11-22 on soroban-sdk 26.1.0 in the native test environment
(`env.cost_estimate()`). Native tests do not meter Wasm execution, so on chain
CPU is higher. Treat these as relative numbers: compare a change against
them, and use simulation for an actual fee.

| Function | CPU instructions | Memory (bytes) | Ledger reads | Ledger writes | Test |
|---|---:|---:|---:|---:|---|
| `asset-token::transfer` | 481,507 | 171,147 | 13 | 3 | `test_resource_budget_transfer` |
| `asset-token::transfer_batch`, 1 recipient | 481,874 | 171,063 | 13 | 3 | `test_resource_budget_transfer_batch_marginal_cost` |
| `asset-token::transfer_batch`, 10 recipients | 2,686,809 | 812,898 | 40 | 12 | 〃 |
| `asset-token::transfer_batch`, 20 recipients | 6,174,356 | 1,864,628 | 70 | 22 | 〃 |
| `compliance::is_allowed` | 91,402 | 46,337 | 4 | 0 | `test_resource_budget_is_allowed` |
| `dividend::claim` | 512,331 | 193,935 | 12 | 5 | `test_resource_budget_claim` |
| `registry::register_asset` | 317,800 | 106,254 | 8 | 6 | `test_resource_budget_register_asset` |

**`transfer_batch` per recipient:** about 349,000 CPU per extra recipient
(between 10 and 20), and 3 ledger entries in the footprint. The binding limit is
the footprint, not CPU: a transaction may touch at most **100 ledger entries**
and write at most **50**. A 50-recipient batch fails (`total footprint ledger
entries: 212 > 100`), so keep batches to about **25 recipients** or fewer.

Reads include entries read from memory (the instance) as well as disk, as
`InvocationResources` reports them. A footprint counts each entry once,
however many times a call reads it.

## Before and after (#718, #719)

| Call | Before | After | Change |
|---|---:|---:|---:|
| `transfer_batch`, 10 recipients | 3,128,530 | 2,686,809 | −14% |
| `transfer_batch`, 20 recipients | 7,328,911 | 6,174,356 | −16% |
| `transfer_batch` marginal per recipient | 420,038 | 348,754 | −17% |
| `claim` | 543,218 | 512,331 | −6% |

## Keeping this current

Each row has a test that prints its numbers and asserts a generous upper
bound, so a large regression fails CI. When a PR changes one of these
functions it must:

1. run `cargo test --workspace resource_budget -- --nocapture --test-threads=1`,
2. update the row here with the new numbers and date, and
3. raise a test's bound only with a sentence in the PR on why the cost grew.
