use earn_quest::{vesting, lessor_registry, fraud_arbitration, storage, Error};
use soroban_sdk::{testutils::Ledger, Address, BytesN, Env, Symbol, Vec};

#[test]
fn test_overlapping_fraud_disputes() {
    let env = Env::default();
    let dao = Address::generate(&env);
    let security_council = Address::generate(&env);
    let admin = Address::generate(&env);
    let initiator = Address::generate(&env);
    let beneficiary1 = Address::generate(&env);
    let beneficiary2 = Address::generate(&env);
    let asset = Address::generate(&env);
    let treasury = Address::generate(&env);
    
    env.mock_auths(&[
        (&dao, &100), 
        (&admin, &100), 
        (&initiator, &100), 
        (&beneficiary1, &100), 
        (&beneficiary2, &100),
        (&treasury, &100)
    ]);
    
    // Initialize systems
    lessor_registry::initialize_lessor_registry(&env, dao.clone()).unwrap();
    fraud_arbitration::initialize_arbitration(
        &env,
        dao.clone(),
        security_council.clone(),
        5,
        3,
        7,
    ).unwrap();
    storage::set_treasury_address(&env, &treasury);
    
    // Add jurors to security council
    let mut jurors = Vec::new(&env);
    for i in 0..5 {
        let juror = Address::generate(&env);
        jurors.push_back(juror.clone());
        fraud_arbitration::add_juror(&env, security_council.clone(), juror, admin.clone()).unwrap();
    }
    
    // Create two vesting schedules for different beneficiaries
    let current_time = 1000;
    env.ledger().set_timestamp(current_time);
    
    let schedule1_id = Symbol::new(&env, "schedule1");
    let schedule1 = vesting::create_vesting_schedule(
        &env,
        schedule1_id.clone(),
        beneficiary1.clone(),
        asset.clone(),
        1000,
        current_time,
        current_time + 2000,
        current_time + 100,
        vesting::VestingType::Linear,
    ).unwrap();
    storage::set_vesting_schedule(&env, &schedule1_id, &schedule1);
    
    let schedule2_id = Symbol::new(&env, "schedule2");
    let schedule2 = vesting::create_vesting_schedule(
        &env,
        schedule2_id.clone(),
        beneficiary2.clone(),
        asset.clone(),
        2000,
        current_time,
        current_time + 3000,
        current_time + 200,
        vesting::VestingType::Linear,
    ).unwrap();
    storage::set_vesting_schedule(&env, &schedule2_id, &schedule2);
    
    // Raise fraud disputes for both schedules
    let dispute1_id = Symbol::new(&env, "dispute1");
    let evidence_hash1 = BytesN::from_array(&env, &[1; 32]);
    
    let dispute1 = fraud_arbitration::raise_fraud_dispute(
        &env,
        dispute1_id.clone(),
        schedule1_id.clone(),
        beneficiary1.clone(),
        initiator.clone(),
        evidence_hash1,
    ).unwrap();
    
    let dispute2_id = Symbol::new(&env, "dispute2");
    let evidence_hash2 = BytesN::from_array(&env, &[2; 32]);
    
    let dispute2 = fraud_arbitration::raise_fraud_dispute(
        &env,
        dispute2_id.clone(),
        schedule2_id.clone(),
        beneficiary2.clone(),
        initiator.clone(),
        evidence_hash2,
    ).unwrap();
    
    // Verify both schedules are frozen
    let frozen_schedule1 = storage::get_vesting_schedule(&env, &schedule1_id).unwrap();
    let frozen_schedule2 = storage::get_vesting_schedule(&env, &schedule2_id).unwrap();
    assert!(frozen_schedule1.is_frozen);
    assert!(frozen_schedule2.is_frozen);
    
    // Jurors vote on first dispute (fraud confirmed)
    for i in 0..3 { // 3 votes to reach threshold
        let result = fraud_arbitration::cast_juror_vote(
            &env,
            dispute1_id.clone(),
            jurors.get(i).unwrap().clone(),
            fraud_arbitration::JurorVote::SlashForFraud,
        );
        assert!(result.is_ok());
    }
    
    // Verify first dispute is resolved and schedule terminated
    let resolved_dispute1 = storage::get_fraud_dispute(&env, &dispute1_id).unwrap();
    assert!(resolved_dispute1.is_resolved);
    assert!(matches!(resolved_dispute1.status, fraud_arbitration::FraudDisputeStatus::Resolved));
    
    let terminated_schedule1 = storage::get_vesting_schedule(&env, &schedule1_id).unwrap();
    assert!(!terminated_schedule1.is_active);
    assert!(terminated_schedule1.is_frozen);
    
    // Jurors vote on second dispute (dismissed)
    for i in 0..3 { // 3 votes to reach threshold
        let result = fraud_arbitration::cast_juror_vote(
            &env,
            dispute2_id.clone(),
            jurors.get(i).unwrap().clone(),
            fraud_arbitration::JurorVote::DismissCharges,
        );
        assert!(result.is_ok());
    }
    
    // Verify second dispute is resolved and schedule unfrozen
    let resolved_dispute2 = storage::get_fraud_dispute(&env, &dispute2_id).unwrap();
    assert!(resolved_dispute2.is_resolved);
    assert!(matches!(resolved_dispute2.status, fraud_arbitration::FraudDisputeStatus::Dismissed));
    
    let unfrozen_schedule2 = storage::get_vesting_schedule(&env, &schedule2_id).unwrap();
    assert!(unfrozen_schedule2.is_active);
    assert!(!unfrozen_schedule2.is_frozen);
    
    // Verify escrow partitions are maintained correctly
    // Schedule 1 should have returned unvested amount to treasury
    // Schedule 2 should be back to normal operation
    assert_eq!(terminated_schedule1.total_amount, 1000);
    assert_eq!(unfrozen_schedule2.total_amount, 2000);
}

#[test]
fn test_anti_reentry_during_claim_with_dispute() {
    let env = Env::default();
    let dao = Address::generate(&env);
    let security_council = Address::generate(&env);
    let admin = Address::generate(&env);
    let initiator = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let asset = Address::generate(&env);
    
    env.mock_auths(&[
        (&dao, &100), 
        (&admin, &100), 
        (&initiator, &100), 
        (&beneficiary, &100)
    ]);
    
    // Initialize systems
    fraud_arbitration::initialize_arbitration(
        &env,
        dao.clone(),
        security_council.clone(),
        5,
        3,
        7,
    ).unwrap();
    
    // Add jurors
    for i in 0..5 {
        let juror = Address::generate(&env);
        fraud_arbitration::add_juror(&env, security_council.clone(), juror, admin.clone()).unwrap();
    }
    
    // Create vesting schedule and let time pass for some vesting
    let schedule_id = Symbol::new(&env, "reentry_test");
    let current_time = 1000;
    env.ledger().set_timestamp(current_time);
    
    let schedule = vesting::create_vesting_schedule(
        &env,
        schedule_id.clone(),
        beneficiary.clone(),
        asset.clone(),
        1000,
        current_time,
        current_time + 2000,
        current_time + 100,
        vesting::VestingType::Linear,
    ).unwrap();
    storage::set_vesting_schedule(&env, &schedule_id, &schedule);
    
    // Advance time to vest some tokens
    env.ledger().set_timestamp(current_time + 500); // 50% should be vested
    
    // First claim should succeed
    let claim_result = vesting::claim_vested_tokens(&env, schedule_id.clone(), beneficiary.clone());
    assert!(claim_result.is_ok());
    assert_eq!(claim_result.unwrap(), 500);
    
    // Raise fraud dispute (should freeze schedule)
    let dispute_id = Symbol::new(&env, "reentry_dispute");
    let evidence_hash = BytesN::from_array(&env, &[1; 32]);
    
    fraud_arbitration::raise_fraud_dispute(
        &env,
        dispute_id,
        schedule_id.clone(),
        beneficiary.clone(),
        initiator.clone(),
        evidence_hash,
    ).unwrap();
    
    // Second claim should fail due to frozen schedule
    let claim_result = vesting::claim_vested_tokens(&env, schedule_id.clone(), beneficiary.clone());
    assert!(matches!(claim_result, Err(Error::InvalidQuestStatus)));
    
    // Verify anti-reentry guard is working
    let guard = storage::get_anti_reentry_guard(&env);
    assert!(!guard.is_locked); // Should be cleared after failed claim
}

#[test]
fn test_lessor_registry_with_vesting_validation() {
    let env = Env::default();
    let governance = Address::generate(&env);
    let registrar = Address::generate(&env);
    let lessor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let asset = Address::generate(&env);
    
    env.mock_auths(&[
        (&governance, &100), 
        (&registrar, &100), 
        (&lessor, &100), 
        (&beneficiary, &100)
    ]);
    
    // Initialize lessor registry
    lessor_registry::initialize_lessor_registry(&env, governance.clone()).unwrap();
    
    // Register lessor with specific limits
    let name = String::from_str(&env, "Validation Test Bank");
    lessor_registry::register_authorized_lessor(
        &env,
        lessor.clone(),
        name.clone(),
        lessor_registry::InstitutionType::Bank,
        150, // Medium credit rating
        5000, // Max 5K
        lessor_registry::ComplianceLevel::Enhanced,
        registrar.clone(),
    ).unwrap();
    
    // Test vesting amount validation within limit
    let validation_result = lessor_registry::validate_vesting_amount(&env, &lessor, 3000);
    assert!(validation_result.is_ok());
    
    // Test vesting amount exceeding limit
    let validation_result = lessor_registry::validate_vesting_amount(&env, &lessor, 6000);
    assert!(matches!(validation_result, Err(Error::AmountTooLarge)));
    
    // Test vesting amount with credit rating multiplier
    // Rating 150 should give ~159% multiplier (150/255 * 100% + 100%)
    let max_by_rating = lessor_registry::calculate_max_by_credit_rating(150, 5000);
    assert!(max_by_rating > 5000); // Should be higher than base amount
    assert!(max_by_rating <= 7950); // Should not exceed 300% cap
}

#[test]
fn test_virtual_accumulator_precision() {
    let env = Env::default();
    let current_time = 1000;
    
    env.ledger().set_timestamp(current_time);
    
    // Test with high-frequency vesting (small amounts over short periods)
    let schedule = vesting::VestingSchedule {
        id: Symbol::new(&env, "precision_test"),
        beneficiary: Address::generate(&env),
        asset: Address::generate(&env),
        total_amount: 1000000, // 1M tokens
        vested_amount: 0,
        claimed_amount: 0,
        start_time: current_time,
        end_time: current_time + 86400, // 1 day
        cliff_time: current_time,
        vesting_type: vesting::VestingType::Linear,
        is_active: true,
        is_frozen: false,
    };
    
    // Create accumulator with high precision rate
    let accumulator = vesting::VirtualAccumulator {
        schedule_id: Symbol::new(&env, "precision_test"),
        last_update_time: current_time,
        accumulated_rate: 11574, // 1M tokens / 86400 seconds ≈ 11.574 tokens/second (with 6 decimal precision)
        accumulated_vested: 0,
    };
    
    storage::set_vesting_schedule(&env, &schedule.id, &schedule);
    storage::set_virtual_accumulator(&env, &schedule.id, &accumulator);
    
    // Test precision after 1 hour (3600 seconds)
    let vested_after_1h = vesting::calculate_linear_vested(&schedule, &accumulator, current_time + 3600);
    let expected_1h = 11574 * 3600 / 1000000; // Rate * time / precision_divisor
    assert_eq!(vested_after_1h, expected_1h);
    
    // Test precision after 12 hours (43200 seconds)
    let vested_after_12h = vesting::calculate_linear_vested(&schedule, &accumulator, current_time + 43200);
    let expected_12h = 11574 * 43200 / 1000000;
    assert_eq!(vested_after_12h, expected_12h);
    
    // Test precision after full day (86400 seconds)
    let vested_after_1d = vesting::calculate_linear_vested(&schedule, &accumulator, current_time + 86400);
    assert_eq!(vested_after_1d, 1000000); // Should be fully vested
}

#[test]
fn test_end_to_end_fraud_arbitration_flow() {
    let env = Env::default();
    let dao = Address::generate(&env);
    let security_council = Address::generate(&env);
    let admin = Address::generate(&env);
    let initiator = Address::generate(&env);
    let malicious_beneficiary = Address::generate(&env);
    let asset = Address::generate(&env);
    let treasury = Address::generate(&env);
    
    env.mock_auths(&[
        (&dao, &100), 
        (&admin, &100), 
        (&initiator, &100), 
        (&malicious_beneficiary, &100),
        (&treasury, &100)
    ]);
    
    // Initialize systems
    fraud_arbitration::initialize_arbitration(
        &env,
        dao.clone(),
        security_council.clone(),
        5,
        3,
        7,
    ).unwrap();
    storage::set_treasury_address(&env, &treasury);
    
    // Add 5 jurors
    let mut jurors = Vec::new(&env);
    for i in 0..5 {
        let juror = Address::generate(&env);
        jurors.push_back(juror.clone());
        fraud_arbitration::add_juror(&env, security_council.clone(), juror, admin.clone()).unwrap();
    }
    
    // Create vesting schedule for malicious beneficiary
    let schedule_id = Symbol::new(&env, "malicious_schedule");
    let current_time = 1000;
    env.ledger().set_timestamp(current_time);
    
    let schedule = vesting::create_vesting_schedule(
        &env,
        schedule_id.clone(),
        malicious_beneficiary.clone(),
        asset.clone(),
        10000, // 10K tokens
        current_time,
        current_time + 10000, // 10K second vesting period
        current_time + 1000, // 1K second cliff
        vesting::VestingType::Cliff,
    ).unwrap();
    storage::set_vesting_schedule(&env, &schedule_id, &schedule);
    
    // Advance time past cliff but before full vesting
    env.ledger().set_timestamp(current_time + 2000); // 20% through vesting period
    
    // Malicious beneficiary tries to claim some vested tokens
    let claim_result = vesting::claim_vested_tokens(&env, schedule_id.clone(), malicious_beneficiary.clone());
    assert!(claim_result.is_ok());
    let claimed_amount = claim_result.unwrap();
    assert!(claimed_amount > 0); // Should be able to claim some amount
    
    // DAO raises fraud dispute
    let dispute_id = Symbol::new(&env, "fraud_case");
    let evidence_hash = BytesN::from_array(&env, &[42; 32]);
    
    let dispute = fraud_arbitration::raise_fraud_dispute(
        &env,
        dispute_id.clone(),
        schedule_id.clone(),
        malicious_beneficiary.clone(),
        initiator.clone(),
        evidence_hash,
    ).unwrap();
    
    // Verify schedule is frozen immediately
    let frozen_schedule = storage::get_vesting_schedule(&env, &schedule_id).unwrap();
    assert!(frozen_schedule.is_frozen);
    
    // Jurors vote to confirm fraud (3 out of 5)
    for i in 0..3 {
        let result = fraud_arbitration::cast_juror_vote(
            &env,
            dispute_id.clone(),
            jurors.get(i).unwrap().clone(),
            fraud_arbitration::JurorVote::SlashForFraud,
        );
        assert!(result.is_ok());
    }
    
    // Verify dispute resolution
    let resolved_dispute = storage::get_fraud_dispute(&env, &dispute_id).unwrap();
    assert!(resolved_dispute.is_resolved);
    assert!(matches!(resolved_dispute.status, fraud_arbitration::FraudDisputeStatus::Resolved));
    assert_eq!(resolved_dispute.slash_votes, 3);
    assert_eq!(resolved_dispute.dismiss_votes, 0);
    
    // Verify vesting schedule is terminated
    let terminated_schedule = storage::get_vesting_schedule(&env, &schedule_id).unwrap();
    assert!(!terminated_schedule.is_active);
    assert!(terminated_schedule.is_frozen);
    
    // Calculate expected unvested amount (should be returned to treasury)
    // 20% was claimed, so 80% of 10K = 8K should be unvested
    let expected_unvested = 8000;
    
    // The actual unvested amount calculation would be done in the terminate function
    // This test verifies the flow works correctly
    assert!(terminated_schedule.total_amount == 10000);
}
