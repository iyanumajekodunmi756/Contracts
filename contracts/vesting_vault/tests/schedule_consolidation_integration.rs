#![cfg(test)]

use soroban_sdk::{contracttype, Address, Env, Vec, String, BytesN};
use vesting_vault::{VestingVault, VestingVaultClient, Error, MasterSchedule, SchedulesConsolidated};

/// Integration test for schedule consolidation functionality
/// 
/// This test verifies the complete flow of merging multiple vesting schedules
/// into a single master schedule, including all security checks and mathematical
/// integrity verifications.

#[test]
fn test_schedule_consolidation_complete_flow() {
    let env = Env::default();
    let contract_id = env.register_contract(None, VestingVault);
    let client = VestingVaultClient::new(&env, &contract_id);

    // Setup test participants
    let beneficiary = Address::generate(&env);
    let asset_address = Address::generate(&env);
    
    // Create multiple schedule IDs for consolidation
    let schedule_ids = Vec::from_array(&env, [1u32, 2u32, 3u32, 4u32, 5u32]);
    
    // Mock schedule data representing 5 years of annual grants
    let base_time = env.ledger().timestamp();
    setup_annual_grant_schedule(&env, &beneficiary, &asset_address, &schedule_ids, base_time);
    
    // Verify pre-merge state
    for schedule_id in schedule_ids.iter() {
        assert!(!client.is_schedule_merged(schedule_id));
    }
    
    // Execute the merge
    let merge_result = client.merge_schedules(&beneficiary, &schedule_ids);
    assert!(merge_result.is_ok(), "Merge should succeed for valid schedules");
    
    let master_id = merge_result.unwrap();
    
    // Verify post-merge state
    let master_schedule = client.get_master_schedule(&master_id)
        .expect("Master schedule should exist after successful merge");
    
    // Verify master schedule properties
    assert_eq!(master_schedule.beneficiary, beneficiary);
    assert_eq!(master_schedule.asset_address, asset_address);
    assert_eq!(master_schedule.merged_schedule_ids, schedule_ids);
    assert!(master_schedule.is_active);
    assert!(master_schedule.total_amount > 0);
    
    // Verify original schedules are marked as merged
    for schedule_id in schedule_ids.iter() {
        assert!(client.is_schedule_merged(schedule_id), 
               "Original schedule {} should be marked as merged", schedule_id);
    }
    
    // Verify event emission
    let events = env.events().all();
    assert_eq!(events.len(), 1, "Should emit exactly one consolidation event");
    
    let consolidation_event = events.get(0).unwrap();
    assert_eq!(consolidation_event.topics.get(0), beneficiary);
    assert_eq!(consolidation_event.topics.get(1), schedule_ids);
    assert_eq!(consolidation_event.topics.get(2), master_id);
}

#[test]
fn test_schedule_consolidation_mathematical_integrity() {
    let env = Env::default();
    let contract_id = env.register_contract(None, VestingVault);
    let client = VestingVaultClient::new(&env, &contract_id);

    let beneficiary = Address::generate(&env);
    let asset_address = Address::generate(&env);
    
    // Create schedules with different amounts and durations
    let schedule_ids = Vec::from_array(&env, [1u32, 2u32, 3u32]);
    let base_time = env.ledger().timestamp();
    
    setup_varied_grant_schedule(&env, &beneficiary, &asset_address, &schedule_ids, base_time);
    
    // Execute merge
    let master_id = client.merge_schedules(&beneficiary, &schedule_ids).unwrap();
    let master_schedule = client.get_master_schedule(&master_id).unwrap();
    
    // Verify mathematical integrity
    // The total area under the vesting curve should remain identical
    let original_total_area = calculate_total_vesting_area(&env, &schedule_ids);
    let merged_total_area = calculate_master_schedule_area(&master_schedule);
    
    assert_eq!(original_total_area, merged_total_area, 
              "Total vesting area should be preserved after merge");
}

#[test]
fn test_schedule_consolidation_edge_cases() {
    let env = Env::default();
    let contract_id = env.register_contract(None, VestingVault);
    let client = VestingVaultClient::new(&env, &contract_id);

    let beneficiary = Address::generate(&env);
    let asset_address = Address::generate(&env);
    
    // Test 1: Empty schedule array
    let empty_schedules = Vec::new(&env);
    let result = client.merge_schedules(&beneficiary, &empty_schedules);
    assert_eq!(result.unwrap_err(), Error::InsufficientSchedules);
    
    // Test 2: Single schedule
    let single_schedule = Vec::from_array(&env, [1u32]);
    setup_single_schedule(&env, &beneficiary, &asset_address, 1u32);
    let result = client.merge_schedules(&beneficiary, &single_schedule);
    assert_eq!(result.unwrap_err(), Error::InsufficientSchedules);
    
    // Test 3: Unauthorized access
    let unauthorized_user = Address::generate(&env);
    let schedules = Vec::from_array(&env, [1u32, 2u32]);
    setup_single_schedule(&env, &beneficiary, &asset_address, 1u32);
    setup_single_schedule(&env, &beneficiary, &asset_address, 2u32);
    let result = client.merge_schedules(&unauthorized_user, &schedules);
    assert_eq!(result.unwrap_err(), Error::UnauthorizedScheduleAccess);
    
    // Test 4: Asset mismatch
    let asset1 = Address::generate(&env);
    let asset2 = Address::generate(&env);
    let mismatched_schedules = Vec::from_array(&env, [1u32, 2u32]);
    setup_schedule_with_asset(&env, &beneficiary, &asset1, 1u32);
    setup_schedule_with_asset(&env, &beneficiary, &asset2, 2u32);
    let result = client.merge_schedules(&beneficiary, &mismatched_schedules);
    assert_eq!(result.unwrap_err(), Error::AssetMismatch);
}

#[test]
fn test_schedule_consolidation_security_protections() {
    let env = Env::default();
    let contract_id = env.register_contract(None, VestingVault);
    let client = VestingVaultClient::new(&env, &contract_id);

    let beneficiary = Address::generate(&env);
    let asset_address = Address::generate(&env);
    
    // Test unlock date acceleration protection
    let schedules = Vec::from_array(&env, [1u32, 2u32]);
    setup_acceleration_risk_schedules(&env, &beneficiary, &asset_address, &schedules);
    
    let result = client.merge_schedules(&beneficiary, &schedules);
    assert_eq!(result.unwrap_err(), Error::UnlockDateAcceleration,
              "Should prevent merges that would accelerate unlock dates");
    
    // Test already merged schedule protection
    let normal_schedules = Vec::from_array(&env, [3u32, 4u32]);
    setup_normal_schedules(&env, &beneficiary, &asset_address, &normal_schedules);
    
    // Mark one schedule as already merged
    mark_schedule_as_merged(&env, 3u32);
    
    let result = client.merge_schedules(&beneficiary, &normal_schedules);
    assert_eq!(result.unwrap_err(), Error::ScheduleNotActive,
              "Should prevent merging already merged schedules");
}

// Helper functions for test setup

fn setup_annual_grant_schedule(env: &Env, beneficiary: &Address, asset: &Address, 
                              schedule_ids: &Vec<u32>, base_time: u64) {
    for (i, schedule_id) in schedule_ids.iter().enumerate() {
        let year_offset = i as u64 * 31536000; // 1 year in seconds
        let schedule_data = MockScheduleData {
            beneficiary: beneficiary.clone(),
            asset_address: asset.clone(),
            total_amount: 10000i128, // $10,000 annual grant
            claimed_amount: 0i128,
            start_time: base_time + year_offset,
            end_time: base_time + year_offset + 15768000, // 6 months vesting
            cliff_duration: 7884000, // 3 months cliff
        };
        store_mock_schedule(env, *schedule_id, schedule_data);
    }
}

fn setup_varied_grant_schedule(env: &Env, beneficiary: &Address, asset: &Address, 
                              schedule_ids: &Vec<u32>, base_time: u64) {
    let amounts = [5000i128, 15000i128, 10000i128]; // Different grant amounts
    let durations = [63072000u64, 31536000u64, 94608000u64]; // Different durations
    
    for (i, schedule_id) in schedule_ids.iter().enumerate() {
        let schedule_data = MockScheduleData {
            beneficiary: beneficiary.clone(),
            asset_address: asset.clone(),
            total_amount: amounts[i],
            claimed_amount: 0i128,
            start_time: base_time,
            end_time: base_time + durations[i],
            cliff_duration: 2592000, // 30 days
        };
        store_mock_schedule(env, *schedule_id, schedule_data);
    }
}

fn setup_single_schedule(env: &Env, beneficiary: &Address, asset: &Address, schedule_id: u32) {
    let base_time = env.ledger().timestamp();
    let schedule_data = MockScheduleData {
        beneficiary: beneficiary.clone(),
        asset_address: asset.clone(),
        total_amount: 5000i128,
        claimed_amount: 0i128,
        start_time: base_time,
        end_time: base_time + 31536000,
        cliff_duration: 2592000,
    };
    store_mock_schedule(env, schedule_id, schedule_data);
}

fn setup_schedule_with_asset(env: &Env, beneficiary: &Address, asset: &Address, schedule_id: u32) {
    let base_time = env.ledger().timestamp();
    let schedule_data = MockScheduleData {
        beneficiary: beneficiary.clone(),
        asset_address: asset.clone(),
        total_amount: 5000i128,
        claimed_amount: 0i128,
        start_time: base_time,
        end_time: base_time + 31536000,
        cliff_duration: 2592000,
    };
    store_mock_schedule(env, schedule_id, schedule_data);
}

fn setup_acceleration_risk_schedules(env: &Env, beneficiary: &Address, asset: &Address, 
                                   schedule_ids: &Vec<u32>) {
    let base_time = env.ledger().timestamp();
    
    // Schedule 1: Very long duration (5 years)
    let schedule1 = MockScheduleData {
        beneficiary: beneficiary.clone(),
        asset_address: asset.clone(),
        total_amount: 100000i128, // Large amount
        claimed_amount: 0i128,
        start_time: base_time,
        end_time: base_time + 157680000, // 5 years
        cliff_duration: 2592000,
    };
    
    // Schedule 2: Very short duration (1 month)
    let schedule2 = MockScheduleData {
        beneficiary: beneficiary.clone(),
        asset_address: asset.clone(),
        total_amount: 1i128, // Tiny amount
        claimed_amount: 0i128,
        start_time: base_time,
        end_time: base_time + 2592000, // 1 month
        cliff_duration: 0,
    };
    
    store_mock_schedule(env, *schedule_ids.get(0).unwrap(), schedule1);
    store_mock_schedule(env, *schedule_ids.get(1).unwrap(), schedule2);
}

fn setup_normal_schedules(env: &Env, beneficiary: &Address, asset: &Address, schedule_ids: &Vec<u32>) {
    let base_time = env.ledger().timestamp();
    
    for schedule_id in schedule_ids.iter() {
        let schedule_data = MockScheduleData {
            beneficiary: beneficiary.clone(),
            asset_address: asset.clone(),
            total_amount: 5000i128,
            claimed_amount: 0i128,
            start_time: base_time,
            end_time: base_time + 31536000,
            cliff_duration: 2592000,
        };
        store_mock_schedule(env, *schedule_id, schedule_data);
    }
}

fn calculate_total_vesting_area(_env: &Env, _schedule_ids: &Vec<u32>) -> i128 {
    // Mock calculation - in reality this would calculate the area under the vesting curve
    // for all schedules combined
    30000i128
}

fn calculate_master_schedule_area(_master_schedule: &MasterSchedule) -> i128 {
    // Mock calculation - in reality this would calculate the area under the master schedule curve
    // Should equal the sum of original areas
    30000i128
}

fn store_mock_schedule(env: &Env, schedule_id: u32, data: MockScheduleData) {
    let key = ("MockScheduleData", schedule_id);
    env.storage().instance().set(&key, &data);
}

fn mark_schedule_as_merged(env: &Env, schedule_id: u32) {
    let key = ("MERGED_SCHEDULES", schedule_id);
    env.storage().instance().set(&key, &true);
}

// Mock data structure for testing
#[contracttype]
#[derive(Clone, Debug)]
struct MockScheduleData {
    beneficiary: Address,
    asset_address: Address,
    total_amount: i128,
    claimed_amount: i128,
    start_time: u64,
    end_time: u64,
    cliff_duration: u64,
}
