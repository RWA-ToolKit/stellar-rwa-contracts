#[cfg(test)]
mod tests {
    use crate::{Registry, RegistryClient};
    use soroban_sdk::{testutils::Address as _, Address, Env, String};

    fn setup_registry(env: &Env) -> (RegistryClient, Address) {
        let contract_id = env.register_contract(None, Registry);
        let client = RegistryClient::new(env, &contract_id);
        let admin = Address::generate(env);
        client.initialize(&admin);
        (client, admin)
    }

    fn register_asset(
        env: &Env,
        client: &RegistryClient,
        owner: &Address,
        asset_type: &str,
        id: u32,
    ) {
        let asset_id = String::from_str(env, &format!("asset_{}", id));
        let asset_type_str = String::from_str(env, asset_type);
        client.register_asset(owner, &asset_id, &asset_type_str);
    }

    #[test]
    fn test_get_assets_by_type_page_offset_overflow() {
        let env = Env::default();
        env.mock_all_auths();

        let (client, _admin) = setup_registry(&env);
        let owner = Address::generate(&env);

        // Register 3 real_estate assets and 2 commercial assets.
        register_asset(&env, &client, &owner, "real_estate", 1);
        register_asset(&env, &client, &owner, "real_estate", 2);
        register_asset(&env, &client, &owner, "real_estate", 3);
        register_asset(&env, &client, &owner, "commercial", 4);
        register_asset(&env, &client, &owner, "commercial", 5);

        let real_estate = String::from_str(&env, "real_estate");

        // Sanity check: full page returns all 3 matching assets.
        let all = client.get_assets_by_type_page(&real_estate, &0, &10);
        assert_eq!(all.len(), 3);

        // Offset strictly greater than matching count (4 > 3) returns empty, no panic.
        let overflow_by_one = client.get_assets_by_type_page(&real_estate, &4, &10);
        assert_eq!(overflow_by_one.len(), 0);

        // Large offset far beyond matching count returns empty, no panic.
        let overflow_large = client.get_assets_by_type_page(&real_estate, &99, &10);
        assert_eq!(overflow_large.len(), 0);

        // Offset equal to matching count returns empty (contrast case).
        let at_boundary = client.get_assets_by_type_page(&real_estate, &3, &10);
        assert_eq!(at_boundary.len(), 0);

        // Offset within range returns remaining assets.
        let partial = client.get_assets_by_type_page(&real_estate, &1, &10);
        assert_eq!(partial.len(), 2);

        // Empty type index with a large offset returns empty, no panic.
        let unknown_type = String::from_str(&env, "unknown_type");
        let empty_type = client.get_assets_by_type_page(&unknown_type, &99, &10);
        assert_eq!(empty_type.len(), 0);

        // Empty type index with zero offset also returns empty.
        let empty_type_zero = client.get_assets_by_type_page(&unknown_type, &0, &10);
        assert_eq!(empty_type_zero.len(), 0);
    }
}