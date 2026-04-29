#![cfg(test)]

use soroban_sdk::{contracttype, Address, Env, Vec, String};
use vesting_vault::{VestingVault, VestingVaultClient, Error, MasterSchedule, SchedulesConsolidated};

#[test]
fn test_merge_schedules_success() {
    let env = Env::default();
    let contract_id = env.register_contract(None, VestingVault);
    let client = VestingVaultClient::new(&env, &contract_id);

    let beneficiary = Address::generate(&env);
    let asset_address = Address::generate(&env);

    // Create mock schedule data for testing
    let schedule_ids = vec![&env, 1u32, 2u32, 3u32];

    // Mock the schedule data storage
    // In a real implementation, this would be done through proper vault creation
    setup_mock_schedules(&env, &beneficiary, &asset_address, &schedule_ids);

    // Test successful merge
    let result = client.merge_schedules(&beneficiary, &schedule_ids);
    assert!(result.is_ok());

    let master_id = result.unwrap();
    
    // Verify master schedule was created
    let master_schedule = client.get_master_schedule(&master_id).unwrap();
    assert_eq!(master_schedule.beneficiary, beneficiary);
    assert_eq!(master_schedule.asset_address, asset_address);
    assert_eq!(master_schedule.merged_schedule_ids, schedule_ids);
    assert!(master_schedule.is_active);

    // Verify original schedules are marked as merged
    for schedule_id in schedule_ids.iter() {
        assert!(client.is_schedule_merged(schedule_id));
    }
}

#[test]
fn test_merge_schedules_insufficient_schedules() {
    let env = Env::default();
    let contract_id = env.register_contract(None, VestingVault);
    let client = VestingVaultClient::new(&env, &contract_id);

    let beneficiary = Address::generate(&env);
    let schedule_ids = vec![&env, 1u32]; // Only one schedule

    // Test with insufficient schedules
    let result = client.merge_schedules(&beneficiary, &schedule_ids);
    assert_eq!(result.unwrap_err(), Error::InsufficientSchedules);
}

#[test]
fn test_merge_schedules_unauthorized_access() {
    let env = Env::default();
    let contract_id = env.register_contract(None, VestingVault);
    let client = VestingVaultClient::new(&env, &contract_id);

    let beneficiary = Address::generate(&env);
    let unauthorized_user = Address::generate(&env);
    let asset_address = Address::generate(&env);

    let schedule_ids = vec![&env, 1u32, 2u32];

    // Setup schedules belonging to beneficiary
    setup_mock_schedules(&env, &beneficiary, &asset_address, &schedule_ids);

    // Test with unauthorized user
    let result = client.merge_schedules(&unauthorized_user, &schedule_ids);
    assert_eq!(result.unwrap_err(), Error::UnauthorizedScheduleAccess);
}

#[test]
fn test_merge_schedules_asset_mismatch() {
    let env = Env::default();
    let contract_id = env.register_contract(None, VestingVault);
    let client = VestingVaultClient::new(&env, &contract_id);

    let beneficiary = Address::generate(&env);
    let asset1 = Address::generate(&env);
    let asset2 = Address::generate(&env);

    let schedule_ids = vec![&env, 1u32, 2u32];

    // Setup schedules with different assets
    setup_mock_schedules_with_different_assets(&env, &beneficiary, &asset1, &asset2, &schedule_ids);

    // Test with asset mismatch
    let result = client.merge_schedules(&beneficiary, &schedule_ids);
    assert_eq!(result.unwrap_err(), Error::AssetMismatch);
}

#[test]
fn test_merge_schedules_already_merged() {
    let env = Env::default();
    let contract_id = env.register_contract(None, VestingVault);
    let client = VestingVaultClient::new(&env, &contract_id);

    let beneficiary = Address::generate(&env);
    let asset_address = Address::generate(&env);

    let schedule_ids = vec![&env, 1u32, 2u32];

    // Setup schedules
    setup_mock_schedules(&env, &beneficiary, &asset_address, &schedule_ids);

    // Mark one schedule as already merged
    mark_schedule_merged(&env, schedule_ids.get(0).unwrap());

    // Test with already merged schedule
    let result = client.merge_schedules(&beneficiary, &schedule_ids);
    assert_eq!(result.unwrap_err(), Error::ScheduleNotActive);
}

#[test]
fn test_merge_schedules_unlock_date_acceleration() {
    let env = Env::default();
    let contract_id = env.register_contract(None, VestingVault);
    let client = VestingVaultClient::new(&env, &contract_id);

    let beneficiary = Address::generate(&env);
    let asset_address = Address::generate(&env);

    let schedule_ids = vec![&env, 1u32, 2u32];

    // Setup schedules with very different end times that would cause acceleration
    setup_mock_schedules_with_acceleration_risk(&env, &beneficiary, &asset_address, &schedule_ids);

    // Test with unlock date acceleration risk
    let result = client.merge_schedules(&beneficiary, &schedule_ids);
    assert_eq!(result.unwrap_err(), Error::UnlockDateAcceleration);
}

#[test]
fn test_merge_schedules_weighted_average_calculation() {
    let env = Env::default();
    let contract_id = env.register_contract(None, VestingVault);
    let client = VestingVaultClient::new(&env, &contract_id);

    let beneficiary = Address::generate(&env);
    let asset_address = Address::generate(&env);

    let schedule_ids = vec![&env, 1u32, 2u32];

    // Setup schedules with known values for testing weighted averages
    let current_time = env.ledger().timestamp();
    setup_mock_schedules_for_math_test(&env, &beneficiary, &asset_address, &schedule_ids, current_time);

    let result = client.merge_schedules(&beneficiary, &schedule_ids);
    assert!(result.is_ok());

    let master_id = result.unwrap();
    let master_schedule = client.get_master_schedule(&master_id).unwrap();

    // Verify weighted average calculations
    // Schedule 1: 1000 tokens, start=100, end=1000, cliff=50
    // Schedule 2: 2000 tokens, start=200, end=2000, cliff=100
    // Weighted start = (100*1000 + 200*2000) / 3000 = 166.67
    // Weighted end = (1000*1000 + 2000*2000) / 3000 = 1666.67
    // Weighted cliff = (50*1000 + 100*2000) / 3000 = 83.33
    
    assert_eq!(master_schedule.total_amount, 3000); // 1000 + 2000
    assert_eq!(master_schedule.start_time, 166); // Floor of 166.67
    assert_eq!(master_schedule.end_time, 1666); // Floor of 1666.67
    assert_eq!(master_schedule.cliff_duration, 83); // Floor of 83.33
}

#[test]
fn test_merge_schedules_event_emission() {
    let env = Env::default();
    let contract_id = env.register_contract(None, VestingVault);
    let client = VestingVaultClient::new(&env, &contract_id);

    let beneficiary = Address::generate(&env);
    let asset_address = Address::generate(&env);

    let schedule_ids = vec![&env, 1u32, 2u32];

    // Setup schedules
    setup_mock_schedules(&env, &beneficiary, &asset_address, &schedule_ids);

    // Test event emission
    let result = client.merge_schedules(&beneficiary, &schedule_ids);
    assert!(result.is_ok());

    // Verify the consolidation event was emitted
    let events = env.events().all();
    assert_eq!(events.len(), 1);
    
    let event = events.get(0).unwrap();
    assert_eq!(event.topics.get(0), beneficiary);
    assert_eq!(event.topics.get(1), schedule_ids);
    assert_eq!(event.topics.get(2), result.unwrap());
}

// Helper functions for test setup

fn setup_mock_schedules(env: &Env, beneficiary: &Address, asset_address: &Address, schedule_ids: &Vec<u32>) {
    let current_time = env.ledger().timestamp();
    
    for (i, schedule_id) in schedule_ids.iter().enumerate() {
        let schedule_data = MockScheduleData {
            beneficiary: beneficiary.clone(),
            asset_address: asset_address.clone(),
            total_amount: 1000i128 + (i as i128 * 500),
            claimed_amount: 0i128,
            start_time: current_time + (i as u64 * 1000),
            end_time: current_time + 31536000 + (i as u64 * 1000),
            cliff_duration: 2592000,
        };
        
        store_mock_schedule_data(env, *schedule_id, schedule_data);
    }
}

fn setup_mock_schedules_with_different_assets(env: &Env, beneficiary: &Address, asset1: &Address, asset2: &Address, schedule_ids: &Vec<u32>) {
    let current_time = env.ledger().timestamp();
    
    let schedule1_data = MockScheduleData {
        beneficiary: beneficiary.clone(),
        asset_address: asset1.clone(),
        total_amount: 1000i128,
        claimed_amount: 0i128,
        start_time: current_time,
        end_time: current_time + 31536000,
        cliff_duration: 2592000,
    };
    
    let schedule2_data = MockScheduleData {
        beneficiary: beneficiary.clone(),
        asset_address: asset2.clone(), // Different asset
        total_amount: 1000i128,
        claimed_amount: 0i128,
        start_time: current_time,
        end_time: current_time + 31536000,
        cliff_duration: 2592000,
    };
    
    store_mock_schedule_data(env, *schedule_ids.get(0).unwrap(), schedule1_data);
    store_mock_schedule_data(env, *schedule_ids.get(1).unwrap(), schedule2_data);
}

fn setup_mock_schedules_with_acceleration_risk(env: &Env, beneficiary: &Address, asset_address: &Address, schedule_ids: &Vec<u32>) {
    let current_time = env.ledger().timestamp();
    
    // Schedule 1: Very long duration
    let schedule1_data = MockScheduleData {
        beneficiary: beneficiary.clone(),
        asset_address: asset_address.clone(),
        total_amount: 1000i128,
        claimed_amount: 0i128,
        start_time: current_time,
        end_time: current_time + 63072000, // 2 years
        cliff_duration: 2592000,
    };
    
    // Schedule 2: Very short duration
    let schedule2_data = MockScheduleData {
        beneficiary: beneficiary.clone(),
        asset_address: asset_address.clone(),
        total_amount: 1i128, // Very small amount
        claimed_amount: 0i128,
        start_time: current_time,
        end_time: current_time + 2592000, // 30 days
        cliff_duration: 2592000,
    };
    
    store_mock_schedule_data(env, *schedule_ids.get(0).unwrap(), schedule1_data);
    store_mock_schedule_data(env, *schedule_ids.get(1).unwrap(), schedule2_data);
}

fn setup_mock_schedules_for_math_test(env: &Env, beneficiary: &Address, asset_address: &Address, schedule_ids: &Vec<u32>, base_time: u64) {
    // Schedule 1: 1000 tokens, specific timing
    let schedule1_data = MockScheduleData {
        beneficiary: beneficiary.clone(),
        asset_address: asset_address.clone(),
        total_amount: 1000i128,
        claimed_amount: 0i128,
        start_time: base_time + 100,
        end_time: base_time + 1000,
        cliff_duration: 50,
    };
    
    // Schedule 2: 2000 tokens, different timing
    let schedule2_data = MockScheduleData {
        beneficiary: beneficiary.clone(),
        asset_address: asset_address.clone(),
        total_amount: 2000i128,
        claimed_amount: 0i128,
        start_time: base_time + 200,
        end_time: base_time + 2000,
        cliff_duration: 100,
    };
    
    store_mock_schedule_data(env, *schedule_ids.get(0).unwrap(), schedule1_data);
    store_mock_schedule_data(env, *schedule_ids.get(1).unwrap(), schedule2_data);
}

fn store_mock_schedule_data(env: &Env, schedule_id: u32, data: MockScheduleData) {
    let key = ("MockScheduleData", schedule_id);
    env.storage().instance().set(&key, &data);
}

fn mark_schedule_merged(env: &Env, schedule_id: &u32) {
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
