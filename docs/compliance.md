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
| `jurisdiction`| `String`          | ISO country code, e.g. `"US"`                       |
| `verified_at` | `u32`             | Ledger sequence at verification                     |
| `expires_at`  | `u32`             | Ledger sequence of expiry; `0` = never expires      |

> Time is measured in **ledger sequence numbers**, not calendar dates.

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
Block/unblock an entire country code. Approved addresses in a blocked
jurisdiction fail `is_allowed`. Admin only.

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

## Events

| Topic        | Data                          | When                       |
|--------------|-------------------------------|----------------------------|
| `init`       | admin address                 | on initialize              |
| `approved`   | (address) → (jurisdiction, expires_at) | address approved  |
| `suspend`    | (address)                     | address suspended          |
| `removed`    | (address)                     | address removed            |
| `blockjur`   | jurisdiction                  | jurisdiction blocked       |
| `unblkjur`   | jurisdiction                  | jurisdiction unblocked     |

## Storage / TTL

Listing of the contract `DataKey` variants and their storage behaviour.

| Key | Payload | Storage | TTL / Notes |
|-----|---------|---------|-------------|
| `Admin` | - | instance | - |
| `Allowlist` | - | instance | - |
| `Record` | Address | persistent | per-key TTL |
| `Blocked` | String | unknown | - |

## Security considerations

- All state-changing functions require the **stored admin** to both authorize
  (`require_auth`) and match the passed `admin` argument.
- `is_allowed` is intentionally total (never panics) so the asset token can rely
  on it inside transfers.
- Expiry is enforced against `env.ledger().sequence()` at read time, so an
  expired KYC blocks transfers without any keeper/cron.
