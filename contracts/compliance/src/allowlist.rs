//! Allowlist paging helpers for the compliance contract (issue #177).

use soroban_sdk::{Address, Env, Vec};

use crate::{DataKey, ALLOWLIST_PAGE_SIZE};

/// Current (page index, length of that page) append cursor.
/// Defaults to an empty page 0 when nothing has been added yet.
pub(crate) fn allowlist_meta(env: &Env) -> (u32, u32) {
    env.storage()
        .instance()
        .get(&DataKey::AllowlistMeta)
        .unwrap_or((0u32, 0u32))
}

/// Append a new address to the current allowlist page, rolling over to a
/// fresh page once the current one is full (issue #177).
pub(crate) fn append_to_allowlist(env: &Env, address: &Address) {
    let (mut page_idx, mut page_len) = allowlist_meta(env);
    if page_len >= ALLOWLIST_PAGE_SIZE {
        page_idx += 1;
        page_len = 0;
    }
    let mut page: Vec<Address> = env
        .storage()
        .persistent()
        .get(&DataKey::AllowlistPage(page_idx))
        .unwrap_or_else(|| Vec::new(env));
    page.push_back(address.clone());
    env.storage()
        .persistent()
        .set(&DataKey::AllowlistPage(page_idx), &page);
    env.storage()
        .persistent()
        .set(&DataKey::AllowlistPageOf(address.clone()), &page_idx);
    page_len += 1;
    env.storage()
        .instance()
        .set(&DataKey::AllowlistMeta, &(page_idx, page_len));

    let count: u32 = env
        .storage()
        .instance()
        .get(&DataKey::AllowlistCount)
        .unwrap_or(0u32);
    env.storage()
        .instance()
        .set(&DataKey::AllowlistCount, &(count + 1));
}

/// Remove an address from whichever page it lives on.
/// Leaves the page under-full rather than repacking pages, keeping removal
/// O(page size) instead of O(list size) (issue #177, issue #308).
pub(crate) fn remove_from_allowlist(env: &Env, address: &Address) {
    let page_idx: Option<u32> = env
        .storage()
        .persistent()
        .get(&DataKey::AllowlistPageOf(address.clone()));
    let Some(page_idx) = page_idx else {
        return;
    };
    let page: Vec<Address> = env
        .storage()
        .persistent()
        .get(&DataKey::AllowlistPage(page_idx))
        .unwrap_or_else(|| Vec::new(env));
    let mut next = Vec::new(env);
    for a in page.iter() {
        if a != *address {
            next.push_back(a);
        }
    }
    env.storage()
        .persistent()
        .set(&DataKey::AllowlistPage(page_idx), &next);
    env.storage()
        .persistent()
        .remove(&DataKey::AllowlistPageOf(address.clone()));

    let count: u32 = env
        .storage()
        .instance()
        .get(&DataKey::AllowlistCount)
        .unwrap_or(0u32);
    env.storage()
        .instance()
        .set(&DataKey::AllowlistCount, &count.saturating_sub(1));
}
