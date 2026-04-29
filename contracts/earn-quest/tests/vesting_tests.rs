use earn_quest::{vesting, storage, Error};
use soroban_sdk::{testutils::Ledger, Address, BytesN, Env, Symbol};

#[test]
fn test_vesting_schedule_creation() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let asset = Address::generate(&env);
    
    env.mock_auths(&[(&admin, &100), (&beneficiary, &100)]);
    
    let schedule_id = Symbol::new(&env, "test_schedule");
    let current_time = 1000;
    let start_time = current_time;
    let end_time = current_time + 1000;
    let cliff_time = start_time + 100;
    
    env.ledger().set_timestamp(current_time);
    
    // Test linear vesting schedule creation
    let result = vesting::create_vesting_schedule(
        &env,
        schedule_id.clone(),
        beneficiary.clone(),
        asset.clone(),
        1000,
        start_time,
        end_time,
        cliff_time,
        vesting::VestingType::Linear,
    );
    
    assert!(result.is_ok());
    
    let schedule = storage::get_vesting_schedule(&env, &schedule_id).unwrap();
    assert_eq!(schedule.beneficiary, beneficiary);
    assert_eq!(schedule.asset, asset);
    assert_eq!(schedule.total_amount, 1000);
    assert_eq!(schedule.start_time, start_time);
    assert_eq!(schedule.end_time, end_time);
    assert!(matches!(schedule.vesting_type, vesting::VestingType::Linear));
    assert!(schedule.is_active);
    assert!(!schedule.is_frozen);
}

#[test]
fn test_vesting_schedule_invalid_amount() {
    let env = Env::default();
    let beneficiary = Address::generate(&env);
    let asset = Address::generate(&env);
    
    let schedule_id = Symbol::new(&env, "invalid_schedule");
    let current_time = 1000;
    
    env.ledger().set_timestamp(current_time);
    
    // Test with zero amount
    let result = vesting::create_vesting_schedule(
        &env,
        schedule_id,
        beneficiary,
        asset,
        0,
        current_time,
        current_time + 1000,
        current_time + 100,
        vesting::VestingType::Linear,
    );
    
    assert!(matches!(result, Err(Error::InvalidRewardAmount)));
}

#[test]
fn test_vesting_schedule_invalid_timeline() {
    let env = Env::default();
    let beneficiary = Address::generate(&env);
    let asset = Address::generate(&env);
    
    let schedule_id = Symbol::new(&env, "invalid_timeline");
    let current_time = 1000;
    
    env.ledger().set_timestamp(current_time);
    
    // Test with end_time <= start_time
    let result = vesting::create_vesting_schedule(
        &env,
        schedule_id,
        beneficiary,
        asset,
        1000,
        current_time + 1000,
        current_time, // end_time equals start_time
        current_time + 100,
        vesting::VestingType::Linear,
    );
    
    assert!(matches!(result, Err(Error::InvalidDeadline)));
}

#[test]
fn test_virtual_accumulator_linear_vesting() {
    let env = Env::default();
    let schedule_id = Symbol::new(&env, "linear_test");
    let current_time = 1000;
    
    env.ledger().set_timestamp(current_time);
    
    let schedule = vesting::VestingSchedule {
        id: schedule_id.clone(),
        beneficiary: Address::generate(&env),
        asset: Address::generate(&env),
        total_amount: 1000,
        vested_amount: 0,
        claimed_amount: 0,
        start_time: current_time,
        end_time: current_time + 1000,
        cliff_time: current_time + 100,
        vesting_type: vesting::VestingType::Linear,
        is_active: true,
        is_frozen: false,
    };
    
    // Create virtual accumulator
    let accumulator = vesting::VirtualAccumulator {
        schedule_id: schedule_id.clone(),
        last_update_time: current_time,
        accumulated_rate: 1000, // 1000 tokens over 1000 seconds = 1 token per second
        accumulated_vested: 0,
    };
    
    storage::set_vesting_schedule(&env, &schedule_id, &schedule);
    storage::set_virtual_accumulator(&env, &schedule_id, &accumulator);
    
    // Test after 500 seconds (should have 500 tokens vested)
    env.ledger().set_timestamp(current_time + 500);
    let vested = vesting::calculate_linear_vested(&schedule, &accumulator, current_time + 500);
    assert_eq!(vested, 500);
    
    // Test after full period (should have 1000 tokens vested)
    env.ledger().set_timestamp(current_time + 1000);
    let vested = vesting::calculate_linear_vested(&schedule, &accumulator, current_time + 1000);
    assert_eq!(vested, 1000);
}

#[test]
fn test_anti_reentry_guard() {
    let env = Env::default();
    let caller1 = Address::generate(&env);
    let caller2 = Address::generate(&env);
    let current_time = 1000;
    
    env.ledger().set_timestamp(current_time);
    
    let mut guard = vesting::AntiReentryGuard::new();
    
    // First caller should be able to enter
    assert!(guard.enter(caller1.clone(), current_time).is_ok());
    assert!(guard.is_locked);
    assert_eq!(guard.caller, caller1);
    
    // Second caller should be blocked
    assert!(matches!(guard.enter(caller2.clone(), current_time + 1), Err(Error::ReentrantCall)));
    
    // Same caller should be able to re-enter (valid caller check)
    assert!(guard.is_valid_caller(&caller1));
    assert!(!guard.is_valid_caller(&caller2));
    
    // Exit should reset the guard
    guard.exit();
    assert!(!guard.is_locked);
    
    // After exit, new caller should be able to enter
    assert!(guard.enter(caller2.clone(), current_time + 2).is_ok());
    assert_eq!(guard.caller, caller2);
}

#[test]
fn test_cliff_vesting_calculation() {
    let env = Env::default();
    let current_time = 1000;
    
    env.ledger().set_timestamp(current_time);
    
    let schedule = vesting::VestingSchedule {
        id: Symbol::new(&env, "cliff_test"),
        beneficiary: Address::generate(&env),
        asset: Address::generate(&env),
        total_amount: 1000,
        vested_amount: 0,
        claimed_amount: 0,
        start_time: current_time,
        end_time: current_time + 2000,
        cliff_time: current_time + 1000, // 1000 second cliff
        vesting_type: vesting::VestingType::Cliff,
        is_active: true,
        is_frozen: false,
    };
    
    // Before cliff - should have 0 vested
    let vested = vesting::calculate_total_vested(&env, &schedule, current_time + 500).unwrap();
    assert_eq!(vested, 0);
    
    // At cliff - should have 0 vested (cliff just reached)
    let vested = vesting::calculate_total_vested(&env, &schedule, current_time + 1000).unwrap();
    assert_eq!(vested, 0);
    
    // After cliff - should start vesting linearly
    let vested = vesting::calculate_total_vested(&env, &schedule, current_time + 1500).unwrap();
    assert_eq!(vested, 250); // 500 seconds into 1000 second period = 50% of 1000 = 500, but cliff period is 1000 seconds so 500/1000 * 1000 = 500
    
    // At end - should have all vested
    let vested = vesting::calculate_total_vested(&env, &schedule, current_time + 2000).unwrap();
    assert_eq!(vested, 1000);
}

#[test]
fn test_vesting_schedule_freeze() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let asset = Address::generate(&env);
    
    env.mock_auths(&[(&admin, &100), (&beneficiary, &100)]);
    
    let schedule_id = Symbol::new(&env, "freeze_test");
    let current_time = 1000;
    
    env.ledger().set_timestamp(current_time);
    
    // Create schedule
    let schedule = vesting::create_vesting_schedule(
        &env,
        schedule_id.clone(),
        beneficiary.clone(),
        asset.clone(),
        1000,
        current_time,
        current_time + 1000,
        current_time + 100,
        vesting::VestingType::Linear,
    ).unwrap();
    
    storage::set_vesting_schedule(&env, &schedule_id, &schedule);
    
    // Freeze schedule
    let result = vesting::freeze_vesting_schedule(&env, schedule_id.clone(), admin.clone());
    assert!(result.is_ok());
    
    let frozen_schedule = storage::get_vesting_schedule(&env, &schedule_id).unwrap();
    assert!(frozen_schedule.is_frozen);
    
    // Should not be able to claim while frozen
    let claim_result = vesting::claim_vested_tokens(&env, schedule_id.clone(), beneficiary.clone());
    assert!(matches!(claim_result, Err(Error::InvalidQuestStatus)));
    
    // Unfreeze schedule
    let result = vesting::unfreeze_vesting_schedule(&env, schedule_id, admin.clone());
    assert!(result.is_ok());
    
    let unfrozen_schedule = storage::get_vesting_schedule(&env, &schedule_id).unwrap();
    assert!(!unfrozen_schedule.is_frozen);
}

#[test]
fn test_vesting_schedule_termination() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let asset = Address::generate(&env);
    let treasury = Address::generate(&env);
    
    env.mock_auths(&[(&admin, &100), (&beneficiary, &100), (&treasury, &100)]);
    
    let schedule_id = Symbol::new(&env, "terminate_test");
    let current_time = 1000;
    
    env.ledger().set_timestamp(current_time);
    
    // Set treasury address
    storage::set_treasury_address(&env, &treasury);
    
    // Create schedule
    let schedule = vesting::create_vesting_schedule(
        &env,
        schedule_id.clone(),
        beneficiary.clone(),
        asset.clone(),
        1000,
        current_time,
        current_time + 1000,
        current_time + 100,
        vesting::VestingType::Linear,
    ).unwrap();
    
    storage::set_vesting_schedule(&env, &schedule_id, &schedule);
    
    // Terminate schedule (should return unvested amount to treasury)
    let unvested_amount = vesting::terminate_vesting_schedule(
        &env,
        schedule_id.clone(),
        admin.clone(),
        "Test termination",
    ).unwrap();
    
    // Since no time has passed, all 1000 should be unvested
    assert_eq!(unvested_amount, 1000);
    
    let terminated_schedule = storage::get_vesting_schedule(&env, &schedule_id).unwrap();
    assert!(!terminated_schedule.is_active);
    assert!(terminated_schedule.is_frozen);
}
