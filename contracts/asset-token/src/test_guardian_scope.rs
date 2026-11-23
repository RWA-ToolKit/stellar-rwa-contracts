I need to look at the actual contract to write a correct test. Since I only have the target file's (empty) content, I'll write the test based on the issue description and typical patterns, but I must be careful to match the real API.

Given the constraints, I'll produce a test file that uses the contract's public API as described in the issue. Since I cannot see `lib.rs`, I'll write the test defensively using the documented function names and `Error::Unauthorized`, and register the module.

Let me write the test file content:

```rust
//! Regression tests for issue #672: guardian privilege scope.
//!
//! The guardian role is intentionally allowed to call `pause` only. This test
//! enumerates every admin-only entry point and asserts that a guardian-only
//! signature is rejected, while `pause` succeeds. This guards against future
//! privileged functions accidentally granting the guardian broader powers.

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::{AssetToken, AssetTokenClient, Error};

/// Helper: deploy the contract, initialize it with an admin, and set a
/// distinct guardian address. Returns (env, client, admin, guardian).
fn setup() -> (Env, AssetTokenClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, AssetToken);
    let client = AssetTokenClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);

    client.initialize(&admin);
    client.set_guardian(&admin, &guardian);

    (env, client, admin, guardian)
}

#[test]
fn guardian_can_pause() {
    let (_env, client, _admin, guardian) = setup();
    // Guardian is explicitly permitted to pause.
    client.pause(&guardian);
}

#[test]
fn guardian_cannot_call_admin_only_functions() {
    let (env, client, _admin, guardian) = setup();

    let other = Address::generate(&env);

    // mint
    let res = client.try_mint(&guardian, &other, &1i128);
    assert_eq!(res, Err(Ok(Error::Unauthorized)), "guardian must not mint");

    // mint_batch
    let recipients = soroban_sdk::vec![&env, other.clone()];
    let amounts = soroban_sdk::vec![&env, 1i128];
    let res = client.try_mint_batch(&guardian, &recipients, &amounts);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not mint_batch"
    );

    // burn (admin-gated)
    let res = client.try_burn(&guardian, &other, &1i128);
    assert_eq!(res, Err(Ok(Error::Unauthorized)), "guardian must not burn");

    // set_compliance
    let res = client.try_set_compliance(&guardian, &other);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not set_compliance"
    );

    // update_valuation
    let res = client.try_update_valuation(&guardian, &1i128);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not update_valuation"
    );

    // snapshot
    let res = client.try_snapshot(&guardian);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not snapshot"
    );

    // unpause
    let res = client.try_unpause(&guardian);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not unpause"
    );

    // set_guardian
    let res = client.try_set_guardian(&guardian, &other);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not set_guardian"
    );

    // propose_admin
    let res = client.try_propose_admin(&guardian, &other);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not propose_admin"
    );

    // cancel_admin_proposal
    let res = client.try_cancel_admin_proposal(&guardian);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not cancel_admin_proposal"
    );

    // accept_admin
    let res = client.try_accept_admin(&guardian);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not accept_admin"
    );
}
```

However, I'm concerned about exact signatures. Since I cannot verify, I'll produce a reasonable file. But the instructions say return complete file content only. I'll return the file.

Actually, I should be more careful — the test must compile. Without seeing lib.rs I risk mismatches. But the task requires me to produce the file. I'll write it as best as possible matching the described API.

Let me finalize.```rust
//! Regression tests for issue #672: guardian privilege scope.
//!
//! The guardian role is intentionally allowed to call `pause` only. This test
//! enumerates every admin-only entry point and asserts that a guardian-only
//! signature is rejected, while `pause` succeeds. This guards against future
//! privileged functions accidentally granting the guardian broader powers.

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::{AssetToken, AssetTokenClient, Error};

/// Helper: deploy the contract, initialize it with an admin, and set a
/// distinct guardian address. Returns (env, client, admin, guardian).
fn setup() -> (Env, AssetTokenClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, AssetToken);
    let client = AssetTokenClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);

    client.initialize(&admin);
    client.set_guardian(&admin, &guardian);

    (env, client, admin, guardian)
}

#[test]
fn guardian_can_pause() {
    let (_env, client, _admin, guardian) = setup();
    // Guardian is explicitly permitted to pause.
    client.pause(&guardian);
}

#[test]
fn guardian_cannot_call_admin_only_functions() {
    let (env, client, _admin, guardian) = setup();

    let other = Address::generate(&env);

    // mint
    let res = client.try_mint(&guardian, &other, &1i128);
    assert_eq!(res, Err(Ok(Error::Unauthorized)), "guardian must not mint");

    // mint_batch
    let recipients = soroban_sdk::vec![&env, other.clone()];
    let amounts = soroban_sdk::vec![&env, 1i128];
    let res = client.try_mint_batch(&guardian, &recipients, &amounts);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not mint_batch"
    );

    // burn (admin-gated)
    let res = client.try_burn(&guardian, &other, &1i128);
    assert_eq!(res, Err(Ok(Error::Unauthorized)), "guardian must not burn");

    // set_compliance
    let res = client.try_set_compliance(&guardian, &other);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not set_compliance"
    );

    // update_valuation
    let res = client.try_update_valuation(&guardian, &1i128);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not update_valuation"
    );

    // snapshot
    let res = client.try_snapshot(&guardian);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not snapshot"
    );

    // unpause
    let res = client.try_unpause(&guardian);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not unpause"
    );

    // set_guardian
    let res = client.try_set_guardian(&guardian, &other);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not set_guardian"
    );

    // propose_admin
    let res = client.try_propose_admin(&guardian, &other);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not propose_admin"
    );

    // cancel_admin_proposal
    let res = client.try_cancel_admin_proposal(&guardian);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not cancel_admin_proposal"
    );

    // accept_admin
    let res = client.try_accept_admin(&guardian);
    assert_eq!(
        res,
        Err(Ok(Error::Unauthorized)),
        "guardian must not accept_admin"
    );
}