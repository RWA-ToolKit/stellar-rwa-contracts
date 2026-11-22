//! Resource budgets for the asset-token hot paths (#719, #720).
//!
//! Prints the measured CPU instructions, memory and ledger reads/writes for
//! `transfer` and for `transfer_batch` at several batch sizes, and asserts
//! upper bounds so a regression fails CI. The numbers are recorded in
//! docs/RESOURCE_BUDGETS.md; see that file for the update policy.
//!
//!   cargo test -p asset-token resource_budget -- --nocapture --test-threads=1
#![cfg(test)]
extern crate std;
use std::println;

use super::*;
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, String, Vec};

struct Ctx {
    env: Env,
    token: AssetTokenContractClient<'static>,
    compliance: ComplianceContractClient<'static>,
    admin: Address,
}

fn ctx() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();
    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    let admin = Address::generate(&env);
    compliance.initialize(&admin);
    compliance.add_to_allowlist(&admin, &admin, &String::from_str(&env, "US"), &0);
    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);
    token.initialize(
        &admin,
        &String::from_str(&env, "Loft"),
        &String::from_str(&env, "LFT"),
        &String::from_str(&env, "real_estate"),
        &1_000_000i128,
        &0u32,
        &compliance_id,
        &String::from_str(&env, "desc"),
        &1_000_000i128,
    );
    Ctx {
        env,
        token,
        compliance,
        admin,
    }
}

fn holders(c: &Ctx, n: u32) -> Vec<(Address, i128)> {
    let mut out = Vec::new(&c.env);
    for _ in 0..n {
        let a = Address::generate(&c.env);
        c.compliance
            .add_to_allowlist(&c.admin, &a, &String::from_str(&c.env, "US"), &0);
        out.push_back((a, 10i128));
    }
    out
}

/// (cpu, mem, read entries, write entries) of the last invocation.
pub(crate) fn last_cost(env: &Env) -> (u64, u64, u32, u32) {
    let budget = env.cost_estimate().budget();
    let res = env.cost_estimate().resources();
    (
        budget.cpu_instruction_cost(),
        budget.memory_bytes_cost(),
        res.disk_read_entries + res.memory_read_entries,
        res.write_entries,
    )
}

#[test]
fn test_resource_budget_transfer() {
    let c = ctx();
    let [bob] = [Address::generate(&c.env)];
    c.compliance
        .add_to_allowlist(&c.admin, &bob, &String::from_str(&c.env, "US"), &0);
    c.env.cost_estimate().budget().reset_unlimited();
    c.token.transfer(&c.admin, &bob, &10);
    let (cpu, mem, reads, writes) = last_cost(&c.env);
    println!("[budget] transfer: cpu={cpu} mem={mem} reads={reads} writes={writes}");
    assert!(cpu < 2_000_000, "transfer cpu {cpu}");
}

#[test]
fn test_resource_budget_transfer_batch_marginal_cost() {
    let mut rows = std::vec::Vec::new();
    for n in [1u32, 10, 20] {
        let c = ctx();
        let batch = holders(&c, n);
        c.env.cost_estimate().budget().reset_unlimited();
        c.token.transfer_batch(&c.admin, &batch);
        let (cpu, mem, reads, writes) = last_cost(&c.env);
        println!(
            "[budget] transfer_batch n={n}: cpu={cpu} mem={mem} reads={reads} writes={writes}"
        );
        rows.push((n, cpu, writes));
    }
    let (n10, cpu10, _) = rows[1];
    let (n20, cpu20, writes20) = rows[2];
    let per_recipient = (cpu20 - cpu10) / (n20 - n10) as u64;
    println!("[budget] transfer_batch marginal cpu per recipient: {per_recipient}");
    // The sender's balance is written once per batch, not once per recipient:
    // one balance per recipient + 1 sender balance (+ the instance bump).
    assert!(
        writes20 <= n20 + 2,
        "transfer_batch wrote {writes20} entries for {n20} recipients"
    );
    assert!(
        per_recipient < 600_000,
        "marginal cpu per recipient {per_recipient}"
    );
}
