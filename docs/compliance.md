# Compliance Contract

The compliance contract is the **core differentiator** of the RWA Toolkit. It
maintains the KYC allowlist and jurisdiction rules that decide who may hold or
transfer a tokenized asset. The asset-token contract calls
[`is_allowed`](#is_allowed---bool) on **both** parties of every transfer and on
the recipient of every mint.

- Testnet: `CBUERYDM7DXTZLLKDBRJKUBPFJ7M4OSUN4T7XKUARU345RLXNAIQD2IU`

## Types

### `ComplianceStatus`

```rust
enum ComplianceStatus { Approved, Pending, Rejected, Suspended }
```

`Pending` and `Rejected` are retained only for ABI compatibility and are
explicitly deprecated. The current contract never writes them, and no public
method can create them; `Approved` and `Suspended` are the only reachable
states from the supported workflows. Only `Approved` records (that are not
expired and not in a blocked jurisdiction) pass `is_allowed`.

### `KycRecord`

| Field         | Type              | Meaning                                             |
|---------------|-------------------|-----------------------------------------------------|
| `address`     | `Address`         | The account this record applies to                  |
| `status`      | `ComplianceStatus`| Approval state                                      |
| `jurisdiction`| `String`          | Canonical ISO-3166-1 alpha-2 country code, e.g. `"US"` |
| `verified_at` | `u32`             | Ledger sequence at verification                     |
| `expires_at`  | `u32`             | Ledger sequence of expiry; `0` = never expires      |

> Time is measured in **ledger sequence numbers**, not calendar dates.

### Jurisdiction canonical form

To prevent "USA", "us", and "US" from being silently treated as three
different jurisdictions, every jurisdiction string passed to
`add_to_allowlist`, `block_jurisdiction`, `unblock_jurisdiction`, and
`is_jurisdiction_blocked` is normalized and validated before use:

- Interior/leading/trailing ASCII spaces are stripped.
- Letters are upper-cased.
- The result **must** be exactly 2 ASCII alphabetic characters (a real
  ISO-3166-1 alpha-2 code such as `US`, `KE`, `DE`).

Anything else — 1 or 3+ letters, digits, punctuation, or an empty string —
panics with `InvalidJurisdiction (#6)`. Stored records and blocked-jurisdiction
keys always hold the canonical 2-letter uppercase form, so lookups by any
casing/whitespace variant of the same code resolve to the same jurisdiction.
> **The `expires_at = 0` sentinel.** `0` means the approval **never
> expires**. This is load-bearing: the web app passes `0` for
> non-expiring approvals, and `is_allowed` skips the expiry comparison
> entirely whenever `expires_at == 0`, so a zero-expiry record can never
> lapse no matter how far the ledger advances. `0` is never a real expiry
> ledger sequence, so there is no collision with genuinely-expired records.

## Functions

### `initialize(admin: Address)`
Sets the admin. Callable once. Requires `admin` auth.
Errors: `AlreadyInitialized (#1)`.

### `add_to_allowlist(admin, address, jurisdiction, expires_at)`
Approves `address` (status → `Approved`) in `jurisdiction`, expiring at
`expires_at` (`0` = never). Admin only.
Errors: `Unauthorized (#5)`, `InvalidExpiry (#4)` if `expires_at` is already in
the past.

### `suspend(admin, address)`
Sets an existing record to `Suspended`; `is_allowed` returns `false` until
re-approved or reinstated. Admin only. Errors: `RecordNotFound (#3)`,
`Unauthorized (#5)`.

### `reinstate(admin, address)`
Restores a `Suspended` record to `Approved` **without discarding its
original KYC metadata** — `jurisdiction`, `verified_at`, and `expires_at`
are left untouched, unlike calling `add_to_allowlist` again (which requires
resupplying those fields and overwrites `verified_at`). Admin only.
Errors: `RecordNotFound (#3)`, `NotSuspended (#8)` if the record is not
currently `Suspended`.

### `remove(admin, address)`
Deletes a record and removes the address from the allowlist. Admin only.
Errors: `RecordNotFound (#3)`, `Unauthorized (#5)`.

### `is_allowed(address) -> bool`
The gate used by the asset token. Returns `true` **iff** the address has a
record that is `Approved`, not expired, and whose jurisdiction is not blocked.
Never panics.

### `get_record(address) -> Option<KycRecord>`
Raw record, or `None`.

### `get_allowlist() -> Vec<Address>`
Every address currently on the allowlist. Costs scale with the size of the
list — prefer `get_allowlist_count` when the caller does not need the full
list.

### `get_allowlist_count() -> u32`
Number of addresses currently on the allowlist, in O(1). Backed by a
counter maintained on every append/removal (not a walk over `get_allowlist`),
so a caller that only needs a count never pays to transfer every address.
The counter is unaffected by `suspend`, since that does not add or remove
page membership — it always matches `get_allowlist().len()`.

### `get_allowlist_page(offset: u32, limit: u32) -> Vec<Address>`
Pages through the allowlist. `offset` skips that many addresses from the
start; `limit` is clamped to `MAX_ALLOWLIST_PAGE_SIZE` (200) — pass `0` or
anything above the max to get the max page size back. An `offset` at or past
the end of the list returns an empty `Vec`, which signals the final page has
already been consumed.

### `block_jurisdiction(admin, jurisdiction)` / `unblock_jurisdiction(admin, jurisdiction)`
Block/unblock an entire country code. Approved addresses in a blocked
jurisdiction fail `is_allowed`. Admin only.

### `is_jurisdiction_blocked(jurisdiction) -> bool`
Whether a jurisdiction is currently blocked.

### `get_blocked_jurisdictions() -> Vec<String>`
Every jurisdiction currently blocked, in the order it was first blocked.
Previously the web app had to *infer* the blocked set from which
jurisdictions had no approved KYC records — a jurisdiction blocked before it
ever had an approved address was invisible that way. This reads the
contract's authoritative blocked set directly, so that inference workaround
is no longer needed.

### `get_admin() -> Address`
The configured admin. Errors: `NotInitialized (#2)`.

### `propose_admin(admin, new_admin)` / `accept_admin(new_admin)` / `cancel_admin_proposal(admin)`
Two-step admin handover: the **current** admin proposes a successor, and the
role moves only when that successor calls `accept_admin` (proving it controls
the address). The current admin can cancel a pending proposal. See
[issue #4](fixes/issue-4.md).

## Errors

| Code | Name                | Cause                                   |
|------|---------------------|-----------------------------------------|
| 1    | AlreadyInitialized  | `initialize` called twice               |
| 2    | NotInitialized      | Used before `initialize`                |
| 3    | RecordNotFound      | Operating on a missing record           |
| 4    | InvalidExpiry       | `expires_at` already in the past        |
| 5    | Unauthorized        | Caller is not the stored admin          |
| 6    | InvalidJurisdiction | Jurisdiction is not 2 ASCII letters after normalization |
| 7    | NoPendingAdmin      | `accept_admin`/`cancel_admin_proposal` with no pending proposal |
| 8    | NotSuspended        | `reinstate` called on a non-`Suspended` record |

## Events

| Topic        | Data                          | When                       |
|--------------|-------------------------------|----------------------------|
| `init`       | admin address                 | on initialize              |
| `approved`   | (address) → (jurisdiction, expires_at) | address approved  |
| `suspend`    | (address)                     | address suspended          |
| `reinstat`   | (address)                     | suspended address reinstated |
| `removed`    | (address)                     | address removed            |
| `blockjur`   | jurisdiction                  | jurisdiction blocked       |
| `unblkjur`   | jurisdiction                  | jurisdiction unblocked     |
| `set_admin`  | (old_admin) → new_admin       | admin handed over          |

## Storage / TTL

Listing of the contract `DataKey` variants and their storage behaviour.

| Key | Payload | Storage | TTL / Notes |
|-----|---------|---------|-------------|
| `Admin` | - | instance | - |
| `Allowlist` | - | instance | - |
| `Record` | Address | persistent | per-key TTL |
| `Blocked` | String | persistent | existence flag per jurisdiction |
| `BlockedList` | - | instance | ordered `Vec<String>` backing `get_blocked_jurisdictions` |

## Admin independence from the asset-token admin (issue #3)

The admin stored by this contract is entirely separate from the `admin`
stored by any asset-token contract that points at it as its
`compliance_contract`. The two are never compared, and neither contract reads
the other's admin — the asset token only ever calls the read-only
`is_allowed`.

[`scripts/deploy.sh`](../scripts/deploy.sh) initializes both contracts with
the same address (`$ADMIN_ADDR`). That is a **convenience default** for
standing up a single-operator demo/testnet deployment in one command, **not**
a requirement of the contracts. A real issuer can run compliance under a
dedicated compliance officer's address while a different address administers
the asset token, simply by calling `initialize` on each contract with a
different admin, e.g.:

```bash
invoke "$COMPLIANCE_ID" initialize --admin "$COMPLIANCE_OFFICER_ADDR"
invoke "$ASSET_ID" initialize --admin "$ISSUER_ADDR" ...
```

`contracts/asset-token/src/test.rs::test_compliance_admin_diverges_from_asset_admin`
proves this: it initializes the two contracts with distinct admins, shows the
compliance officer can independently approve/suspend addresses and the asset
admin can independently mint/pause, and confirms neither admin can act on the
other's contract.

## Security considerations

- All state-changing functions require the **stored admin** to both authorize
  (`require_auth`) and match the passed `admin` argument.
- `is_allowed` is intentionally total (never panics) so the asset token can rely
  on it inside transfers.
- Expiry is enforced against `env.ledger().sequence()` at read time, so an
  expired KYC blocks transfers without any keeper/cron.
