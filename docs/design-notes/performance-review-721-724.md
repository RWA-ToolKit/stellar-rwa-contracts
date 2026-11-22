# Performance review: #721, #722, #723, #724

Measured 2026-11-22 on soroban-sdk 26.1.0. "Native" numbers come from the
ordinary test environment, which does not execute Wasm. "Wasm" numbers run
the release `.wasm` builds in the same host via `env.register(wasm_bytes, ())`,
which includes VM instantiation. That is what dominates cross-contract calls
on chain.

## #721: compliance calls on `transfer`

**Before:** `transfer` made four cross-contract calls to compliance:
`is_allowed(from)`, `is_allowed(to)`, then `get_jurisdiction` for each side
to decide whether the jurisdiction cap applies.

**After:** one call, `compliance::check_transfer(from, to) -> (from_allowed,
to_allowed, same_jurisdiction)`. Compliance computes all three with the same
internal functions the separate entry points use, so the guarantee is
unchanged. `test_check_transfer_matches_the_individual_queries` pins that
equivalence, including a suspended recipient. Error precedence on `transfer`
is unchanged too: sender, then recipient, then balance, then cap.

| `transfer` (both parties allowlisted) | before | after | change |
|---|---:|---:|---:|
| Wasm, same jurisdiction | 3,271,597 | 1,813,908 | **−44.6%** |
| native, same jurisdiction | 481,507 | 411,852 | −14.5% |
| native, cross jurisdiction | 582,180 | 512,089 | −12.0% |

**Deployment order matters.** An asset token built from this version calls
`check_transfer`, which older compliance deployments do not have. Upgrade the
compliance contract first, then the asset token. A compliance contract with
`check_transfer` still serves older asset tokens, because the existing entry
points are unchanged.

`transfer_batch` and `mint` still use the individual calls. They make one
call per recipient, not two of the same kind, so the same merge does not apply
directly. A batched `check_recipients` is the natural follow-up if batch cost
matters.

## #722: `get_assets_by_issuer` (non-paginated)

**Decision: deprecate, keep working.** It already returned at most
`MAX_PAGE_SIZE` assets: the same first page as `get_assets_by_issuer_page(issuer,
0, MAX_PAGE_SIZE)`, with the same cost (`test_issuer_unpaginated_cap`:
3,530,439 vs 3,530,445 CPU for 50 rows). An unbounded version cannot exist,
because a call is limited to 100 footprint entries. Its doc comment claimed it
returned *all* assets, so an issuer with more than one page got a silently
truncated list. The doc now says so and points to the paginated getter, and
removal is left for the next major version (see CHANGELOG).

## #723: dividend claimed-state layout

**Audit result: no refactor.** Claimed state is not inside `Distribution`.
It is already one small entry per `(distribution_id, holder)`
(`DataKey::Claimed`), and the holder snapshot and supply have their own keys.
`claim` does rewrite the `Distribution` entry, because `distributed` and
`completed` change, but that struct is fixed-size:

| | bytes |
|---|---:|
| `Distribution` entry | 348 |
| everything one `claim` writes (5 entries: Distribution, Claimed flag, two payment-token balances, instance) | 1,104 |

Moving `distributed`/`completed` into their own key would save at most about
290 written bytes per claim. It would add a ledger entry with its own rent and
TTL to manage, and need a storage migration for existing deployments. That
isn't worth it for a struct that does not grow. `test_claim_storage_layout`
asserts the struct stays under 400 bytes, so a future change that embeds
per-holder state fails loudly.

## #724: `#[inline]` on hot helpers

Tried on the small helpers called from many entry points: `panic_err`,
`check_str`, `metadata`, `compliant`, `check_amount`, `set_balance`, `bump`,
`load`, `bump_instance`.

| Release `.wasm` size | no hint | `#[inline]` | `#[inline(always)]` |
|---|---:|---:|---:|
| asset_token | 57,235 | 57,235 | 59,274 (+3.6%) |
| compliance | 53,817 | 53,817 | 54,155 (+0.6%) |
| dividend | 45,755 | 45,755 | 46,511 (+1.7%) |
| registry | 43,836 | 43,836 | 44,091 (+0.6%) |

Wasm `transfer` CPU: 1,813,908 with no hint, 1,812,999 with `inline(always)`
(−0.05%, noise).

**Result: no annotations applied.** With `lto = true` and `codegen-units = 1`,
the compiler sees every helper and makes the same choices a plain `#[inline]`
asks for. The sizes are byte-for-byte identical. Forcing `inline(always)`
grows every contract for no measurable CPU gain. Revisit only if the release
profile drops LTO.
