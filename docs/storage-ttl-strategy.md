# Storage TTL and Bump Strategy

This document maps every storage key in the RWA contracts to its storage type and where its TTL is extended.

**Key takeaway:** Storage entries expire unless their TTL is extended before the threshold. Each table below lists which operations bump TTL for which keys, ensuring critical state survives.

---

## Security Audit: Admin-Critical TTL Extension (Issue #673)

This section records the security pass over every `extend_ttl` call site. The goal is not
performance but correctness: if an admin-critical entry (admin address, pending-admin
proposal, compliance contract address, oracle address) expires, reads may return a
default/zero value or trap, leaving the contract unusable or, worse, controllable by
whoever re-initializes it.

### Policy

1. **Admin-critical keys get the longest TTL.** They use `ADMIN_BUMP_AMOUNT`
   (`120 * DAY_IN_LEDGERS`, 120 days) with `ADMIN_LIFETIME_THRESHOLD`
   (`ADMIN_BUMP_AMOUNT - DAY_IN_LEDGERS`, 119 days). All other keys use the standard
   30-day window.
2. **Admin-critical keys are bumped on every privileged call, not just on read.**
   Every function gated by `require_admin` (or that writes an admin-critical key)
   extends the TTL of the admin-critical entries before returning.
3. **Bumps happen on write paths too.** A privileged call that only reads the admin
   address still bumps it, so a long series of read-only privileged calls cannot let
   the admin entry lapse.

### Call-site inventory

Every `extend_ttl` call site, per contract:

**Compliance**
- `initialize` — instance (`Admin`), `ADMIN_BUMP_AMOUNT`
- `set_pending_admin` — instance (`Admin`, `PendingAdmin`), `ADMIN_BUMP_AMOUNT`
- `accept_admin` — instance (`Admin`, `PendingAdmin`), `ADMIN_BUMP_AMOUNT`
- `set_compliance_contract` — instance (`ComplianceContract`), `ADMIN_BUMP_AMOUNT`
- `add_to_allowlist` / `remove_from_allowlist` — instance (`Admin`), `ADMIN_BUMP_AMOUNT`;
  persistent allowlist keys, standard window
- `record_approved_transfer` — instance (`Admin`), `ADMIN_BUMP_AMOUNT`; `Record(Address)`,
  standard window
- `block_jurisdiction` / `unblock_jurisdiction` — instance (`Admin`), `ADMIN_BUMP_AMOUNT`;
  `Blocked(String)`, standard window

**Asset Token**
- `initialize` — instance (`Metadata`, `Admin`), `ADMIN_BUMP_AMOUNT`
- `set_pending_admin` / `accept_admin` — instance (`Admin`, `PendingAdmin`), `ADMIN_BUMP_AMOUNT`
- `set_compliance_contract` — instance (`ComplianceContract`), `ADMIN_BUMP_AMOUNT`
- `transfer` / `mint` / `burn` / `approve` — instance (`Admin`), `ADMIN_BUMP_AMOUNT`;
  `Balance(Address)`, standard window

**Registry**
- `initialize` — instance (`Admin`), `ADMIN_BUMP_AMOUNT`
- `set_pending_admin` / `accept_admin` — instance (`Admin`, `PendingAdmin`), `ADMIN_BUMP_AMOUNT`
- `set_oracle` — instance (`Oracle`), `ADMIN_BUMP_AMOUNT`
- `register_asset` — instance (`Admin`, `Counter`, `Ids`), `ADMIN_BUMP_AMOUNT`;
  persistent index keys, standard window
- `update_valuation` — instance (`Admin`, `Oracle`), `ADMIN_BUMP_AMOUNT`;
  `Asset(u64)`, `TotalValuation`, standard window

**Dividend**
- `initialize` — instance (`Admin`), `ADMIN_BUMP_AMOUNT`
- `set_pending_admin` / `accept_admin` — instance (`Admin`, `PendingAdmin`), `ADMIN_BUMP_AMOUNT`
- `create_distribution` — instance (`Admin`, `Counter`, `Ids`), `ADMIN_BUMP_AMOUNT`;
  persistent distribution keys, standard window
- `claim` / `cancel_distribution` — instance (`Admin`), `ADMIN_BUMP_AMOUNT`;
  persistent distribution keys, standard window

### Findings

- ✅ `Admin`, `PendingAdmin`, `ComplianceContract`, and `Oracle` all use
  `ADMIN_BUMP_AMOUNT` and are bumped on every privileged call site listed above.
- ⚠️ **Fixed:** `set_pending_admin` in all four contracts previously bumped only the
  standard instance window, so a pending-admin proposal could expire before
  `accept_admin` was called. It now bumps with `ADMIN_BUMP_AMOUNT`.
- ⚠️ **Fixed:** `set_compliance_contract` (Compliance, Asset Token) and `set_oracle`
  (Registry) previously did not bump TTL at all. They now bump the affected
  admin-critical instance keys with `ADMIN_BUMP_AMOUNT`.

### Test coverage for this audit

`contracts/*/src/test.rs` contains `test_admin_ttl_extended_on_privileged_calls`, which
uses `env.ledger().set_sequence_number(...)` to advance the ledger past the standard
30-day window and asserts that the admin-critical entries (and the pending-admin
proposal) are still readable and that the TTL was extended to `ADMIN_BUMP_AMOUNT`.

---

## Compliance Contract

| Key | Type | Storage | Bump Sites | Notes |
|-----|------|---------|-----------|-------|
| `Admin` | instance | Bumped on: `initialize`, `set_pending_admin`, `accept_admin`, `set_compliance_contract`, `add_to_allowlist`, `remove_from_allowlist`, `record_approved_transfer`, `block_jurisdiction`, `unblock_jurisdiction` | Instance storage; shared with all instance keys | Admin address; admin-critical, `ADMIN_BUMP_AMOUNT` (120 days) |
| `PendingAdmin` | instance | Bumped on: `set_pending_admin`, `accept_admin` | Pending admin proposal | Admin-critical, `ADMIN_BUMP_AMOUNT` (120 days) |
| `ComplianceContract` | instance | Bumped on: `set_compliance_contract` | Compliance contract address | Admin-critical, `ADMIN_BUMP_AMOUNT` (120 days) |
| `AllowlistMeta` | persistent | Bumped on: `add_to_allowlist`, `remove_from_allowlist` | Tracks cursor for paginated allowlist | Updated on list changes |
| `AllowlistPage(u32)` | persistent | Bumped on: `add_to_allowlist`, `remove_from_allowlist` | Individual pages of KYC addresses | Bumped when page is modified |
| `AllowlistPageOf(Address)` | persistent | Bumped on: `add_to_allowlist`, `remove_from_allowlist` | Address → page mapping for O(1) removal | Bumped on membership change |
| `Record(Address)` | persistent | Bumped on: `add_to_allowlist`, `record_approved_transfer` | Compliance record (approval status, jurisdiction) | Bumped when record changes |
| `Blocked(String)` | persistent | Bumped on: `block_jurisdiction`, `unblock_jurisdiction` | Blocked jurisdiction set | Bumped on list changes |

**TTL Configuration:** `INSTANCE_BUMP_AMOUNT = 30 * DAY_IN_LEDGERS` (30 days), `INSTANCE_LIFETIME_THRESHOLD = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS` (29 days); admin-critical keys use `ADMIN_BUMP_AMOUNT = 120 * DAY_IN_LEDGERS` (120 days) with `ADMIN_LIFETIME_THRESHOLD = ADMIN_BUMP_AMOUNT - DAY_IN_LEDGERS` (119 days)

**Risk Assessment:** ✅ Safe — all entries bumped on modification; admin-critical keys bumped on every privileged call with the longest TTL

---

## Asset Token Contract

| Key | Type | Storage | Bump Sites | Notes |
|-----|------|---------|-----------|-------|
| `Metadata` | instance | Bumped on: `initialize` | Instance storage; shared | Token name, symbol, decimals; set once |
| `Admin` | instance | Bumped on: `initialize`, `set_pending_admin`, `accept_admin`, `set_compliance_contract`, `transfer`, `mint`, `burn`, `approve` | Instance storage; shared | Admin-critical, `ADMIN_BUMP_AMOUNT` (120 days) |
| `PendingAdmin` | instance | Bumped on: `set_pending_admin`, `accept_admin` | Pending admin proposal | Admin-critical, `ADMIN_BUMP_AMOUNT` (120 days) |
| `ComplianceContract` | instance | Bumped on: `set_compliance_contract` | Compliance contract address | Admin-critical, `ADMIN_BUMP_AMOUNT` (120 days) |
| `Balance(Address)` | persistent | Bumped on: `transfer`, `mint`, `burn`, `approve` | Per-account balance; critical | Bumped on every balance-modifying operation |

**TTL Configuration:** Standard keys 30 days; admin-critical keys `ADMIN_BUMP_AMOUNT` (120 days)

**Risk Assessment:** ✅ Safe — balances bumped on every transfer/mint/burn; admin-critical keys bumped on every privileged call

---

## Registry Contract

| Key | Type | Storage | Bump Sites | Notes |
|-----|------|---------|-----------|-------|
| `Admin` | instance | Bumped on: `initialize`, `set_pending_admin`, `accept_admin`, `set_oracle`, `register_asset`, `update_valuation` | Instance storage; shared | Admin-critical, `ADMIN_BUMP_AMOUNT` (120 days) |
| `PendingAdmin` | instance | Bumped on: `set_pending_admin`, `accept_admin` | Pending admin proposal | Admin-critical, `ADMIN_BUMP_AMOUNT` (120 days) |
| `Oracle` | instance | Bumped on: `set_oracle`, `update_valuation` | Oracle address for valuations | Admin-critical, `ADMIN_BUMP_AMOUNT` (120 days) |
| `Counter` | instance | Bumped on: `register_asset` | Tracks next asset ID | Updated when new asset registered |
| `Ids` | instance | Bumped on: `register_asset` | Global list of asset IDs | Updated when new asset registered |
| `Asset(u64)` | persistent | Bumped on: `register_asset`, `update_valuation` | Asset metadata and valuation | Bumped on registration and valuation updates |
| `ActiveCount` | persistent | Bumped on: `register_asset` | Count of active assets | Bumped on registration |
| `IssuerIndex(Address)` | persistent | Bumped on: `register_asset` | Issuer → asset IDs mapping | Bumped on registration |
| `TypeIndex(String)` | persistent | Bumped on: `register_asset` | Asset type → asset IDs mapping | Bumped on registration |
| `TotalValuation` | persistent | Bumped on: `update_valuation` | Sum of all asset valuations | Bumped on valuation changes |

**TTL Configuration:** Standard keys 30 days; admin-critical keys `ADMIN_BUMP_AMOUNT` (120 days)

**Risk Assessment:** ⚠️ **Watch** — `Counter` and `Ids` are instance-only; only bumped on registration. If no registrations occur within the TTL window, these could expire. However, this is acceptable because: (1) the contract is initialized once with a counter, and (2) registrations are expected to happen regularly. If the protocol goes dormant, re-registration is an acceptable recovery path. Admin-critical keys (`Admin`, `PendingAdmin`, `Oracle`) are not affected: they are bumped on every privileged call with the longest TTL.

---

## Dividend Contract

| Key | Type | Storage | Bump Sites | Notes |
|-----|------|---------|-----------|-------|
| `Admin` | instance | Bumped on: `initialize`, `set_pending_admin`, `accept_admin`, `create_distribution`, `claim`, `cancel_distribution` | Instance storage; shared | Admin-critical, `ADMIN_BUMP_AMOUNT` (120 days) |
| `PendingAdmin` | instance | Bumped on: `set_pending_admin`, `accept_admin` | Pending admin proposal | Admin-critical, `ADMIN_BUMP_AMOUNT` (120 days) |
| `Counter` | instance | Bumped on: `create_distribution` | Distribution ID counter | Incremented on distribution creation |
| `Ids` | instance | Bumped on: `create_distribution` | Global list of distribution IDs | Updated on distribution creation |
| `Dist(u64)` | persistent | Bumped on: `create_distribution`, `claim`, `cancel_distribution` | Distribution metadata | Bumped on create, claim, and cancel |
| `Claimed(u64, Address)` | persistent | Bumped on: `claim` | Per-(distribution, holder) claim flag | Bumped when holder claims |
| `AssetIds(Address)` | persistent | Bumped on: `create_distribution` | Asset token → distribution IDs mapping | Bumped on distribution creation |
| `Supply(u64)` | persistent | Bumped on: `create_distribution` | Snapshot supply for distribution | Set at creation, removed when distribution completes |
| `Snapshot(u64)` | persistent | Bumped on: `create_distribution` | Eligible holder list snapshot | Set at creation, removed when distribution completes |

**TTL Configuration:** Standard keys 30 days; admin-critical keys `ADMIN_BUMP_AMOUNT` (120 days)

**Risk Assessment:** ✅ Safe — all critical keys bumped on modification. `Supply` and `Snapshot` are removed when distribution completes (not just left to expire). Admin-critical keys are bumped on every privileged call with the longest TTL.

---

## Summary

### Universally Safe Keys
- Any key bumped on every modification (transfers, registrations, claims, etc.)
- Instance storage shared across all keys — bumped together
- Admin-critical keys (`Admin`, `PendingAdmin`, `ComplianceContract`, `Oracle`) — bumped on every privileged call with `ADMIN_BUMP_AMOUNT` (120 days)

### Keys Requiring Monitoring
- **Registry `Counter` and `Ids`:** Instance-only; bumped only on registration. Acceptable if registrations are frequent.
- **Dividend `Supply` and `Snapshot`:** Removed explicitly when distribution completes, not left to expire.

### Bump Window
- Standard window duration: 30 days
- Standard bump threshold: 29 days
- Admin-critical window duration: 120 days
- Admin-critical bump threshold: 119 days
- If a key is not accessed within its threshold, its TTL may expire before the next bump

### Test Coverage
See `contracts/*/src/test.rs` for tests that advance the ledger past TTL boundaries and verify state survives. Issue #371 adds ledger advance tests. Issue #673 adds `test_admin_ttl_extended_on_privileged_calls`, which advances the ledger past the standard 30-day window with `env.ledger().set_sequence_number(...)` and verifies admin-critical entries and pending-admin proposals survive and are extended to `ADMIN_BUMP_AMOUNT`.

---

## Storage Layout Safety (Issue #372)

Each contract uses a distinct `enum DataKey` with unique variant discriminants. Soroban serializes enum variants with discriminants, ensuring no two keys from different variants can collide. Within each contract:

- **Compliance:** 6 distinct variants
- **Asset-Token:** 2 distinct variants  
- **Registry:** 8 distinct variants
- **Dividend:** 8 distinct variants

All variants within each contract are unambiguous. Cross-contract collisions are impossible (different types).

**Conclusion:** Storage layout is safe from collisions.
