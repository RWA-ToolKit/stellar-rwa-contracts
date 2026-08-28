# Asset Lifecycle: Issuance to Dividend Claim

This walks through the full, intended call sequence for taking a real-world
asset from nothing to a holder claiming a dividend, naming every call, who
must sign it, and which contract it goes to. See the per-contract docs
([compliance](compliance.md), [asset-token](asset-token.md),
[registry](registry.md), [dividend](dividend.md)) for full parameter lists
and error codes.

## 0. One-time setup (per deployment)

| Step | Call | Contract | Signer |
|---|---|---|---|
| 1 | `initialize(admin)` | compliance | the compliance admin |
| 2 | `initialize(admin)` | registry | the registry admin |

These can happen in either order and are independent of any specific asset.
`scripts/deploy.sh` does this for you on a fresh deployment — see the
[Quick start](../README.md#quick-start) in the README.

## 1. Approve the issuer with compliance

Before an issuer can hold the initial supply of their own token, **the
issuer's own address must already be KYC-approved** — `asset-token`'s
`initialize` requires the admin (who receives the initial supply) to pass
the compliance check.

```
compliance.add_to_allowlist(admin, issuer_address, jurisdiction, expires_at)
```
Signer: the **compliance admin**. See
[compliance jurisdiction codes](compliance.md#jurisdiction-code-format) for
the `jurisdiction` format.

## 2. Deploy and initialize the asset token

```
asset_token.initialize(
    admin,                 // becomes the token's admin; must already be compliance-approved
    name, symbol,
    asset_type,             // "real_estate" | "invoice" | "commodity"
    total_supply, decimals,
    compliance_contract,    // the compliance contract from step 0
    asset_description,
    valuation,               // USD cents
)
```
Signer: the **asset admin** (`admin`). This mints `total_supply` entirely to
`admin` — there are no other holders yet. Errors if `admin` isn't already
compliance-approved (`RecipientNotCompliant`).

## 3. Register the asset in the registry

```
registry.register_asset(issuer, token_contract, name, asset_type, valuation) -> asset_id
```
Signer: the **issuer** (any address may register any token contract it
controls — the registry does not itself verify that `issuer` is the asset
token's admin). This is what makes the asset discoverable via
`get_all_assets` / `get_assets_by_type` and counted in
`total_value_locked()`. Registration is optional for the token to function,
but expected for the asset to appear anywhere in the platform's UI/indexer.

## 4. Approve holders

Each address that will ever hold or receive the token must be
compliance-approved **before** it can be a `transfer`/`mint` recipient:

```
compliance.add_to_allowlist(admin, holder_address, jurisdiction, expires_at)
```
Signer: the **compliance admin**, once per holder. Do this for every
beneficiary/investor before minting or transferring to them.

## 5. Distribute the token to holders

Either:

```
asset_token.mint(admin, to, amount)
```
Signer: the **asset admin**, to increase supply and pay `to` directly, or

```
asset_token.transfer(from, to, amount)
```
Signer: **`from`**, moving existing supply between two already-approved
holders. Both `mint`'s recipient and `transfer`'s sender+recipient are
checked against compliance on every call — see
[what happens if that check can't complete](compliance.md#unreachable-or-panicking-compliance-checks).

## 6. Create a dividend distribution

```
dividend.create_distribution(admin, asset_token, payment_token, total_amount, eligible) -> distribution_id
```
Signer: the **dividend contract's admin** (set via `dividend.initialize`,
independent of the asset token's own admin — the same address is used in
practice but the contracts don't enforce that). This call pulls
`total_amount` of `payment_token` from `admin` into the dividend contract's
own escrow, so `admin` must have approved that transfer amount beforehand
(standard SEP-41 token allowance/balance requirements apply to
`payment_token`, not to the RWA asset token itself). `eligible` is the frozen
snapshot of `(holder_address, balance)` pairs used to size every payout —
see [dividend.md](dividend.md) for how the snapshot is built and why it's
frozen at creation rather than read live at claim time.

## 7. Holders claim

```
dividend.claim(distribution_id, holder)
```
Signer: **the holder claiming their own share**. Pays
`total_amount * holder's snapshot balance / snapshot supply` (floored) from
the dividend contract's escrow. Each holder can claim a given distribution
once; a second `claim` call fails `AlreadyClaimed`. Call
`dividend.claimable(distribution_id, holder)` first (no auth required) to
check the amount before claiming.

## Full sequence at a glance

```
compliance.initialize(admin)                                    [compliance admin]
registry.initialize(admin)                                      [registry admin]
compliance.add_to_allowlist(admin, issuer, jurisdiction, exp)    [compliance admin]
asset_token.initialize(admin, ..., compliance_contract, ...)     [asset admin]
registry.register_asset(issuer, token_contract, ...)             [issuer]
compliance.add_to_allowlist(admin, holder, jurisdiction, exp)    [compliance admin]  (repeat per holder)
asset_token.mint(admin, holder, amount)                          [asset admin]        (or transfer)
dividend.create_distribution(admin, asset_token, pay_token, amt, eligible)  [dividend admin]
dividend.claim(distribution_id, holder)                          [holder]             (repeat per holder)
```
