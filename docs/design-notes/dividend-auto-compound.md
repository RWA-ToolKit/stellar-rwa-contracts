# Design proposal: dividend auto-compound (DRIP) — issue #734

**Status: proposal for discussion only. No on-chain implementation is included.**
Implementation is tracked as a follow-up once this design is agreed.

## Problem

`DividendContract::claim` (`contracts/dividend/src/lib.rs`) currently transfers
the holder's claimable amount in the distribution's `payment_token` and marks
the claim consumed. A holder who wants to reinvest dividends must claim and
then separately purchase more asset tokens. There is no primary
issuance/purchase contract in this toolkit today, so that second leg has no
canonical on-chain endpoint. Several RWA products want a DRIP-style option
that performs both legs atomically.

## Proposed user flow

1. At distribution creation (`create_distribution[_with_payout]`), the issuer
   opts in to auto-compound for eligible holders (or each holder opts in
   individually before the claim).
2. On `claim`, instead of transferring the payout token to the holder, the
   contract computes the claimable amount as today, then routes it into the
   primary issuance mechanism to buy additional asset tokens for that holder.
3. The claim marker is written before the external calls, as today; any trap
   rolls back the whole invocation, so a holder can never be double-paid or
   left with a consumed claim and no reinvested tokens.
4. A distinct event (`("auto_compound", holder)` with data
   `(distribution_id, amount_in, tokens_out, issuance_contract)`) makes
   reinvestments distinguishable from cash claims for indexers and tax
   reporting.

## Interaction with the (currently nonexistent) primary issuance mechanism

This is the main blocker. A minimal issuance interface would need to expose
something like:

```text
purchase(buyer: Address, payment_token: Address, amount_in: i128, min_tokens_out: i128) -> i128
```

The dividend contract would need:

- the issuance contract address, configured per distribution by the issuer;
- an allowance/authorization model (the holder authorizes the dividend
  contract to spend their claim, or the dividend contract passes the escrowed
  funds directly and the issuance contract credits the holder);
- a price/source of truth and slippage bound (`min_tokens_out`) so a holder
  cannot silently receive zero or an unfair amount;
- clear handling of compliance: the issuance contract must apply its own KYC
  checks on the recipient, and a failed check must revert the claim rather
  than leave funds stranded;
- handling for non-divisible payout tokens: any leftover dust below the
  purchase minimum must either return to the holder as a cash claim or stay
  claimable, and this must be explicit in the API.

Until such an issuance contract exists, the design can be implemented in a
separate adapter contract that holds the issuance address and is called by the
dividend contract, keeping `DividendContract` unchanged apart from a
configurable "reinvestment router" hook. That preserves backward compatibility
for issuers who want plain cash dividends.

## Alternatives considered

- **Manual claim + external purchase (status quo):** no new code, but fails the
  DRIP requirement and the second leg is currently impossible on-chain here.
- **Dividend contract mints asset tokens directly:** rejected. Minting is the
  asset token admin's responsibility and is compliance-gated; duplicating it
  here would bypass supply controls and jurisdiction checks.
- **Router contract only (recommended for phase 1):** isolates pricing and
  issuance integration behind a narrow interface, so the dividend contract
  does not need to know how primary issuance works.

## Open questions

- Who opts in: issuer at distribution creation, holder before claiming, or
  both (issuer enables, holder chooses)?
- Is the reinvestment router immutable per distribution or admin-updatable?
- Should auto-compound claims remain subject to the existing deadline and
  reclaim rules? (Proposed: yes, unchanged.)
- How are failure and dust refunds surfaced to the holder?

## Follow-up

Implementation is **not** part of issue #734. Once this design is agreed, open
a follow-up issue covering (a) the primary issuance/purchase contract or router
interface, (b) the dividend contract hook and events, (c) slippage and dust
tests, and (d) documentation updates to `docs/dividend.md`.
