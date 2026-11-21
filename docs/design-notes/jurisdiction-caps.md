# Design note: graduated per-jurisdiction holding caps (issue #737)

## Problem

`compliance::block_jurisdiction` is binary: a jurisdiction is either fully
allowed or fully denied. Securities regulations often instead impose a
graduated cap (for example, holders in one jurisdiction may not collectively
own more than 20% of the issue).

## Interface

The compliance contract owns the *rules*; the asset token owns the *balances*:

* `compliance::set_jurisdiction_cap(env, admin, jurisdiction, max_bps: Option<u32>)`
  stores a cap in basis points of total supply (`0..=10_000`). `None` clears it.
  Caps are independent of the binary blocked-jurisdiction set.
* `compliance::get_jurisdiction_cap(jurisdiction) -> Option<u32>` and
  `compliance::get_jurisdiction(address) -> Option<String>` are read-only
  getters used by the token.
* `asset-token::transfer`, `transfer_batch`, `transfer_from`, `mint` and
  `mint_batch` reject an operation with `JurisdictionCapExceeded (#15)` when it
  would push the recipient jurisdiction's aggregate holdings above the cap.

## Why this split

The compliance contract has no view of token balances or total supply, so it
cannot evaluate a cap on its own. Conversely the token does not own KYC
records. Rather than pushing balance snapshots into compliance (which would
require cross-contract authorization and risk stale state), the token
computes the aggregate read-only: it walks `compliance::get_allowlist()`, asks
for each holder's jurisdiction, and sums its *own* stored balances for the
jurisdiction being checked. `total_supply` comes from the token's metadata.
No compliance state is written by a token call.

Costs scale with the size of the allowlist and the number of cross-contract
reads; this is acceptable for the current KYC allowlist sizes but should be
revisited with an indexed aggregate if the allowlist grows into the thousands.

## Edge cases

* Transfers within the same jurisdiction (including self-transfers) do not
  change the aggregate and are exempt.
* Mint checks the *post-mint* supply; burn only decreases holdings and is not
  capped.
* A holder without a KYC record is already rejected by `is_allowed`; the cap
  check is a no-op for them.
* Raising/lowering a cap never retroactively confiscates or freezes balances;
  it only gates increases (see issue #738 for partial freezes).
