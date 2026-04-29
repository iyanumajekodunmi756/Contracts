use crate::errors::Error;
use crate::events;
use crate::storage;
use crate::types::Address;
use soroban_sdk::{Env, Symbol, String, Vec};

/// Authorized lessor information
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedLessor {
    pub address: Address,
    pub name: String,
    pub institution_type: InstitutionType,
    pub registration_date: u64,
    pub is_active: bool,
    pub credit_rating: u8, // 0-255 rating
    pub max_vesting_amount: i128,
    pub compliance_level: ComplianceLevel,
    pub authorized_by: Address,
}

/// Types of institutions
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstitutionType {
    Bank,
    VentureCapital,
    HedgeFund,
    Corporate,
    Foundation,
    Government,
    Other,
}

/// Compliance levels
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComplianceLevel {
    Basic,      // Standard KYC/AML
    Enhanced,   // Enhanced due diligence
    Full,       // Full regulatory compliance
    Sovereign,  // Sovereign immunity
}

/// Lessor registry state
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LessorRegistry {
    pub total_lessors: u32,
    pub active_lessors: u32,
    pub registry_version: u32,
    pub last_updated: u64,
    pub governance_address: Address,
}

/// Register a new authorized lessor
pub fn register_authorized_lessor(
    env: &Env,
    lessor_address: Address,
    name: String,
    institution_type: InstitutionType,
    credit_rating: u8,
    max_vesting_amount: i128,
    compliance_level: ComplianceLevel,
    registrar: Address,
) -> Result<(), Error> {
    // Check if registrar is authorized (DAO or admin)
    if !storage::is_authorized_registrar(env, &registrar) {
        return Err(Error::Unauthorized);
    }

    // Check if lessor already exists
    if storage::is_authorized_lessor(env, &lessor_address) {
        return Err(Error::AlreadyExists);
    }

    // Validate inputs
    if name.len() == 0 || name.len() > 100 {
        return Err(Error::StringTooLong);
    }

    if max_vesting_amount <= 0 {
        return Err(Error::InvalidRewardAmount);
    }

    let current_time = env.ledger().timestamp();
    let lessor = AuthorizedLessor {
        address: lessor_address.clone(),
        name: name.clone(),
        institution_type,
        registration_date: current_time,
        is_active: true,
        credit_rating,
        max_vesting_amount,
        compliance_level,
        authorized_by: registrar.clone(),
    };

    // Store lessor information
    storage::set_authorized_lessor(env, &lessor_address, &lessor);

    // Update registry state
    let mut registry = storage::get_lessor_registry(env);
    registry.total_lessors += 1;
    registry.active_lessors += 1;
    registry.last_updated = current_time;
    storage::set_lessor_registry(env, &registry);

    // Emit event
    events::authorized_lessor_registered(env, lessor_address, name, registrar);

    Ok(())
}

/// Update lessor information
pub fn update_lessor_info(
    env: &Env,
    lessor_address: Address,
    name: Option<String>,
    credit_rating: Option<u8>,
    max_vesting_amount: Option<i128>,
    compliance_level: Option<ComplianceLevel>,
    updater: Address,
) -> Result<(), Error> {
    // Check if updater is authorized
    if !storage::is_authorized_registrar(env, &updater) {
        return Err(Error::Unauthorized);
    }

    let mut lessor = storage::get_authorized_lessor(env, &lessor_address)?;

    // Update fields if provided
    if let Some(new_name) = name {
        if new_name.len() == 0 || new_name.len() > 100 {
            return Err(Error::StringTooLong);
        }
        lessor.name = new_name;
    }

    if let Some(new_rating) = credit_rating {
        lessor.credit_rating = new_rating;
    }

    if let Some(new_max_amount) = max_vesting_amount {
        if new_max_amount <= 0 {
            return Err(Error::InvalidRewardAmount);
        }
        lessor.max_vesting_amount = new_max_amount;
    }

    if let Some(new_compliance) = compliance_level {
        lessor.compliance_level = new_compliance;
    }

    // Update storage
    storage::set_authorized_lessor(env, &lessor_address, &lessor);

    // Update registry timestamp
    let mut registry = storage::get_lessor_registry(env);
    registry.last_updated = env.ledger().timestamp();
    storage::set_lessor_registry(env, &registry);

    // Emit event
    events::lessor_info_updated(env, lessor_address, updater);

    Ok(())
}

/// Deactivate an authorized lessor
pub fn deactivate_lessor(
    env: &Env,
    lessor_address: Address,
    deactivator: Address,
    reason: String,
) -> Result<(), Error> {
    // Check if deactivator is authorized
    if !storage::is_authorized_registrar(env, &deactivator) {
        return Err(Error::Unauthorized);
    }

    let mut lessor = storage::get_authorized_lessor(env, &lessor_address)?;

    if !lessor.is_active {
        return Err(Error::InvalidQuestStatus);
    }

    lessor.is_active = false;
    storage::set_authorized_lessor(env, &lessor_address, &lessor);

    // Update registry state
    let mut registry = storage::get_lessor_registry(env);
    registry.active_lessors = registry.active_lessors.saturating_sub(1);
    registry.last_updated = env.ledger().timestamp();
    storage::set_lessor_registry(env, &registry);

    // Emit event
    events::lessor_deactivated(env, lessor_address, deactivator, reason);

    Ok(())
}

/// Reactivate a deactivated lessor
pub fn reactivate_lessor(
    env: &Env,
    lessor_address: Address,
    reactivator: Address,
) -> Result<(), Error> {
    // Check if reactivator is authorized
    if !storage::is_authorized_registrar(env, &reactivator) {
        return Err(Error::Unauthorized);
    }

    let mut lessor = storage::get_authorized_lessor(env, &lessor_address)?;

    if lessor.is_active {
        return Err(Error::InvalidQuestStatus);
    }

    lessor.is_active = true;
    storage::set_authorized_lessor(env, &lessor_address, &lessor);

    // Update registry state
    let mut registry = storage::get_lessor_registry(env);
    registry.active_lessors += 1;
    registry.last_updated = env.ledger().timestamp();
    storage::set_lessor_registry(env, &registry);

    // Emit event
    events::lessor_reactivated(env, lessor_address, reactivator);

    Ok(())
}

/// Check if an address is an authorized lessor
pub fn is_authorized_lessor(env: &Env, address: &Address) -> bool {
    storage::is_authorized_lessor(env, address)
}

/// Get lessor information
pub fn get_lessor_info(env: &Env, address: &Address) -> Result<AuthorizedLessor, Error> {
    storage::get_authorized_lessor(env, address)
}

/// Get all active lessors
pub fn get_active_lessors(env: &Env, offset: u32, limit: u32) -> Result<Vec<Address>, Error> {
    storage::get_active_lessors(env, offset, limit)
}

/// Get lessors by institution type
pub fn get_lessors_by_type(
    env: &Env,
    institution_type: InstitutionType,
    offset: u32,
    limit: u32,
) -> Result<Vec<Address>, Error> {
    storage::get_lessors_by_type(env, institution_type, offset, limit)
}

/// Get lessors by compliance level
pub fn get_lessors_by_compliance_level(
    env: &Env,
    compliance_level: ComplianceLevel,
    offset: u32,
    limit: u32,
) -> Result<Vec<Address>, Error> {
    storage::get_lessors_by_compliance_level(env, compliance_level, offset, limit)
}

/// Validate vesting amount for lessor
pub fn validate_vesting_amount(
    env: &Env,
    lessor_address: &Address,
    amount: i128,
) -> Result<(), Error> {
    let lessor = storage::get_authorized_lessor(env, lessor_address)?;

    if !lessor.is_active {
        return Err(Error::Unauthorized);
    }

    if amount > lessor.max_vesting_amount {
        return Err(Error::AmountTooLarge);
    }

    // Additional validation based on credit rating
    let max_by_rating = calculate_max_by_credit_rating(lessor.credit_rating, amount);
    if amount > max_by_rating {
        return Err(Error::AmountTooLarge);
    }

    Ok(())
}

/// Calculate maximum vesting amount based on credit rating
fn calculate_max_by_credit_rating(credit_rating: u8, requested_amount: i128) -> i128 {
    // Credit rating 0-255 maps to percentage multiplier
    // Higher rating = higher multiplier
    let base_multiplier = 100; // 100% base
    let rating_bonus = (credit_rating as u128 * base_multiplier as u128) / 255;
    let total_multiplier = base_multiplier + rating_bonus as u128;
    
    // Apply multiplier to requested amount (capped at 300%)
    let capped_multiplier = total_multiplier.min(300);
    (requested_amount as u128 * capped_multiplier / 100) as i128
}

/// Initialize the lessor registry
pub fn initialize_lessor_registry(env: &Env, governance_address: Address) -> Result<(), Error> {
    if storage::is_lessor_registry_initialized(env) {
        return Err(Error::AlreadyInitialized);
    }

    let registry = LessorRegistry {
        total_lessors: 0,
        active_lessors: 0,
        registry_version: 1,
        last_updated: env.ledger().timestamp(),
        governance_address: governance_address.clone(),
    };

    storage::set_lessor_registry(env, &registry);
    storage::mark_lessor_registry_initialized(env);

    events::lessor_registry_initialized(env, governance_address);

    Ok(())
}

/// Update registry governance address
pub fn update_registry_governance(
    env: &Env,
    new_governance: Address,
    updater: Address,
) -> Result<(), Error> {
    // Check if updater is current governance
    let registry = storage::get_lessor_registry(env);
    if registry.governance_address != updater {
        return Err(Error::Unauthorized);
    }

    let mut registry = registry;
    registry.governance_address = new_governance.clone();
    registry.last_updated = env.ledger().timestamp();
    storage::set_lessor_registry(env, &registry);

    events::registry_governance_updated(env, new_governance, updater);

    Ok(())
}

/// Get registry statistics
pub fn get_registry_stats(env: &Env) -> LessorRegistry {
    storage::get_lessor_registry(env)
}
