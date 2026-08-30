## Summary

<!-- What does this change do, and why? -->

## Contract change checklist

Delete this section if the PR doesn't touch `contracts/`.

- [ ] Tests added/updated for the behavior change (`cargo test --workspace` passes)
- [ ] New/changed events are documented in `docs/<contract>.md`
- [ ] Storage layout (the `DataKey` enum) is unchanged, **or** the contract's
      `VERSION` constant was bumped
- [ ] Error codes regenerated if the `Error` enum changed:
      `python3 scripts/generate_error_docs.py`
- [ ] Storage docs regenerated if `DataKey` changed:
      `python3 scripts/generate_storage_docs.py`
- [ ] `CHANGELOG.md` updated

## Related issues

Closes #
