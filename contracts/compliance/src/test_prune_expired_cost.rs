//! #727: what `prune_expired` costs per record, and the largest `max_records`
//! that fits the transaction limits when every examined record has expired
//! (the worst case: each one is deleted).
//!
//! `cargo test -p compliance prune_expired_cost -- --nocapture --test-threads=1`
#![cfg(test)]
extern crate std;
use std::println;

use super::*;
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env, String};

/// `n` allowlisted addresses, all expired; then prune with `max_records`
/// under the real limits. Some(cpu) if it fits.
fn prune(n: u32, max_records: u32) -> Option<u64> {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();
    let id = env.register(ComplianceContract, ());
    let client = ComplianceContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    env.ledger().with_mut(|l| l.sequence_number = 10);
    let us = String::from_str(&env, "US");
    for _ in 0..n {
        client.add_to_allowlist(&admin, &Address::generate(&env), &us, &100);
    }
    env.ledger().with_mut(|l| l.sequence_number = 200);
    env.cost_estimate().budget().reset_default();
    client
        .try_prune_expired(&admin, &max_records)
        .is_ok()
        .then(|| env.cost_estimate().budget().cpu_instruction_cost())
}

fn fits(n: u32, max_records: u32) -> Option<u64> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(std::boxed::Box::new(|_| {}));
    let out = std::panic::catch_unwind(|| prune(n, max_records)).unwrap_or(None);
    std::panic::set_hook(hook);
    out
}

#[test]
fn test_prune_expired_cost_worst_case_ceiling() {
    let n = 60;
    let mut largest_ok = 0;
    for m in [1u32, 5, 10, 15, 20, 25, 30, 40] {
        match fits(n, m) {
            Some(cpu) => {
                println!(
                    "[#727] prune_expired max_records={m} (all expired): cpu={cpu} ({} per record)",
                    cpu / m as u64
                );
                largest_ok = m;
            }
            None => println!(
                "[#727] prune_expired max_records={m} (all expired): exceeds transaction limits"
            ),
        }
    }
    let mut m = largest_ok + 1;
    while fits(n, m).is_some() {
        largest_ok = m;
        m += 1;
    }
    println!("[#727] largest max_records that fits when every record is expired: {largest_ok}");
    println!(
        "[#727] max_records=0 (unbounded) over {n} expired records: {}",
        if fits(n, 0).is_some() {
            "fits"
        } else {
            "exceeds transaction limits"
        }
    );
    // docs/compliance.md recommends max_records = 10-20.
    assert!(largest_ok >= 20, "worst-case ceiling {largest_ok}");
}
