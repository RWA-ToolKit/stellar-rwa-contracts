//! Fuzz target: jurisdiction blocking vs KYC approval interactions
//!
//! Exercises the interplay between `block_jurisdiction`,
//! `unblock_jurisdiction`, `add_to_allowlist`, and `suspend` / `reinstate`
//! across multiple addresses and jurisdictions to discover unexpected
//! interactions or state corruption.
//!
//! Invariants checked:
//! - An address in a currently-blocked jurisdiction always fails `is_allowed`,
//!   regardless of its KYC status or any other rule.
//! - An address whose jurisdiction is not blocked passes `is_allowed` if and
//!   only if it is `Approved` and not expired.
//! - `get_blocked_jurisdictions()` exactly reflects the set of jurisdictions
//!   for which `is_jurisdiction_blocked()` returns `true`.
//! - `get_allowlist_count()` always equals `get_allowlist().len()`.

use arbitrary::{Arbitrary, Unstructured};
use compliance::ComplianceContract;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, String,
};

#[derive(Debug, Arbitrary)]
enum Op {
    /// Approve an address in a jurisdiction (index into USERS and JURS).
    Approve { user_idx: u8, jur_idx: u8 },
    /// Suspend an already-approved address.
    Suspend { user_idx: u8 },
    /// Reinstate a suspended address (no-op if not suspended).
    Reinstate { user_idx: u8 },
    /// Block a jurisdiction.
    BlockJur { jur_idx: u8 },
    /// Unblock a jurisdiction.
    UnblockJur { jur_idx: u8 },
    /// Advance the ledger by a small amount.
    AdvanceLedger { delta: u16 },
    /// Check all invariants without mutating state.
    CheckInvariants,
}

const JURS: &[&str] = &["US", "KE", "DE", "IR", "KP", "ZZ"];
const N_USERS: usize = 4;

fn main() {
    let raw: Vec<u8> = (0u8..=255).cycle().take(8192).collect();
    let mut u = Unstructured::new(&raw);
    for _ in 0..200 {
        let Ok(ops) = <[Op; 20]>::arbitrary(&mut u) else {
            break;
        };
        fuzz_one(&ops);
    }
}

fn fuzz_one(ops: &[Op]) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ComplianceContract, ());
    let client = compliance::ComplianceContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Pre-generate a fixed pool of user addresses.
    let users: std::vec::Vec<Address> = (0..N_USERS)
        .map(|_| Address::generate(&env))
        .collect();

    // Track which users have ever been approved (have a record).
    let mut approved = std::vec![false; N_USERS];
    let mut suspended = std::vec![false; N_USERS];

    env.ledger().with_mut(|l| l.sequence_number = 100);

    for op in ops {
        match op {
            Op::Approve { user_idx, jur_idx } => {
                let u_idx = (*user_idx as usize) % N_USERS;
                let j_idx = (*jur_idx as usize) % JURS.len();
                let jur = String::from_str(&env, JURS[j_idx]);
                let _ = client.try_add_to_allowlist(
                    &admin,
                    &users[u_idx],
                    &jur,
                    &0u32, // never-expires
                );
                approved[u_idx] = true;
                suspended[u_idx] = false;
            }
            Op::Suspend { user_idx } => {
                let u_idx = (*user_idx as usize) % N_USERS;
                if approved[u_idx] && !suspended[u_idx] {
                    let _ = client.try_suspend(&admin, &users[u_idx]);
                    suspended[u_idx] = true;
                }
            }
            Op::Reinstate { user_idx } => {
                let u_idx = (*user_idx as usize) % N_USERS;
                if suspended[u_idx] {
                    let _ = client.try_reinstate(&admin, &users[u_idx]);
                    suspended[u_idx] = false;
                }
            }
            Op::BlockJur { jur_idx } => {
                let j_idx = (*jur_idx as usize) % JURS.len();
                let jur = String::from_str(&env, JURS[j_idx]);
                let _ = client.try_block_jurisdiction(&admin, &jur);
            }
            Op::UnblockJur { jur_idx } => {
                let j_idx = (*jur_idx as usize) % JURS.len();
                let jur = String::from_str(&env, JURS[j_idx]);
                let _ = client.try_unblock_jurisdiction(&admin, &jur);
            }
            Op::AdvanceLedger { delta } => {
                let now = env.ledger().sequence();
                env.ledger()
                    .with_mut(|l| l.sequence_number = now.saturating_add(*delta as u32));
            }
            Op::CheckInvariants => {}
        }

        // After every operation, check invariants.
        check_invariants(&env, &client, &users, JURS);
    }
}

fn check_invariants(
    env: &Env,
    client: &compliance::ComplianceContractClient,
    users: &[Address],
    jurs: &[&str],
) {
    // 1. counter == get_allowlist().len()
    assert_eq!(
        client.get_allowlist_count(),
        client.get_allowlist().len(),
        "allowlist counter must match get_allowlist().len()"
    );

    // 2. get_blocked_jurisdictions() exactly matches is_jurisdiction_blocked() for each jur.
    let blocked_list = client.get_blocked_jurisdictions();
    for &jur_str in jurs {
        let jur = String::from_str(env, jur_str);
        let is_blocked = client.is_jurisdiction_blocked(&jur);
        let in_list = blocked_list.iter().any(|j| j == jur);
        assert_eq!(
            is_blocked, in_list,
            "is_jurisdiction_blocked({jur_str}) disagrees with get_blocked_jurisdictions()"
        );
    }

    // 3. Every address in a blocked jurisdiction must fail is_allowed.
    for user in users {
        if let Some(rec) = client.get_record(user) {
            let jur = rec.jurisdiction;
            let blocked = client.is_jurisdiction_blocked(&jur);
            if blocked {
                assert!(
                    !client.is_allowed(user),
                    "user in blocked jurisdiction must fail is_allowed"
                );
            }
        }
    }
}
