# Storage Layout & TTL Strategy

Soroban has two storage tiers per contract: **instance** (bundled with the
contract instance itself, cheap to read/write, one combined TTL for
everything in it) and **persistent** (per-key, individually priced and
individually TTL'd, and — critically — **archived if its TTL lapses**,
after which it can no longer be read without first being restored). This
page lists every `DataKey` variant in every contract, which tier it lives
in, whether/where its TTL gets extended, and the resulting archival risk —
verified against each contract's actual `storage()` calls, not assumed from
naming.

## asset-token

| Key | Tier | Extended where | Archival risk |
|---|---|---|---|
| `Metadata` | instance | Every state-changing call ends with `Self::bump(&env)`, which extends the *whole instance* | Low — touched on every write |
| `Balance(Address)` | persistent | **Nowhere.** `set_balance()` writes it with `.set()` but no call site in this contract ever calls `.extend_ttl()` on a `Balance` key | **High.** A holder whose balance was last written long enough ago that its persistent TTL lapses will have that entry **archived**. Reading it (`balance()`) then fails until it's explicitly restored (an operation outside this contract's own API) — even though the holder did nothing wrong and the token itself is otherwise healthy. Every other persistent key in every other contract in this workspace does get its TTL extended on read or write; `Balance` here is the one exception. |

`Metadata`'s per-write bump means the *contract-level* TTL strategy looks
healthy; the real risk is entirely in the per-holder `Balance` entries,
which can go stale independently of how active the token as a whole is.

## compliance

| Key | Tier | Extended where | Archival risk |
|---|---|---|---|
| `Admin` | instance | `Self::bump_instance` on every write path | Low |
| `Allowlist` | instance | Same — one `Vec<Address>` for the whole allowlist, rewritten (and thus its containing instance TTL bumped) on every `add_to_allowlist`/`suspend`/`remove`/`prune_expired` | Low on TTL, but see the size risk below |
| `Record(Address)` | persistent | **Not extended anywhere.** `add_to_allowlist` writes it with `.set()`; `get_record`/`is_allowed` only `.get()` it — no `.extend_ttl()` call exists for this key at all | **High**, same failure mode as `asset-token`'s `Balance`: a KYC record that isn't touched by a fresh `add_to_allowlist` call can be archived, at which point `is_allowed` reading `None` back (rather than an actually-expired-but-present record) makes the address fail compliance the same way an explicitly-rejected one would — indistinguishable from the outside without checking `get_record`'s presence directly. |
| `Blocked(String)` | persistent | Not extended | Same category of risk, lower practical impact since blocked-jurisdiction lists change rarely |

**The allowlist size limit:** `Allowlist` is a **single instance-storage
entry** holding one `Vec<Address>` — every address ever approved (until
`remove`d or pruned by `prune_expired`), all read and rewritten in full on
every `add_to_allowlist`/`suspend`/`remove` call. Soroban instance storage
has a per-contract-instance size ceiling; unlike `Record`, which is one
persistent entry per address (so it scales with how many addresses exist,
each independently priced), `Allowlist` scales as a *single* growing entry
whose read/write cost rises with total approved-address count. There is no
pagination or sharding here — for a platform expecting a large holder base,
this is the practical ceiling on how many distinct addresses this compliance
contract can track before individual `add_to_allowlist` calls become
noticeably more expensive, well before the KYC-approval logic itself is
wrong.

## dividend

| Key | Tier | Extended where | Archival risk |
|---|---|---|---|
| `Admin` | instance | `bump()` on every write path | Low |
| `Counter` | instance | Same | Low |
| `Ids` | instance | **Written nowhere in the current implementation** — declared but unused | N/A |
| `Dist(u64)` | persistent | `.extend_ttl()` on both `create_distribution` and every `load()` (i.e. every read via `get_distribution`/`claimable`/`claim`) | Low — refreshed on every read, not just every write |
| `Claimed(u64, Address)` | persistent | **Not extended anywhere** after being set in `claim` | High for the same reason as `asset-token`'s `Balance` — an old claimed-flag could in principle be archived, though the practical impact is limited since `has_claimed` returning "not found" (`false`) after archival happens to match the pre-claim state rather than silently allowing a double-claim |
| `AssetIds(Address)` | persistent | `.extend_ttl()` on write in `create_distribution` and on every read in `get_distributions_for_asset` | Low |

## registry

| Key | Tier | Extended where | Archival risk |
|---|---|---|---|
| `Admin` | instance | `bump()` on every write path | Low |
| `Counter` | instance | Same | Low |
| `ActiveCount` | instance | Same | Low |
| `Ids` | instance | **Written nowhere in the current implementation** — declared but unused (the crate's own comment notes it's kept only for forward-compatibility reads of already-deployed contracts) | N/A |
| `Asset(u64)` | persistent | `.extend_ttl()` on write in `register_asset`/`deactivate_asset` and on every read (`get_asset`, and inside `iter_assets`, which backs `get_assets_by_issuer`/`get_assets_by_type`/`get_all_assets`/`total_value_locked`) | Low — the most-read key in the contract, refreshed constantly |

## Summary: what actually needs attention

Two concrete gaps, both the same shape — a persistent key written once and
never touched again by any `extend_ttl` call, unlike every other persistent
key in this workspace which gets refreshed on read and/or write:

1. **`asset-token::Balance(Address)`** — the highest-impact one, since an
   archived balance makes a holder's tokens unreadable/untransferable until
   restored.
2. **`compliance::Record(Address)`** — lower blast radius (an archived
   record just reads as "not approved," which is at least a safe failure
   direction), but still a correctness gap worth closing the same way.

Both would be fixed the same way the other keys already are: add an
`extend_ttl` call alongside each `.set()`/`.get()` for that key.
