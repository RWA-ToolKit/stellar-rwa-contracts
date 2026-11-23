# Admin handover

Each contract (`compliance`, `asset-token`, `registry`, and `dividend`) keeps
its admin independently. The current admin proposes a successor; only that
address can accept. A new proposal replaces any existing pending proposal, and
the current admin can cancel the pending proposal at any time.

## Proposal expiry

Pending admin proposals expire. When `propose_admin` is called, the contract
records the current ledger sequence alongside the nominee. The proposal is
valid for a fixed window of `ADMIN_PROPOSAL_TTL_LEDGERS` ledgers (currently
`17_280`, roughly one day at ~5s per ledger). This bounds the window in which
a compromised successor key could be used to take over the contract without
the current admin having to remember to cancel.

After the window elapses, `accept_admin` fails with an explicit
`AdminProposalExpired` error. The pending proposal is not silently cleared on
read; `get_pending_admin` still reports the nominee until it is replaced or
cancelled, but it can no longer be accepted. To retry, the current admin must
call `propose_admin` again, which stores a fresh ledger sequence and restarts
the window. `cancel_admin_proposal` continues to work at any time.

This behaviour is identical across `compliance`, `asset-token`, `registry`,
and `dividend`.

Set the contract ID to the instance being handed over, then configure the
network and both identities. The successor identity must already exist and be
available on the selected network.

```bash
NETWORK=testnet
CONTRACT_ID="<contract-id>" # compliance, asset-token, registry, or dividend
CURRENT_ADMIN_IDENTITY=rwa-admin
NEW_ADMIN_IDENTITY=rwa-admin-successor
CURRENT_ADMIN_ADDR="$(stellar keys address "$CURRENT_ADMIN_IDENTITY")"
NEW_ADMIN_ADDR="$(stellar keys address "$NEW_ADMIN_IDENTITY")"

invoke() {
  local source="$1"
  shift
  stellar contract invoke \
    --id "$CONTRACT_ID" \
    --source "$source" \
    --network "$NETWORK" \
    -- "$@"
}

# Current admin proposes; inspect the pending nominee.
invoke "$CURRENT_ADMIN_IDENTITY" propose_admin \
  --admin "$CURRENT_ADMIN_ADDR" --new_admin "$NEW_ADMIN_ADDR"
invoke "$CURRENT_ADMIN_IDENTITY" get_pending_admin

# Successor accepts using its own identity.
invoke "$NEW_ADMIN_IDENTITY" accept_admin --new_admin "$NEW_ADMIN_ADDR"
invoke "$CURRENT_ADMIN_IDENTITY" get_admin
invoke "$CURRENT_ADMIN_IDENTITY" get_pending_admin
```

To cancel instead, the current admin runs
`invoke "$CURRENT_ADMIN_IDENTITY" cancel_admin_proposal --admin "$CURRENT_ADMIN_ADDR"`.
`get_pending_admin` then returns `None`, and an attempt to accept the cancelled
proposal fails with `NoPendingAdmin`. To replace a nominee, call `propose_admin`
again with the new address; only the latest nominee can accept.