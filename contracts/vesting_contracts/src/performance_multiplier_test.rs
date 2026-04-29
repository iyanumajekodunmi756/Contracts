#[cfg(test)]
mod performance_multiplier_tests {
    use super::*;
    use soroban_sdk::{
        testutils::Address as TestAddress,
        testutils::Ledger as TestLedger,
        Address,
        Env,
        Symbol,
        vec,
    };
    use crate::{VestingContract, VestingContractClient, OracleClient, ComparisonOperator, PerformanceMultiplier, Milestone};

    #[test]
    fn test_performance_multiplier_logic() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let beneficiary = Address::generate(&env);

        let contract_id = env.register_contract(None, VestingContract);
        let client = VestingContractClient::new(&env, &contract_id);

        client.initialize(&admin, &1000000);
        client.set_token(&Address::generate(&env));

        // Create a multiplier condition: TVL > 500,000
        let oracle_address = Address::generate(&env);
        let tvl_condition = OracleClient::create_tvl_condition(
            oracle_address.clone(),
            500000,
            ComparisonOperator::GreaterThan
        );

        let multiplier = PerformanceMultiplier {
            condition: tvl_condition,
            multiplier_bps: 15000,        // 1.5x if met
            fallback_multiplier_bps: 10000, // 1.0x if not met
        };

        // Create vault
        let vault_id = client.create_vault_full(
            &beneficiary,
            &100000,
            &1000, // start
            &2000, // end
            &0,
            &true,
            &false,
            &0
        );

        // Set multiplier
        client.set_vesting_multiplier(&vault_id, &multiplier);

        // Advance ledger to 50% vesting (1500)
        env.ledger().set_timestamp(1500);

        // Current oracle return value is 0 (mocked in oracle.rs)
        // Condition (0 > 500,000) is FALSE.
        // Should use fallback_multiplier_bps = 10000 (1.0x)
        // Base vested at 50% = 50,000
        let claimable = client.get_claimable_amount(&vault_id);
        assert_eq!(claimable, 50000);

        // Now, let's change the multiplier so that condition is met even with 0
        let easy_multiplier = PerformanceMultiplier {
            condition: OracleClient::create_tvl_condition(oracle_address, 0, ComparisonOperator::Equal),
            multiplier_bps: 12000, // 1.2x
            fallback_multiplier_bps: 10000,
        };
        client.set_vesting_multiplier(&vault_id, &easy_multiplier);

        // Base vested at 50% = 50,000. Multiplier 1.2x = 60,000
        let claimable_multiplied = client.get_claimable_amount(&vault_id);
        assert_eq!(claimable_multiplied, 60000);
        
        // Test capping logic
        // Advance to 90% (1900) -> base = 90,000. 1.2x = 108,000. Cap at 100,000
        env.ledger().set_timestamp(1900);
        let claimable_capped = client.get_claimable_amount(&vault_id);
        assert_eq!(claimable_capped, 100000);
    }

    #[test]
    fn test_multiplier_with_milestones() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let beneficiary = Address::generate(&env);

        let contract_id = env.register_contract(None, VestingContract);
        let client = VestingContractClient::new(&env, &contract_id);

        client.initialize(&admin, &1000000);
        client.set_token(&Address::generate(&env));

        let vault_id = client.create_vault_full(
            &beneficiary,
            &100000,
            &1000,
            &2000,
            &0,
            &true,
            &false,
            &0
        );

        // Set milestone: 50% unlock
        let milestones = vec![&env, Milestone { id: 1, percentage: 50, is_unlocked: false }];
        client.set_milestones(&vault_id, &milestones);

        // Set multiplier: 2.0x
        let oracle_address = Address::generate(&env);
        let multiplier = PerformanceMultiplier {
            condition: OracleClient::create_tvl_condition(oracle_address, 0, ComparisonOperator::Equal),
            multiplier_bps: 20000, // 2.0x
            fallback_multiplier_bps: 10000,
        };
        client.set_vesting_multiplier(&vault_id, &multiplier);

        // Unlock milestone
        client.unlock_milestone(&vault_id, &1);

        // 50% of 100,000 = 50,000. 2.0x = 100,000
        let claimable = client.get_claimable_amount(&vault_id);
        assert_eq!(claimable, 100000);
    }
}
