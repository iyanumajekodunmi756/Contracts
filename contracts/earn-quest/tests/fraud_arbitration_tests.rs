use earn_quest::{fraud_arbitration, storage, vesting, Error};
use soroban_sdk::{testutils::Ledger, Address, BytesN, Env, Symbol, Vec};

#[test]
fn test_arbitration_initialization() {
    let env = Env::default();
    let dao = Address::generate(&env);
    let security_council = Address::generate(&env);
    let admin = Address::generate(&env);
    
    env.mock_auths(&[(&dao, &100), (&admin, &100)]);
    
    // Initialize arbitration system
    let result = fraud_arbitration::initialize_arbitration(
        &env,
        dao.clone(),
        security_council.clone(),
        5, // required_jurors
        3, // voting_threshold
        7, // voting_period_days
    );
    
    assert!(result.is_ok());
    
    let config = storage::get_arbitration_config(&env);
    assert_eq!(config.dao_address, dao);
    assert_eq!(config.security_council_address, security_council);
    assert_eq!(config.required_jurors, 5);
    assert_eq!(config.voting_threshold, 3);
    assert_eq!(config.voting_period_seconds, 7 * 24 * 60 * 60);
    assert!(storage::is_arbitration_initialized(&env));
}

#[test]
fn test_arbitration_double_initialization() {
    let env = Env::default();
    let dao = Address::generate(&env);
    let security_council = Address::generate(&env);
    let admin = Address::generate(&env);
    
    env.mock_auths(&[(&dao, &100), (&admin, &100)]);
    
    // Initialize arbitration system twice should fail
    fraud_arbitration::initialize_arbitration(
        &env,
        dao.clone(),
        security_council.clone(),
        5,
        3,
        7,
    ).unwrap();
    
    let result = fraud_arbitration::initialize_arbitration(
        &env,
        dao,
        security_council,
        5,
        3,
        7,
    );
    
    assert!(matches!(result, Err(Error::AlreadyInitialized)));
}

#[test]
fn test_add_juror_to_security_council() {
    let env = Env::default();
    let dao = Address::generate(&env);
    let security_council = Address::generate(&env);
    let admin = Address::generate(&env);
    let juror1 = Address::generate(&env);
    let juror2 = Address::generate(&env);
    
    env.mock_auths(&[(&dao, &100), (&admin, &100)]);
    
    // Initialize arbitration system
    fraud_arbitration::initialize_arbitration(
        &env,
        dao.clone(),
        security_council.clone(),
        5,
        3,
        7,
    ).unwrap();
    
    // Add first juror
    let result = fraud_arbitration::add_juror(&env, security_council.clone(), juror1.clone(), admin.clone());
    assert!(result.is_ok());
    
    let pool = storage::get_juror_pool(&env, &security_council).unwrap();
    assert_eq!(pool.jurors.len(), 1);
    assert!(pool.jurors.contains(&juror1));
    
    // Add second juror
    let result = fraud_arbitration::add_juror(&env, security_council.clone(), juror2.clone(), admin.clone());
    assert!(result.is_ok());
    
    let pool = storage::get_juror_pool(&env, &security_council).unwrap();
    assert_eq!(pool.jurors.len(), 2);
    assert!(pool.jurors.contains(&juror2));
}

#[test]
fn test_add_juror_unauthorized() {
    let env = Env::default();
    let dao = Address::generate(&env);
    let security_council = Address::generate(&env);
    let admin = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let juror = Address::generate(&env);
    
    env.mock_auths(&[(&dao, &100), (&admin, &100), (&unauthorized, &100)]);
    
    // Initialize arbitration system
    fraud_arbitration::initialize_arbitration(
        &env,
        dao,
        security_council.clone(),
        5,
        3,
        7,
    ).unwrap();
    
    // Unauthorized user should not be able to add juror
    let result = fraud_arbitration::add_juror(&env, security_council.clone(), juror, unauthorized);
    assert!(matches!(result, Err(Error::Unauthorized)));
}

#[test]
fn test_remove_juror_from_security_council() {
    let env = Env::default();
    let dao = Address::generate(&env);
    let security_council = Address::generate(&env);
    let admin = Address::generate(&env);
    let juror = Address::generate(&env);
    
    env.mock_auths(&[(&dao, &100), (&admin, &100)]);
    
    // Initialize arbitration system and add juror
    fraud_arbitration::initialize_arbitration(
        &env,
        dao.clone(),
        security_council.clone(),
        5,
        3,
        7,
    ).unwrap();
    fraud_arbitration::add_juror(&env, security_council.clone(), juror.clone(), admin.clone()).unwrap();
    
    // Verify juror was added
    let pool = storage::get_juror_pool(&env, &security_council).unwrap();
    assert_eq!(pool.jurors.len(), 1);
    
    // Remove juror
    let result = fraud_arbitration::remove_juror(&env, security_council.clone(), juror.clone(), admin.clone());
    assert!(result.is_ok());
    
    // Verify juror was removed
    let pool = storage::get_juror_pool(&env, &security_council).unwrap();
    assert_eq!(pool.jurors.len(), 0);
    assert!(!pool.jurors.contains(&juror));
}

#[test]
fn test_raise_fraud_dispute() {
    let env = Env::default();
    let dao = Address::generate(&env);
    let security_council = Address::generate(&env);
    let admin = Address::generate(&env);
    let initiator = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let asset = Address::generate(&env);
    
    env.mock_auths(&[(&dao, &100), (&admin, &100), (&initiator, &100), (&beneficiary, &100)]);
    
    // Initialize arbitration system and add jurors
    fraud_arbitration::initialize_arbitration(
        &env,
        dao.clone(),
        security_council.clone(),
        5,
        3,
        7,
    ).unwrap();
    
    // Add 5 jurors to security council
    for i in 0..5 {
        let juror = Address::generate(&env);
        fraud_arbitration::add_juror(&env, security_council.clone(), juror, admin.clone()).unwrap();
    }
    
    // Create vesting schedule to target
    let schedule_id = Symbol::new(&env, "target_schedule");
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
    
    // Raise fraud dispute
    let dispute_id = Symbol::new(&env, "fraud_dispute_1");
    let evidence_hash = BytesN::from_array(&env, &[1; 32]);
    
    let result = fraud_arbitration::raise_fraud_dispute(
        &env,
        dispute_id.clone(),
        schedule_id.clone(),
        beneficiary.clone(),
        initiator.clone(),
        evidence_hash,
    );
    
    assert!(result.is_ok());
    
    let dispute = result.unwrap();
    assert_eq!(dispute.id, dispute_id);
    assert_eq!(dispute.target_schedule, schedule_id);
    assert_eq!(dispute.target_beneficiary, beneficiary);
    assert_eq!(dispute.initiator, initiator);
    assert_eq!(dispute.evidence_hash, evidence_hash);
    assert!(matches!(dispute.status, fraud_arbitration::FraudDisputeStatus::UnderReview));
    assert_eq!(dispute.jurors.len(), 5); // Should have 5 jurors
    assert_eq!(dispute.slash_votes, 0);
    assert_eq!(dispute.dismiss_votes, 0);
    assert!(!dispute.is_resolved);
    
    // Verify target schedule is frozen
    let frozen_schedule = storage::get_vesting_schedule(&env, &schedule_id).unwrap();
    assert!(frozen_schedule.is_frozen);
}

#[test]
fn test_raise_fraud_dispute_unauthorized() {
    let env = Env::default();
    let unauthorized = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let asset = Address::generate(&env);
    
    env.mock_auths(&[(&unauthorized, &100), (&beneficiary, &100)]);
    
    // Try to raise fraud dispute without authorization
    let dispute_id = Symbol::new(&env, "unauthorized_dispute");
    let schedule_id = Symbol::new(&env, "target_schedule");
    let evidence_hash = BytesN::from_array(&env, &[1; 32]);
    
    let result = fraud_arbitration::raise_fraud_dispute(
        &env,
        dispute_id,
        schedule_id,
        beneficiary,
        unauthorized,
        evidence_hash,
    );
    
    assert!(matches!(result, Err(Error::Unauthorized)));
}

#[test]
fn test_raise_fraud_dispute_already_exists() {
    let env = Env::default();
    let dao = Address::generate(&env);
    let security_council = Address::generate(&env);
    let admin = Address::generate(&env);
    let initiator = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let asset = Address::generate(&env);
    
    env.mock_auths(&[(&dao, &100), (&admin, &100), (&initiator, &100), (&beneficiary, &100)]);
    
    // Initialize arbitration system and add jurors
    fraud_arbitration::initialize_arbitration(
        &env,
        dao.clone(),
        security_council.clone(),
        5,
        3,
        7,
    ).unwrap();
    
    // Add 5 jurors
    for i in 0..5 {
        let juror = Address::generate(&env);
        fraud_arbitration::add_juror(&env, security_council.clone(), juror, admin.clone()).unwrap();
    }
    
    // Create vesting schedule
    let schedule_id = Symbol::new(&env, "duplicate_schedule");
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
    
    // Raise first dispute
    let dispute_id_1 = Symbol::new(&env, "dispute_1");
    let evidence_hash = BytesN::from_array(&env, &[1; 32]);
    
    fraud_arbitration::raise_fraud_dispute(
        &env,
        dispute_id_1,
        schedule_id.clone(),
        beneficiary.clone(),
        initiator.clone(),
        evidence_hash,
    ).unwrap();
    
    // Try to raise second dispute for same schedule should fail
    let dispute_id_2 = Symbol::new(&env, "dispute_2");
    let evidence_hash_2 = BytesN::from_array(&env, &[2; 32]);
    
    let result = fraud_arbitration::raise_fraud_dispute(
        &env,
        dispute_id_2,
        schedule_id,
        beneficiary,
        initiator,
        evidence_hash_2,
    );
    
    assert!(matches!(result, Err(Error::FraudDisputeAlreadyExists)));
}

#[test]
fn test_juror_voting() {
    let env = Env::default();
    let dao = Address::generate(&env);
    let security_council = Address::generate(&env);
    let admin = Address::generate(&env);
    let initiator = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let asset = Address::generate(&env);
    
    env.mock_auths(&[(&dao, &100), (&admin, &100), (&initiator, &100), (&beneficiary, &100)]);
    
    // Initialize arbitration system and add jurors
    fraud_arbitration::initialize_arbitration(
        &env,
        dao.clone(),
        security_council.clone(),
        3, // Only 3 jurors for simpler test
        2, // 2 votes needed
        7,
    ).unwrap();
    
    // Add 3 jurors
    let mut jurors = Vec::new(&env);
    for i in 0..3 {
        let juror = Address::generate(&env);
        jurors.push_back(juror.clone());
        fraud_arbitration::add_juror(&env, security_council.clone(), juror, admin.clone()).unwrap();
    }
    
    // Create vesting schedule and raise dispute
    let schedule_id = Symbol::new(&env, "voting_schedule");
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
    
    let dispute_id = Symbol::new(&env, "voting_dispute");
    let evidence_hash = BytesN::from_array(&env, &[1; 32]);
    
    let dispute = fraud_arbitration::raise_fraud_dispute(
        &env,
        dispute_id.clone(),
        schedule_id.clone(),
        beneficiary.clone(),
        initiator.clone(),
        evidence_hash,
    ).unwrap();
    
    // First juror votes to slash
    let result = fraud_arbitration::cast_juror_vote(
        &env,
        dispute_id.clone(),
        jurors.get(0).unwrap().clone(),
        fraud_arbitration::JurorVote::SlashForFraud,
    );
    assert!(result.is_ok());
    
    // Check vote count
    let updated_dispute = storage::get_fraud_dispute(&env, &dispute_id).unwrap();
    assert_eq!(updated_dispute.slash_votes, 1);
    assert_eq!(updated_dispute.dismiss_votes, 0);
    assert!(!updated_dispute.is_resolved);
    
    // Second juror votes to slash (should reach threshold)
    let result = fraud_arbitration::cast_juror_vote(
        &env,
        dispute_id.clone(),
        jurors.get(1).unwrap().clone(),
        fraud_arbitration::JurorVote::SlashForFraud,
    );
    assert!(result.is_ok());
    
    // Check that dispute is resolved
    let resolved_dispute = storage::get_fraud_dispute(&env, &dispute_id).unwrap();
    assert_eq!(resolved_dispute.slash_votes, 2);
    assert_eq!(resolved_dispute.dismiss_votes, 0);
    assert!(resolved_dispute.is_resolved);
    assert!(matches!(resolved_dispute.status, fraud_arbitration::FraudDisputeStatus::Resolved));
    
    // Verify vesting schedule is terminated
    let terminated_schedule = storage::get_vesting_schedule(&env, &schedule_id).unwrap();
    assert!(!terminated_schedule.is_active);
    assert!(terminated_schedule.is_frozen);
}

#[test]
fn test_juror_voting_unauthorized() {
    let env = Env::default();
    let dao = Address::generate(&env);
    let security_council = Address::generate(&env);
    let admin = Address::generate(&env);
    let initiator = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let asset = Address::generate(&env);
    let unauthorized_juror = Address::generate(&env);
    
    env.mock_auths(&[(&dao, &100), (&admin, &100), (&initiator, &100), (&beneficiary, &100), (&unauthorized_juror, &100)]);
    
    // Initialize arbitration system and add authorized jurors
    fraud_arbitration::initialize_arbitration(
        &env,
        dao.clone(),
        security_council.clone(),
        3,
        2,
        7,
    ).unwrap();
    
    // Add 3 authorized jurors
    for i in 0..3 {
        let juror = Address::generate(&env);
        fraud_arbitration::add_juror(&env, security_council.clone(), juror, admin.clone()).unwrap();
    }
    
    // Create vesting schedule and raise dispute
    let schedule_id = Symbol::new(&env, "unauthorized_voting");
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
    
    let dispute_id = Symbol::new(&env, "unauthorized_dispute");
    let evidence_hash = BytesN::from_array(&env, &[1; 32]);
    
    fraud_arbitration::raise_fraud_dispute(
        &env,
        dispute_id.clone(),
        schedule_id,
        beneficiary,
        initiator,
        evidence_hash,
    ).unwrap();
    
    // Unauthorized juror should not be able to vote
    let result = fraud_arbitration::cast_juror_vote(
        &env,
        dispute_id,
        unauthorized_juror,
        fraud_arbitration::JurorVote::SlashForFraud,
    );
    
    assert!(matches!(result, Err(Error::NotAuthorizedJuror)));
}

#[test]
fn test_juror_double_voting() {
    let env = Env::default();
    let dao = Address::generate(&env);
    let security_council = Address::generate(&env);
    let admin = Address::generate(&env);
    let initiator = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let asset = Address::generate(&env);
    
    env.mock_auths(&[(&dao, &100), (&admin, &100), (&initiator, &100), (&beneficiary, &100)]);
    
    // Initialize arbitration system and add juror
    fraud_arbitration::initialize_arbitration(
        &env,
        dao.clone(),
        security_council.clone(),
        3,
        2,
        7,
    ).unwrap();
    
    let juror = Address::generate(&env);
    fraud_arbitration::add_juror(&env, security_council.clone(), juror.clone(), admin.clone()).unwrap();
    
    // Create vesting schedule and raise dispute
    let schedule_id = Symbol::new(&env, "double_vote_schedule");
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
    
    let dispute_id = Symbol::new(&env, "double_vote_dispute");
    let evidence_hash = BytesN::from_array(&env, &[1; 32]);
    
    fraud_arbitration::raise_fraud_dispute(
        &env,
        dispute_id.clone(),
        schedule_id,
        beneficiary,
        initiator,
        evidence_hash,
    ).unwrap();
    
    // First vote should succeed
    let result = fraud_arbitration::cast_juror_vote(
        &env,
        dispute_id.clone(),
        juror.clone(),
        fraud_arbitration::JurorVote::SlashForFraud,
    );
    assert!(result.is_ok());
    
    // Second vote from same juror should fail
    let result = fraud_arbitration::cast_juror_vote(
        &env,
        dispute_id,
        juror,
        fraud_arbitration::JurorVote::DismissCharges,
    );
    
    assert!(matches!(result, Err(Error::AlreadySigned)));
}

#[test]
fn test_auto_resolve_expired_disputes() {
    let env = Env::default();
    let dao = Address::generate(&env);
    let security_council = Address::generate(&env);
    let admin = Address::generate(&env);
    let initiator = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let asset = Address::generate(&env);
    
    env.mock_auths(&[(&dao, &100), (&admin, &100), (&initiator, &100), (&beneficiary, &100)]);
    
    // Initialize arbitration system with very short voting period
    fraud_arbitration::initialize_arbitration(
        &env,
        dao.clone(),
        security_council.clone(),
        3,
        2,
        1, // 1 day voting period
    ).unwrap();
    
    // Add jurors
    for i in 0..3 {
        let juror = Address::generate(&env);
        fraud_arbitration::add_juror(&env, security_council.clone(), juror, admin.clone()).unwrap();
    }
    
    // Create vesting schedule and raise dispute
    let schedule_id = Symbol::new(&env, "expired_dispute_schedule");
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
    
    let dispute_id = Symbol::new(&env, "expired_dispute");
    let evidence_hash = BytesN::from_array(&env, &[1; 32]);
    
    fraud_arbitration::raise_fraud_dispute(
        &env,
        dispute_id.clone(),
        schedule_id.clone(),
        beneficiary.clone(),
        initiator.clone(),
        evidence_hash,
    ).unwrap();
    
    // Advance time past voting deadline
    env.ledger().set_timestamp(current_time + 2 * 24 * 60 * 60); // 2 days later
    
    // Auto-resolve should dismiss the dispute
    let resolved_count = fraud_arbitration::auto_resolve_expired_disputes(&env).unwrap();
    assert_eq!(resolved_count, 1);
    
    // Verify dispute was auto-dismissed
    let dismissed_dispute = storage::get_fraud_dispute(&env, &dispute_id).unwrap();
    assert!(dismissed_dispute.is_resolved);
    assert!(matches!(dismissed_dispute.status, fraud_arbitration::FraudDisputeStatus::Dismissed));
    assert_eq!(dismissed_dispute.resolution_reason, "Auto-dismissed - voting deadline expired");
    
    // Verify vesting schedule was unfrozen
    let unfrozen_schedule = storage::get_vesting_schedule(&env, &schedule_id).unwrap();
    assert!(!unfrozen_schedule.is_frozen);
}
