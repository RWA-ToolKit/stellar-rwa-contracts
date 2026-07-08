# Asset Token Contract

A compliant token representing a tokenized real-world asset. Every `transfer`
checks the compliance contract for **both** sender and recipient, and every
`mint` checks the recipient — so only KYC-approved addresses can hold the asset.

- Testnet: `CBMCWLSQSWUTLUJFCNBHNBSXMUM3XU7NAQ5TSNERW4HA4ZZBYHLG4ECZ`

## The compliance check (core feature)

`transfer` and `mint` call into the compliance contract via a lightweight
generated client:

```rust
#[contractclient(name = "ComplianceClient")]
pub trait ComplianceInterface {
    fn is_allowed(env: Env, address: Address) -> bool;
}
// inside transfer:
if !ComplianceClient::new(&env, &meta.compliance_contract).is_allowed(&from) {
    // -> SenderNotCompliant (#7)
}
if !ComplianceClient::new(&env, &meta.compliance_contract).is_allowed(&to) {
    // -> RecipientNotCompliant (#8)
}
```

This decouples the two contracts at build time — the token only knows the
compliance *interface*, and the concrete compliance contract address is stored
in metadata and can be swapped with `set_compliance`.

## `AssetMetadata`

| Field                 | Type      | Meaning                              |
|-----------------------|-----------|--------------------------------------|
| `name` / `symbol`     | `String`  | Display name and ticker              |
| `asset_type`          | `String`  | `real_estate`, `invoice`, `commodity`|
| `total_supply`        | `i128`    | Current supply (base units)          |
| `decimals`            | `u32`     | Token decimals                       |
| `admin`               | `Address` | Controls mint/pause/valuation        |
| `compliance_contract` | `Address` | Gate consulted on transfer/mint      |
| `asset_description`   | `String`  | Free-text description                |
| `valuation`           | `i128`    | Asset value in **USD cents**         |
| `paused`              | `bool`    | When true, transfers/mints revert    |

## Functions

- `initialize(admin, name, symbol, asset_type, total_supply, decimals, compliance_contract, asset_description, valuation)` —
  stores metadata and mints `total_supply` to `admin`. The admin must already be
  compliance-approved. Admin auth. Once only.
- `transfer(from, to, amount)` — `from` auth; not paused; both parties compliant;
  `from` has balance; moves tokens.
- `mint(admin, to, amount)` — admin auth; not paused; `to` compliant; increases
  supply.
- `burn(from, amount)` — `from` auth; reduces caller balance and supply.
- `balance(id) -> i128`
- `total_supply() -> i128`
- `pause(admin)` / `unpause(admin)` — admin auth.
- `get_metadata() -> AssetMetadata`
- `update_valuation(admin, new_valuation)` — admin auth.
- `set_compliance(admin, compliance)` — admin auth; repoints the gate.

## Errors

| Code | Name                   | Cause                                |
|------|------------------------|--------------------------------------|
| 1    | AlreadyInitialized     | double init                          |
| 2    | NotInitialized         | used before init                     |
| 3    | Unauthorized           | non-admin admin-only call            |
| 4    | InsufficientBalance    | transfer/burn over balance           |
| 5    | InvalidAmount          | amount <= 0 (or negative supply/val) |
| 6    | Paused                 | transfer/mint while paused           |
| 7    | SenderNotCompliant     | sender fails `is_allowed`            |
| 8    | RecipientNotCompliant  | recipient fails `is_allowed`         |
| 9    | Overflow               | supply overflow on mint              |

## Events

| Topic       | Data                    | When         |
|-------------|-------------------------|--------------|
| `mint`      | (to) → amount           | mint / init  |
| `transfer`  | (from, to) → amount     | transfer     |
| `burn`      | (from) → amount         | burn         |
| `pause`     | admin                   | pause        |
| `unpause`   | admin                   | unpause      |
| `valuation` | new valuation           | valuation up |
| `setcomp`   | compliance address      | gate changed |

## Security considerations

- Compliance is enforced **inside** `transfer`/`mint`; it cannot be bypassed by
  calling the token directly.
- Amounts must be strictly positive; zero/negative amounts revert.
- `mint` overflow is checked; supply cannot wrap.
- Only the admin can pause, mint, change valuation, or repoint compliance.
