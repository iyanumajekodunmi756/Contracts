use crate::errors::Error;
use crate::events;
use crate::storage;
use crate::types::Address;
use soroban_sdk::{Env, Symbol, String, Vec, BytesN};

/// Fraud dispute status
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FraudDisputeStatus {
    /// Dispute has been raised and is pending juror selection
    Pending,
    /// Jurors have been selected and are reviewing evidence
    UnderReview,
    /// Voting is in progress
    Voting,
    /// Dispute has been resolved
    Resolved,
    /// Dispute was dismissed
    Dismissed,
}

/// Juror vote
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JurorVote {
    /// Vote to slash for fraud
    SlashForFraud,
    /// Vote to dismiss charges
    DismissCharges,
    /// Not yet voted
    NotVoted,
}

/// Fraud dispute structure
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FraudDispute {
    pub id: Symbol,
    pub target_schedule: Symbol,
    pub target_beneficiary: Address,
    pub initiator: Address, // DAO or authorized entity
    pub evidence_hash: BytesN<32>,
    pub status: FraudDisputeStatus,
    pub filed_at: u64,
    pub voting_deadline: u64,
    pub jurors: Vec<Address>,
    pub votes: Vec<JurorVote>,
    pub slash_votes: u32,
    pub dismiss_votes: u32,
    pub resolution_reason: String,
    pub is_resolved: bool,
}

/// Juror pool for security council
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JurorPool {
    pub jurors: Vec<Address>,
    pub last_updated: u64,
    pub minimum_jurors: u32,
    pub voting_period_days: u64,
}

/// Arbitration configuration
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArbitrationConfig {
    pub required_jurors: u32,
    pub voting_threshold: u32, // votes needed for decision
    pub voting_period_seconds: u64,
    pub evidence_submission_deadline: u64,
    pub dao_address: Address,
    pub security_council_address: Address,
}

/// Raise a fraud dispute
pub fn raise_fraud_dispute(
    env: &Env,
    dispute_id: Symbol,
    target_schedule: Symbol,
    target_beneficiary: Address,
    initiator: Address,
    evidence_hash: BytesN<32>,
) -> Result<FraudDispute, Error> {
    // Verify initiator is authorized (DAO or admin)
    if !storage::is_fraud_dispute_initiator(env, &initiator) {
        return Err(Error::Unauthorized);
    }

    // Check if target schedule exists and is active
    let schedule = storage::get_vesting_schedule(env, &target_schedule)?;
    if schedule.beneficiary != target_beneficiary {
        return Err(Error::Unauthorized);
    }

    if !schedule.is_active || schedule.is_frozen {
        return Err(Error::InvalidQuestStatus);
    }

    // Check if dispute already exists for this schedule
    if storage::has_fraud_dispute(env, &target_schedule) {
        return Err(Error::DisputeAlreadyExists);
    }

    let current_time = env.ledger().timestamp();
    let config = storage::get_arbitration_config(env);
    
    // Freeze the target schedule immediately to prevent front-running
    crate::vesting::freeze_vesting_schedule(env, target_schedule.clone(), initiator.clone())?;

    // Select random jurors from security council
    let jurors = select_jurors(env, &config.security_council_address, config.required_jurors)?;
    
    let voting_deadline = current_time + config.voting_period_seconds;

    let dispute = FraudDispute {
        id: dispute_id.clone(),
        target_schedule: target_schedule.clone(),
        target_beneficiary: target_beneficiary.clone(),
        initiator: initiator.clone(),
        evidence_hash: evidence_hash.clone(),
        status: FraudDisputeStatus::UnderReview,
        filed_at: current_time,
        voting_deadline,
        jurors: jurors.clone(),
        votes: {
            let mut votes = Vec::new(&env);
            for _ in 0..config.required_jurors {
                votes.push_back(JurorVote::NotVoted);
            }
            votes
        },
        slash_votes: 0,
        dismiss_votes: 0,
        resolution_reason: String::from_str(&env, ""),
        is_resolved: false,
    };

    // Store dispute
    storage::set_fraud_dispute(env, &dispute_id, &dispute);
    storage::set_dispute_for_schedule(env, &target_schedule, &dispute_id);

    // Emit event
    events::fraud_dispute_raised(
        env,
        dispute_id,
        target_schedule,
        target_beneficiary,
        initiator,
        evidence_hash,
        jurors,
    );

    Ok(dispute)
}

/// Select random jurors from security council
fn select_jurors(
    env: &Env,
    security_council_address: &Address,
    required_jurors: u32,
) -> Result<Vec<Address>, Error> {
    let juror_pool = storage::get_juror_pool(env, security_council_address)?;
    
    if juror_pool.jurors.len() < required_jurors as usize {
        return Err(Error::InsufficientSignatures);
    }

    // Use pseudo-random selection based on current ledger
    let seed = env.ledger().timestamp();
    let mut selected_jurors = Vec::new(env);
    let mut used_indices = Vec::new(env);

    for _ in 0..required_jurors {
        let random_index = (seed + selected_jurors.len() as u64) % juror_pool.jurors.len() as u64;
        
        if !used_indices.contains(&random_index) {
            selected_jurors.push_back(juror_pool.jurors.get(random_index as usize).unwrap());
            used_indices.push_back(random_index);
        }
    }

    Ok(selected_jurors)
}

/// Cast vote as a juror
pub fn cast_juror_vote(
    env: &Env,
    dispute_id: Symbol,
    juror: Address,
    vote: JurorVote,
) -> Result<(), Error> {
    let mut dispute = storage::get_fraud_dispute(env, &dispute_id)?;

    // Verify juror is authorized
    if !dispute.jurors.contains(&juror) {
        return Err(Error::Unauthorized);
    }

    // Check if voting is still open
    let current_time = env.ledger().timestamp();
    if current_time > dispute.voting_deadline {
        return Err(Error::EmergencyWindowClosed);
    }

    // Find juror index
    let juror_index = dispute.jurors.iter().position(|&j| j == juror);
    if juror_index.is_none() {
        return Err(Error::Unauthorized);
    }

    let index = juror_index.unwrap();
    
    // Check if already voted
    if dispute.votes.get(index).unwrap() != &JurorVote::NotVoted {
        return Err(Error::AlreadySigned);
    }

    // Update vote
    dispute.votes.set(index, vote.clone());

    // Update vote counts
    match vote {
        JurorVote::SlashForFraud => {
            dispute.slash_votes += 1;
        }
        JurorVote::DismissCharges => {
            dispute.dismiss_votes += 1;
        }
        JurorVote::NotVoted => {} // Should not happen
    }

    // Store updated dispute
    storage::set_fraud_dispute(env, &dispute_id, &dispute);

    // Check if voting threshold is reached
    let config = storage::get_arbitration_config(env);
    let total_votes = dispute.slash_votes + dispute.dismiss_votes;
    
    if total_votes >= config.voting_threshold {
        resolve_dispute(env, &dispute_id)?;
    }

    // Emit event
    events::juror_vote_cast(env, dispute_id, juror, vote);

    Ok(())
}

/// Resolve dispute based on votes
fn resolve_dispute(env: &Env, dispute_id: &Symbol) -> Result<(), Error> {
    let mut dispute = storage::get_fraud_dispute(env, dispute_id)?;
    
    if dispute.is_resolved {
        return Err(Error::DisputeAlreadyResolved);
    }

    let config = storage::get_arbitration_config(env);
    let total_jurors = dispute.jurors.len() as u32;
    
    // Check if we have enough votes
    let total_votes = dispute.slash_votes + dispute.dismiss_votes;
    if total_votes < config.voting_threshold {
        return Err(Error::InsufficientSignatures);
    }

    // Determine outcome
    let slash_for_fraud = dispute.slash_votes >= config.voting_threshold;
    
    if slash_for_fraud {
        // Slash the beneficiary - terminate vesting and return tokens to treasury
        let unvested_amount = crate::vesting::terminate_vesting_schedule(
            env,
            dispute.target_schedule.clone(),
            dispute.initiator.clone(),
            "Fraud confirmed by arbitration panel",
        )?;

        dispute.resolution_reason = String::from_str(&env, "Fraud confirmed - tokens slashed");
        dispute.status = FraudDisputeStatus::Resolved;

        // Emit resolution event
        events::arbitration_resolved(
            env,
            dispute_id.clone(),
            dispute.target_schedule.clone(),
            true, // fraud_confirmed
            unvested_amount,
            dispute.resolution_reason.clone(),
        );
    } else {
        // Dismiss charges - unfreeze the schedule
        crate::vesting::unfreeze_vesting_schedule(
            env,
            dispute.target_schedule.clone(),
            dispute.initiator.clone(),
        )?;

        dispute.resolution_reason = String::from_str(&env, "Charges dismissed - no fraud found");
        dispute.status = FraudDisputeStatus::Dismissed;

        // Emit resolution event
        events::arbitration_resolved(
            env,
            dispute_id.clone(),
            dispute.target_schedule.clone(),
            false, // fraud_confirmed
            0,
            dispute.resolution_reason.clone(),
        );
    }

    dispute.is_resolved = true;
    storage::set_fraud_dispute(env, dispute_id, &dispute);

    Ok(())
}

/// Auto-resolve disputes after voting deadline
pub fn auto_resolve_expired_disputes(env: &Env) -> Result<u32, Error> {
    let current_time = env.ledger().timestamp();
    let pending_disputes = storage::get_pending_fraud_disputes(env)?;
    let mut resolved_count = 0;

    for dispute_id in pending_disputes.iter() {
        let dispute = storage::get_fraud_dispute(env, dispute_id)?;
        
        if !dispute.is_resolved && current_time > dispute.voting_deadline {
            // Auto-dismiss if voting deadline passed without resolution
            let mut dispute = dispute;
            dispute.resolution_reason = String::from_str(&env, "Auto-dismissed - voting deadline expired");
            dispute.status = FraudDisputeStatus::Dismissed;
            dispute.is_resolved = true;

            // Unfreeze the schedule
            crate::vesting::unfreeze_vesting_schedule(
                env,
                dispute.target_schedule.clone(),
                dispute.initiator.clone(),
            )?;

            storage::set_fraud_dispute(env, dispute_id, &dispute);
            
            // Emit auto-resolution event
            events::arbitration_resolved(
                env,
                dispute_id.clone(),
                dispute.target_schedule.clone(),
                false, // fraud_confirmed
                0,
                dispute.resolution_reason.clone(),
            );

            resolved_count += 1;
        }
    }

    Ok(resolved_count)
}

/// Get dispute details
pub fn get_fraud_dispute(env: &Env, dispute_id: &Symbol) -> Result<FraudDispute, Error> {
    storage::get_fraud_dispute(env, dispute_id)
}

/// Get dispute for a specific schedule
pub fn get_dispute_for_schedule(env: &Env, schedule_id: &Symbol) -> Result<FraudDispute, Error> {
    let dispute_id = storage::get_dispute_for_schedule(env, schedule_id)?;
    storage::get_fraud_dispute(env, &dispute_id)
}

/// Initialize arbitration system
pub fn initialize_arbitration(
    env: &Env,
    dao_address: Address,
    security_council_address: Address,
    required_jurors: u32,
    voting_threshold: u32,
    voting_period_days: u64,
) -> Result<(), Error> {
    if storage::is_arbitration_initialized(env) {
        return Err(Error::AlreadyInitialized);
    }

    let config = ArbitrationConfig {
        required_jurors,
        voting_threshold,
        voting_period_seconds: voting_period_days * 24 * 60 * 60, // Convert days to seconds
        evidence_submission_deadline: 7 * 24 * 60 * 60, // 7 days
        dao_address: dao_address.clone(),
        security_council_address: security_council_address.clone(),
    };

    storage::set_arbitration_config(env, &config);
    storage::mark_arbitration_initialized(env);

    // Initialize juror pool
    let juror_pool = JurorPool {
        jurors: Vec::new(env),
        last_updated: env.ledger().timestamp(),
        minimum_jurors: required_jurors,
        voting_period_days,
    };

    storage::set_juror_pool(env, &security_council_address, &juror_pool);

    events::arbitration_initialized(env, dao_address, security_council_address);

    Ok(())
}

/// Add juror to security council
pub fn add_juror(
    env: &Env,
    security_council_address: Address,
    juror_address: Address,
    admin: Address,
) -> Result<(), Error> {
    // Check if admin is authorized
    if !storage::is_arbitration_admin(env, &admin) {
        return Err(Error::Unauthorized);
    }

    let mut juror_pool = storage::get_juror_pool(env, &security_council_address)?;
    
    if juror_pool.jurors.contains(&juror_address) {
        return Err(Error::AlreadyExists);
    }

    juror_pool.jurors.push_back(juror_address.clone());
    juror_pool.last_updated = env.ledger().timestamp();

    storage::set_juror_pool(env, &security_council_address, &juror_pool);

    events::juror_added(env, security_council_address, juror_address, admin);

    Ok(())
}

/// Remove juror from security council
pub fn remove_juror(
    env: &Env,
    security_council_address: Address,
    juror_address: Address,
    admin: Address,
) -> Result<(), Error> {
    // Check if admin is authorized
    if !storage::is_arbitration_admin(env, &admin) {
        return Err(Error::Unauthorized);
    }

    let mut juror_pool = storage::get_juror_pool(env, &security_council_address)?;
    
    let index = juror_pool.jurors.iter().position(|&j| j == juror_address);
    if index.is_none() {
        return Err(Error::NotFound);
    }

    juror_pool.jurors.remove(index.unwrap());
    juror_pool.last_updated = env.ledger().timestamp();

    storage::set_juror_pool(env, &security_council_address, &juror_pool);

    events::juror_removed(env, security_council_address, juror_address, admin);

    Ok(())
}

/// Get current juror pool
pub fn get_juror_pool(env: &Env, security_council_address: &Address) -> Result<JurorPool, Error> {
    storage::get_juror_pool(env, security_council_address)
}
