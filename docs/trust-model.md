# Trust Model & Admin Powers

Each of the four contracts has its own independent `admin` address, set once
at `initialize` and immutable thereafter (there is no `transfer_admin`
function anywhere in this workspace — changing an admin means redeploying).
In practice a deployer typically uses the same key for all four, but the
contracts don't enforce or assume that; this page treats each admin
separately, since a holder or integrator evaluating trust should know
exactly what each one can and cannot do.

## compliance admin

**Can:**
- Approve, suspend, or remove any address's KYC status at will
  (`add_to_allowlist`, `suspend`, `remove`) — including retroactively
  suspending an address that already holds tokens, which then blocks that
  address on its *next* transfer/mint/burn (existing balances aren't
  touched or clawed back).
- Block or unblock an entire jurisdiction (`block_jurisdiction`/
  `unblock_jurisdiction`), instantly failing `is_allowed` for every
  currently-approved address in that jurisdiction, without touching their
  individual records.
- Prune expired records (`prune_expired`) — housekeeping only, doesn't
  change who's currently approved.

**Cannot:**
- Move, freeze, or seize any holder's tokens directly — compliance only
  gates future `transfer`/`mint` calls on the **asset-token** contract; it
  has no reference to token balances at all.
- Change `jurisdiction`/`expires_at` retroactively for a record without
  re-approving it (each `add_to_allowlist` call fully replaces the prior
  record).

**Holders are trusting** the compliance admin to run KYC/AML checks
honestly and to not weaponize `suspend`/`block_jurisdiction` against a
specific holder for reasons unrelated to compliance — there's no on-chain
appeal or timelock on either action; both take effect on the holder's very
next transfer attempt.

## asset-token admin

**Can:**
- Mint new supply to any compliance-approved address, arbitrarily
  (`mint`, `mint_batch`) — there is no supply cap enforced anywhere in this
  contract.
- Pause and unpause all transfers and mints platform-wide for this asset
  (`pause`/`unpause`), for any reason, for any duration.
- Re-price the asset (`update_valuation`) to any non-negative value — this
  is purely a recorded number with no oracle or external check; the admin
  can set it to anything.
- Repoint the compliance gate to a different contract entirely
  (`set_compliance`) — see
  [what this doesn't re-check](compliance.md#unreachable-or-panicking-compliance-checks)
  for what happens if the new address is wrong, and note it only verifies
  the *admin* is approved under the new contract, not any existing holder.

**Cannot:**
- Transfer or burn a holder's tokens without that holder's own
  authorization — `transfer`'s `from` and `burn`'s `from` both call
  `require_auth()` on the token owner, not the admin.
- Bypass the compliance gate for itself — `mint`/`mint_batch` still check
  the *recipient* against `is_allowed`, including when the admin mints to
  itself.

**Holders are trusting** the asset admin extensively: it can dilute their
holding's value at will via unlimited minting, freeze all activity via
`pause` indefinitely, and unilaterally change the asset's recorded
valuation with no external check. It **cannot**, however, take a holder's
existing balance without that holder signing the transaction — dilution and
freezing are the two real admin risks here, not outright seizure.

## dividend admin

**Can:**
- Create a distribution (`create_distribution`) against any asset token and
  any payment token it chooses, for any amount it can fund, at any time,
  with an arbitrary `eligible` snapshot list — the contract does not verify
  that the snapshot matches the asset token's real holder set or their real
  balances at all.

**Cannot:**
- Alter a distribution once created, cancel it, or reclaim escrowed funds
  early — there is no admin "cancel" or "withdraw" path in this contract; a
  funded distribution's escrow can only leave via holder `claim` calls.
- Force a holder to claim, or claim on a holder's behalf — `claim` requires
  the holder's own `require_auth()`.

**Holders are trusting** the dividend admin to build an honest `eligible`
snapshot — since nothing on-chain cross-checks it against the asset token's
real balances, an admin *could* construct a snapshot that pays itself or
any other address a share disproportionate to their real holdings.
Correctness of `eligible` is entirely a matter of the admin's honesty and
off-chain process (e.g. snapshotting `asset-token`'s real balances at a
known ledger and using exactly that), not something this contract verifies.

## registry admin

**Can:**
- Deactivate any registered asset (`deactivate_asset`), immediately
  excluding it from `total_value_locked()` and from being counted as
  "active" by any indexer reading `AssetEntry.active`.

**Cannot:**
- Register an asset on someone else's behalf, or edit an existing entry's
  `name`/`asset_type`/`valuation`/`token_contract` after registration —
  registration (`register_asset`) requires the **issuer's** own
  authorization, and there is no `update_asset` function at all.
- Reactivate a deactivated asset — there is no `reactivate_asset`; once
  deactivated, an entry stays excluded from TVL permanently (short of a
  fresh `register_asset` call by the issuer, which would get a new id).

**Holders are trusting** the registry admin only with *discoverability and
TVL accounting* — deactivating an entry doesn't touch the underlying
asset-token contract, its holders, or their balances in any way; it's purely
an index-level flag. The lowest-power admin role in this workspace by a wide
margin.

## What no admin, anywhere in this workspace, can do

- Directly move a holder's asset-token balance without that holder signing.
- Directly access or move dividend escrow outside the `claim` path.
- Read another contract's storage or override another contract's admin —
  each contract's admin is scoped to that contract alone.
