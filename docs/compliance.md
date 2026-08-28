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

Only `Approved` records (that are not expired and not in a blocked jurisdiction)
pass `is_allowed`.

### `KycRecord`

| Field         | Type              | Meaning                                             |
|---------------|-------------------|-----------------------------------------------------|
| `address`     | `Address`         | The account this record applies to                  |
| `status`      | `ComplianceStatus`| Approval state                                      |
| `jurisdiction`| `String`          | Normalized 2-letter code, e.g. `"US"` — see [format](#jurisdiction-code-format) |
| `verified_at` | `u32`             | Ledger sequence at verification                     |
| `expires_at`  | `u32`             | Ledger sequence of expiry; `0` = never expires      |

> Time is measured in **ledger sequence numbers**, not calendar dates.

## Jurisdiction code format

`jurisdiction` is intended to be an **ISO 3166-1 alpha-2** country code
(`"US"`, `"GB"`, `"NG"`, …), and both `add_to_allowlist` and
`block_jurisdiction`/`unblock_jurisdiction` **do validate and normalize** the
value they're given — it is not free-form:

- Whitespace is stripped.
- The remaining characters are uppercased, so input is **case-insensitive**
  (`"us"`, `"Us"`, and `"US"` all normalize to the stored value `"US"`).
- The normalized result must be **exactly 2 ASCII alphabetic characters**, or
  the call panics with `InvalidJurisdiction (#6)`.

What is **not** validated: the two letters are not checked against the real
list of ISO 3166-1 country codes, so a well-formed-but-nonexistent code like
`"ZZ"` or `"XX"` is accepted. There is no on-chain source of truth for "is
this a real country" — that check, if needed, belongs off-chain before
calling `add_to_allowlist`.

Because normalization happens on write, `is_allowed`, `is_jurisdiction_blocked`,
and every stored `KycRecord.jurisdiction` always see the canonical uppercase
2-letter form regardless of how the caller cased their input.

## Functions

### `initialize(admin: Address)`
Sets the admin. Callable once. Requires `admin` auth.
Errors: `AlreadyInitialized (#1)`.

### `add_to_allowlist(admin, address, jurisdiction, expires_at)`
Approves `address` (status → `Approved`) in `jurisdiction`, expiring at
`expires_at` (`0` = never). `jurisdiction` is normalized and validated — see
[format](#jurisdiction-code-format). Admin only.
Errors: `Unauthorized (#5)`, `InvalidExpiry (#4)` if `expires_at` is already in
the past, `InvalidJurisdiction (#6)` if `jurisdiction` isn't a well-formed
2-letter code.

### `suspend(admin, address)`
Sets an existing record to `Suspended`; `is_allowed` returns `false` until
re-approved. Admin only. Errors: `RecordNotFound (#3)`, `Unauthorized (#5)`.

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
Every address currently on the allowlist.

### `block_jurisdiction(admin, jurisdiction)` / `unblock_jurisdiction(admin, jurisdiction)`
Block/unblock an entire country code. `jurisdiction` is normalized the same
way as in `add_to_allowlist` — see [format](#jurisdiction-code-format), so
`block_jurisdiction(admin, "us")` blocks the same jurisdiction as a record
stored with `"US"`. Approved addresses in a blocked jurisdiction fail
`is_allowed`. Admin only. Errors: `Unauthorized (#5)`,
`InvalidJurisdiction (#6)`.

### `is_jurisdiction_blocked(jurisdiction) -> bool`
Whether a jurisdiction is currently blocked.

### `get_admin() -> Address`
The configured admin. Errors: `NotInitialized (#2)`.

## Errors

| Code | Name                | Cause                                   |
|------|---------------------|-----------------------------------------|
| 1    | AlreadyInitialized  | `initialize` called twice               |
| 2    | NotInitialized      | Used before `initialize`                |
| 3    | RecordNotFound      | Operating on a missing record           |
| 4    | InvalidExpiry       | `expires_at` already in the past        |
| 5    | Unauthorized        | Caller is not the stored admin          |
| 6    | InvalidJurisdiction | `jurisdiction` isn't exactly 2 ASCII letters (after stripping whitespace) — see [format](#jurisdiction-code-format) |

## Events

| Topic        | Data                          | When                       |
|--------------|-------------------------------|----------------------------|
| `init`       | admin address                 | on initialize              |
| `approved`   | (address) → (jurisdiction, expires_at) | address approved  |
| `suspend`    | (address)                     | address suspended          |
| `removed`    | (address)                     | address removed            |
| `blockjur`   | jurisdiction                  | jurisdiction blocked       |
| `unblkjur`   | jurisdiction                  | jurisdiction unblocked     |

## Usage examples

Approve an address, check it, then suspend it:

```rust
// admin, client set up as usual (see contract tests for full setup boilerplate)
client.initialize(&admin);

// Approve `holder` in the US, never expiring.
client.add_to_allowlist(&admin, &holder, &String::from_str(&env, "US"), &0);
assert!(client.is_allowed(&holder));

// Approve `other` in Germany, expiring at ledger 500_000.
client.add_to_allowlist(&admin, &other, &String::from_str(&env, "de"), &500_000);
// stored/read back as "DE" — see jurisdiction code format above

// Suspend `holder`; is_allowed now returns false until re-approved.
client.suspend(&admin, &holder);
assert!(!client.is_allowed(&holder));
```

Block an entire jurisdiction — every approved address in it fails
`is_allowed` immediately, with no per-address change needed:

```rust
client.block_jurisdiction(&admin, &String::from_str(&env, "KP"));
assert!(client.is_jurisdiction_blocked(&String::from_str(&env, "KP")));
// any Approved, non-expired record with jurisdiction "KP" now fails is_allowed
```

## Storage / TTL

Listing of the contract `DataKey` variants and their storage behaviour.

| Key | Payload | Storage | TTL / Notes |
|-----|---------|---------|-------------|
| `Admin` | - | instance | - |
| `Allowlist` | - | instance | - |
| `Record` | Address | persistent | per-key TTL |
| `Blocked` | String | unknown | - |

## Unreachable or panicking compliance checks

Every `asset-token` `transfer` and `mint` calls `is_allowed` via a generated
client:

```rust
ComplianceClient::new(&env, &meta.compliance_contract).is_allowed(who)
```

This is a **direct** (non-`try_`) cross-contract call. `is_allowed` itself is
written to be total and never panic (see [Functions](#is_allowed---bool)
above) — but that guarantee only holds if `meta.compliance_contract` really
is a deployed instance of *this* compliance contract. Three failure modes to
be aware of, all with the **same observable effect**:

1. **`compliance_contract` is not a contract at all** (e.g. a plain account
   address, or an address nothing was ever deployed to).
2. **It is a contract, but doesn't implement `is_allowed(Address) -> bool`**
   with a matching signature.
3. **It does implement the interface, but a non-default compliance
   implementation panics inside its own `is_allowed`** (the interface itself
   doesn't prevent someone from swapping in a buggy or malicious
   implementation via `set_compliance`).

In every one of these cases, because the call is direct rather than
`try_invoke`d, the failure is a **host-level trap that propagates up through
`transfer`/`mint` and aborts the entire transaction**. The caller does **not**
see `asset-token`'s own typed errors (`SenderNotCompliant (#7)` /
`RecipientNotCompliant (#8)`) — those are only returned when `is_allowed`
successfully returns `false`. An unreachable or panicking compliance
contract instead surfaces as a generic invocation failure, and nothing is
partially applied: no balance moves, no event is emitted.

**Practical implication:** `set_compliance` effectively grants the asset
admin the power to brick every transfer/mint on the token (denial of
service) by pointing `compliance_contract` at a bad address — there is no
on-chain fallback or default-allow behavior if the compliance contract can't
be reached. Treat `set_compliance` with the same care as any other
admin-only, trust-critical operation.

## Security considerations

- All state-changing functions require the **stored admin** to both authorize
  (`require_auth`) and match the passed `admin` argument.
- `is_allowed` is intentionally total (never panics) so the asset token can rely
  on it inside transfers.
- Expiry is enforced against `env.ledger().sequence()` at read time, so an
  expired KYC blocks transfers without any keeper/cron.
