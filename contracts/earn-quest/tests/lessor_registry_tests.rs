use earn_quest::{lessor_registry, storage, Error};
use soroban_sdk::{testutils::Ledger, Address, Env, Symbol, String};

#[test]
fn test_lessor_registry_initialization() {
    let env = Env::default();
    let governance = Address::generate(&env);
    
    // Initialize registry
    let result = lessor_registry::initialize_lessor_registry(&env, governance.clone());
    assert!(result.is_ok());
    
    let registry = storage::get_lessor_registry(&env);
    assert_eq!(registry.governance_address, governance);
    assert_eq!(registry.total_lessors, 0);
    assert_eq!(registry.active_lessors, 0);
    assert!(storage::is_lessor_registry_initialized(&env));
}

#[test]
fn test_lessor_registry_double_initialization() {
    let env = Env::default();
    let governance = Address::generate(&env);
    
    // Initialize registry twice should fail
    lessor_registry::initialize_lessor_registry(&env, governance.clone()).unwrap();
    let result = lessor_registry::initialize_lessor_registry(&env, governance);
    assert!(matches!(result, Err(Error::AlreadyInitialized)));
}

#[test]
fn test_register_authorized_lessor() {
    let env = Env::default();
    let governance = Address::generate(&env);
    let registrar = Address::generate(&env);
    let lessor = Address::generate(&env);
    
    env.mock_auths(&[(&governance, &100), (&registrar, &100)]);
    
    // Initialize registry
    lessor_registry::initialize_lessor_registry(&env, governance.clone()).unwrap();
    
    let name = String::from_str(&env, "Test Bank");
    let institution_type = lessor_registry::InstitutionType::Bank;
    let credit_rating = 200;
    let max_vesting_amount = 1000000;
    let compliance_level = lessor_registry::ComplianceLevel::Enhanced;
    
    // Register lessor
    let result = lessor_registry::register_authorized_lessor(
        &env,
        lessor.clone(),
        name.clone(),
        institution_type.clone(),
        credit_rating,
        max_vesting_amount,
        compliance_level.clone(),
        registrar.clone(),
    );
    
    assert!(result.is_ok());
    
    // Verify lessor was registered
    let registered_lessor = storage::get_authorized_lessor(&env, &lessor).unwrap();
    assert_eq!(registered_lessor.address, lessor);
    assert_eq!(registered_lessor.name, name);
    assert!(matches!(registered_lessor.institution_type, lessor_registry::InstitutionType::Bank));
    assert_eq!(registered_lessor.credit_rating, credit_rating);
    assert_eq!(registered_lessor.max_vesting_amount, max_vesting_amount);
    assert!(matches!(registered_lessor.compliance_level, lessor_registry::ComplianceLevel::Enhanced));
    assert!(registered_lessor.is_active);
    assert_eq!(registered_lessor.authorized_by, registrar);
    
    // Verify registry stats
    let registry = storage::get_lessor_registry(&env);
    assert_eq!(registry.total_lessors, 1);
    assert_eq!(registry.active_lessors, 1);
}

#[test]
fn test_register_lessor_unauthorized() {
    let env = Env::default();
    let governance = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let lessor = Address::generate(&env);
    
    env.mock_auths(&[(&governance, &100), (&unauthorized, &100)]);
    
    // Initialize registry
    lessor_registry::initialize_lessor_registry(&env, governance).unwrap();
    
    // Unauthorized user should not be able to register lessor
    let result = lessor_registry::register_authorized_lessor(
        &env,
        lessor,
        String::from_str(&env, "Unauthorized Bank"),
        lessor_registry::InstitutionType::Bank,
        200,
        1000000,
        lessor_registry::ComplianceLevel::Enhanced,
        unauthorized,
    );
    
    assert!(matches!(result, Err(Error::Unauthorized)));
}

#[test]
fn test_register_lessor_invalid_amount() {
    let env = Env::default();
    let governance = Address::generate(&env);
    let registrar = Address::generate(&env);
    let lessor = Address::generate(&env);
    
    env.mock_auths(&[(&governance, &100), (&registrar, &100)]);
    
    // Initialize registry
    lessor_registry::initialize_lessor_registry(&env, governance).unwrap();
    
    // Register lessor with zero amount should fail
    let result = lessor_registry::register_authorized_lessor(
        &env,
        lessor,
        String::from_str(&env, "Zero Bank"),
        lessor_registry::InstitutionType::Bank,
        200,
        0, // Invalid amount
        lessor_registry::ComplianceLevel::Enhanced,
        registrar,
    );
    
    assert!(matches!(result, Err(Error::InvalidRewardAmount)));
}

#[test]
fn test_register_lessor_invalid_name() {
    let env = Env::default();
    let governance = Address::generate(&env);
    let registrar = Address::generate(&env);
    let lessor = Address::generate(&env);
    
    env.mock_auths(&[(&governance, &100), (&registrar, &100)]);
    
    // Initialize registry
    lessor_registry::initialize_lessor_registry(&env, governance).unwrap();
    
    // Register lessor with empty name should fail
    let result = lessor_registry::register_authorized_lessor(
        &env,
        lessor,
        String::from_str(&env, ""), // Empty name
        lessor_registry::InstitutionType::Bank,
        200,
        1000000,
        lessor_registry::ComplianceLevel::Enhanced,
        registrar,
    );
    
    assert!(matches!(result, Err(Error::StringTooLong))); // Empty string triggers length validation
}

#[test]
fn test_update_lessor_info() {
    let env = Env::default();
    let governance = Address::generate(&env);
    let registrar = Address::generate(&env);
    let updater = Address::generate(&env);
    let lessor = Address::generate(&env);
    
    env.mock_auths(&[(&governance, &100), (&registrar, &100), (&updater, &100)]);
    
    // Initialize registry and register lessor
    lessor_registry::initialize_lessor_registry(&env, governance).unwrap();
    lessor_registry::register_authorized_lessor(
        &env,
        lessor.clone(),
        String::from_str(&env, "Original Name"),
        lessor_registry::InstitutionType::Bank,
        150,
        1000000,
        lessor_registry::ComplianceLevel::Basic,
        registrar,
    ).unwrap();
    
    // Update lessor info
    let new_name = Some(String::from_str(&env, "Updated Name"));
    let new_rating = Some(200);
    let new_amount = Some(2000000);
    let new_compliance = Some(lessor_registry::ComplianceLevel::Enhanced);
    
    let result = lessor_registry::update_lessor_info(
        &env,
        lessor.clone(),
        new_name.clone(),
        new_rating,
        new_amount,
        new_compliance.clone(),
        updater.clone(),
    );
    
    assert!(result.is_ok());
    
    // Verify updates
    let updated_lessor = storage::get_authorized_lessor(&env, &lessor).unwrap();
    assert_eq!(updated_lessor.name, new_name.unwrap());
    assert_eq!(updated_lessor.credit_rating, new_rating.unwrap());
    assert_eq!(updated_lessor.max_vesting_amount, new_amount.unwrap());
    assert!(matches!(updated_lessor.compliance_level, new_compliance.unwrap()));
}

#[test]
fn test_deactivate_lessor() {
    let env = Env::default();
    let governance = Address::generate(&env);
    let registrar = Address::generate(&env);
    let deactivator = Address::generate(&env);
    let lessor = Address::generate(&env);
    
    env.mock_auths(&[(&governance, &100), (&registrar, &100), (&deactivator, &100)]);
    
    // Initialize registry and register lessor
    lessor_registry::initialize_lessor_registry(&env, governance).unwrap();
    lessor_registry::register_authorized_lessor(
        &env,
        lessor.clone(),
        String::from_str(&env, "Active Bank"),
        lessor_registry::InstitutionType::Bank,
        200,
        1000000,
        lessor_registry::ComplianceLevel::Enhanced,
        registrar,
    ).unwrap();
    
    // Verify active count
    let registry = storage::get_lessor_registry(&env);
    assert_eq!(registry.active_lessors, 1);
    
    // Deactivate lessor
    let reason = String::from_str(&env, "Compliance violation");
    let result = lessor_registry::deactivate_lessor(&env, lessor.clone(), deactivator.clone(), reason.clone());
    assert!(result.is_ok());
    
    // Verify deactivation
    let deactivated_lessor = storage::get_authorized_lessor(&env, &lessor).unwrap();
    assert!(!deactivated_lessor.is_active);
    
    // Verify active count decreased
    let registry = storage::get_lessor_registry(&env);
    assert_eq!(registry.active_lessors, 0);
}

#[test]
fn test_reactivate_lessor() {
    let env = Env::default();
    let governance = Address::generate(&env);
    let registrar = Address::generate(&env);
    let deactivator = Address::generate(&env);
    let reactivator = Address::generate(&env);
    let lessor = Address::generate(&env);
    
    env.mock_auths(&[(&governance, &100), (&registrar, &100), (&deactivator, &100), (&reactivator, &100)]);
    
    // Initialize registry and register lessor
    lessor_registry::initialize_lessor_registry(&env, governance).unwrap();
    lessor_registry::register_authorized_lessor(
        &env,
        lessor.clone(),
        String::from_str(&env, "Reactivatable Bank"),
        lessor_registry::InstitutionType::Bank,
        200,
        1000000,
        lessor_registry::ComplianceLevel::Enhanced,
        registrar,
    ).unwrap();
    
    // Deactivate lessor first
    let reason = String::from_str(&env, "Temporary suspension");
    lessor_registry::deactivate_lessor(&env, lessor.clone(), deactivator, reason).unwrap();
    
    // Verify deactivated
    let deactivated_lessor = storage::get_authorized_lessor(&env, &lessor).unwrap();
    assert!(!deactivated_lessor.is_active);
    
    // Reactivate lessor
    let result = lessor_registry::reactivate_lessor(&env, lessor.clone(), reactivator.clone());
    assert!(result.is_ok());
    
    // Verify reactivation
    let reactivated_lessor = storage::get_authorized_lessor(&env, &lessor).unwrap();
    assert!(reactivated_lessor.is_active);
    
    // Verify active count increased
    let registry = storage::get_lessor_registry(&env);
    assert_eq!(registry.active_lessors, 1);
}

#[test]
fn test_validate_vesting_amount() {
    let env = Env::default();
    let governance = Address::generate(&env);
    let registrar = Address::generate(&env);
    let lessor = Address::generate(&env);
    
    env.mock_auths(&[(&governance, &100), (&registrar, &100)]);
    
    // Initialize registry and register lessor with specific limits
    lessor_registry::initialize_lessor_registry(&env, governance).unwrap();
    lessor_registry::register_authorized_lessor(
        &env,
        lessor.clone(),
        String::from_str(&env, "Limit Test Bank"),
        lessor_registry::InstitutionType::Bank,
        200, // High credit rating
        1000000, // Max 1M
        lessor_registry::ComplianceLevel::Enhanced,
        registrar,
    ).unwrap();
    
    // Test amount within limit should pass
    let result = lessor_registry::validate_vesting_amount(&env, &lessor, 500000);
    assert!(result.is_ok());
    
    // Test amount exceeding limit should fail
    let result = lessor_registry::validate_vesting_amount(&env, &lessor, 2000000);
    assert!(matches!(result, Err(Error::AmountTooLarge)));
    
    // Test with inactive lessor should fail
    let deactivator = Address::generate(&env);
    env.mock_auths(&[(&governance, &100), (&registrar, &100), (&deactivator, &100)]);
    lessor_registry::deactivate_lessor(&env, lessor.clone(), deactivator, String::from_str(&env, "Test")).unwrap();
    
    let result = lessor_registry::validate_vesting_amount(&env, &lessor, 500000);
    assert!(matches!(result, Err(Error::Unauthorized)));
}

#[test]
fn test_calculate_max_by_credit_rating() {
    // Test the credit rating multiplier calculation
    let base_amount = 1000;
    
    // Rating 0 should give 100% (base multiplier)
    let max_0 = lessor_registry::calculate_max_by_credit_rating(0, base_amount);
    assert_eq!(max_0, base_amount);
    
    // Rating 127 (50%) should give 150% (base + 50%)
    let max_127 = lessor_registry::calculate_max_by_credit_rating(127, base_amount);
    assert_eq!(max_127, 1500);
    
    // Rating 255 (100%) should give 200% (base + 100%)
    let max_255 = lessor_registry::calculate_max_by_credit_rating(255, base_amount);
    assert_eq!(max_255, 2000);
}
