use crate::errors::Error;
use crate::events;
use crate::storage;
use crate::types::{Address, Symbol, U256, Vec};
use soroban_sdk::{token, Env};

/// Vesting schedule types
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VestingType {
    Linear,
    Cliff,
    Custom,
}

/// Vesting schedule structure
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VestingSchedule {
    pub id: Symbol,
    pub beneficiary: Address,
    pub asset: Address,
    pub total_amount: i128,
    pub vested_amount: i128,
    pub claimed_amount: i128,
    pub start_time: u64,
    pub end_time: u64,
    pub cliff_time: u64,
    pub vesting_type: VestingType,
    pub is_active: bool,
    pub is_frozen: bool,
}

/// Virtual accumulator for high-frequency linear vesting
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualAccumulator {
    pub schedule_id: Symbol,
    pub last_update_time: u64,
    pub accumulated_rate: u128, // Rate in smallest units per second
    pub accumulated_vested: i128,
}

/// Anti-reentry guard for external asset transfers
pub struct AntiReentryGuard {
    pub is_locked: bool,
    pub lock_timestamp: u64,
    pub caller: Address,
}

impl AntiReentryGuard {
    pub fn new() -> Self {
        Self {
            is_locked: false,
            lock_timestamp: 0,
            caller: Address::default(),
        }
    }

    pub fn enter(&mut self, caller: Address, current_time: u64) -> Result<(), Error> {
        if self.is_locked {
            return Err(Error::ReentrantCall);
        }
        self.is_locked = true;
        self.lock_timestamp = current_time;
        self.caller = caller;
        Ok(())
    }

    pub fn exit(&mut self) {
        self.is_locked = false;
        self.lock_timestamp = 0;
        self.caller = Address::default();
    }

    pub fn is_valid_caller(&self, caller: &Address) -> bool {
        !self.is_locked || self.caller == *caller
    }
}

/// Calculate vested amount for linear vesting using virtual accumulator
pub fn calculate_linear_vested(
    schedule: &VestingSchedule,
    accumulator: &VirtualAccumulator,
    current_time: u64,
) -> i128 {
    if current_time <= schedule.start_time || schedule.is_frozen {
        return schedule.vested_amount;
    }

    let effective_time = if current_time >= schedule.end_time {
        schedule.end_time
    } else {
        current_time
    };

    let time_elapsed = effective_time.saturating_sub(accumulator.last_update_time);
    let additional_vested = (accumulator.accumulated_rate as u128 * time_elapsed as u128) as i128;

    schedule.vested_amount + additional_vested
}

/// Update virtual accumulator for linear vesting
pub fn update_virtual_accumulator(
    schedule: &VestingSchedule,
    accumulator: &mut VirtualAccumulator,
    current_time: u64,
) -> Result<(), Error> {
    if current_time <= accumulator.last_update_time {
        return Ok(());
    }

    let effective_time = if current_time >= schedule.end_time {
        schedule.end_time
    } else {
        current_time
    };

    let time_elapsed = effective_time.saturating_sub(accumulator.last_update_time);
    let additional_vested = (accumulator.accumulated_rate as u128 * time_elapsed as u128) as i128;

    accumulator.accumulated_vested += additional_vested;
    accumulator.last_update_time = current_time;

    Ok(())
}

/// Create a new vesting schedule
pub fn create_vesting_schedule(
    env: &Env,
    id: Symbol,
    beneficiary: Address,
    asset: Address,
    total_amount: i128,
    start_time: u64,
    end_time: u64,
    cliff_time: u64,
    vesting_type: VestingType,
) -> Result<VestingSchedule, Error> {
    if total_amount <= 0 {
        return Err(Error::InvalidRewardAmount);
    }

    if start_time >= end_time {
        return Err(Error::InvalidDeadline);
    }

    if cliff_time > end_time {
        return Err(Error::InvalidDeadline);
    }

    let schedule = VestingSchedule {
        id: id.clone(),
        beneficiary: beneficiary.clone(),
        asset: asset.clone(),
        total_amount,
        vested_amount: 0,
        claimed_amount: 0,
        start_time,
        end_time,
        cliff_time,
        vesting_type,
        is_active: true,
        is_frozen: false,
    };

    // Create virtual accumulator for linear vesting
    if matches!(vesting_type, VestingType::Linear) {
        let vesting_duration = end_time.saturating_sub(start_time);
        let rate = if vesting_duration > 0 {
            (total_amount as u128 * 1_000_000) / vesting_duration as u128 // Rate with 6 decimal precision
        } else {
            0
        };

        let accumulator = VirtualAccumulator {
            schedule_id: id,
            last_update_time: start_time,
            accumulated_rate: rate,
            accumulated_vested: 0,
        };

        storage::set_virtual_accumulator(env, &id, &accumulator);
    }

    Ok(schedule)
}

/// Claim vested tokens with anti-reentry protection
pub fn claim_vested_tokens(
    env: &Env,
    schedule_id: Symbol,
    claimer: Address,
) -> Result<i128, Error> {
    // Check anti-reentry guard
    let mut guard = storage::get_anti_reentry_guard(env);
    guard.enter(claimer.clone(), env.ledger().timestamp())?;
    
    // Store guard back to storage
    storage::set_anti_reentry_guard(env, &guard);

    let mut schedule = storage::get_vesting_schedule(env, &schedule_id)?;
    
    if schedule.beneficiary != claimer {
        storage::clear_anti_reentry_guard(env);
        return Err(Error::Unauthorized);
    }

    if !schedule.is_active || schedule.is_frozen {
        storage::clear_anti_reentry_guard(env);
        return Err(Error::InvalidQuestStatus);
    }

    let current_time = env.ledger().timestamp();
    let available_to_claim = calculate_available_to_claim(env, &schedule, current_time)?;

    if available_to_claim <= 0 {
        storage::clear_anti_reentry_guard(env);
        return Err(Error::InsufficientBalance);
    }

    // Update schedule before external transfer (CEI pattern)
    schedule.claimed_amount += available_to_claim;
    storage::set_vesting_schedule(env, &schedule_id, &schedule);

    // Perform external transfer
    let token_client = token::Client::new(env, &schedule.asset);
    let contract_address = env.current_contract_address();
    
    token_client.transfer(&contract_address, &claimer, &available_to_claim)?;

    // Clear anti-reentry guard after successful transfer
    storage::clear_anti_reentry_guard(env);

    // Emit event
    events::vesting_tokens_claimed(env, schedule_id, claimer, schedule.asset.clone(), available_to_claim);

    Ok(available_to_claim)
}

/// Calculate available tokens to claim
fn calculate_available_to_claim(
    env: &Env,
    schedule: &VestingSchedule,
    current_time: u64,
) -> Result<i128, Error> {
    let total_vested = calculate_total_vested(env, schedule, current_time)?;
    let available = total_vested.saturating_sub(schedule.claimed_amount);
    Ok(available)
}

/// Calculate total vested amount
fn calculate_total_vested(
    env: &Env,
    schedule: &VestingSchedule,
    current_time: u64,
) -> Result<i128, Error> {
    match schedule.vesting_type {
        VestingType::Linear => {
            let mut accumulator = storage::get_virtual_accumulator(env, &schedule.id)?;
            update_virtual_accumulator(schedule, &mut accumulator, current_time)?;
            storage::set_virtual_accumulator(env, &schedule.id, &accumulator);
            Ok(accumulator.accumulated_vested.min(schedule.total_amount))
        }
        VestingType::Cliff => {
            if current_time < schedule.cliff_time {
                Ok(0)
            } else if current_time >= schedule.end_time {
                Ok(schedule.total_amount)
            } else {
                let cliff_elapsed = current_time.saturating_sub(schedule.cliff_time);
                let cliff_duration = schedule.end_time.saturating_sub(schedule.cliff_time);
                if cliff_duration > 0 {
                    let vested = (schedule.total_amount as u128 * cliff_elapsed as u128) / cliff_duration as u128;
                    Ok(vested as i128)
                } else {
                    Ok(0)
                }
            }
        }
        VestingType::Custom => {
            // For custom vesting, we'd need additional data structures
            // For now, return linear calculation as fallback
            if current_time <= schedule.start_time {
                Ok(0)
            } else if current_time >= schedule.end_time {
                Ok(schedule.total_amount)
            } else {
                let elapsed = current_time.saturating_sub(schedule.start_time);
                let duration = schedule.end_time.saturating_sub(schedule.start_time);
                if duration > 0 {
                    let vested = (schedule.total_amount as u128 * elapsed as u128) / duration as u128;
                    Ok(vested as i128)
                } else {
                    Ok(0)
                }
            }
        }
    }
}

/// Freeze a vesting schedule (for fraud disputes)
pub fn freeze_vesting_schedule(env: &Env, schedule_id: Symbol, freezer: Address) -> Result<(), Error> {
    // Check if freezer is authorized (DAO or admin)
    if !storage::is_authorized_freezer(env, &freezer) {
        return Err(Error::Unauthorized);
    }

    let mut schedule = storage::get_vesting_schedule(env, &schedule_id)?;
    if schedule.is_frozen {
        return Err(Error::InvalidQuestStatus);
    }

    schedule.is_frozen = true;
    storage::set_vesting_schedule(env, &schedule_id, &schedule);

    events::vesting_schedule_frozen(env, schedule_id, freezer);
    Ok(())
}

/// Unfreeze a vesting schedule
pub fn unfreeze_vesting_schedule(env: &Env, schedule_id: Symbol, unfreezer: Address) -> Result<(), Error> {
    // Check if unfreezer is authorized
    if !storage::is_authorized_freezer(env, &unfreezer) {
        return Err(Error::Unauthorized);
    }

    let mut schedule = storage::get_vesting_schedule(env, &schedule_id)?;
    if !schedule.is_frozen {
        return Err(Error::InvalidQuestStatus);
    }

    schedule.is_frozen = false;
    storage::set_vesting_schedule(env, &schedule_id, &schedule);

    events::vesting_schedule_unfrozen(env, schedule_id, unfreezer);
    Ok(())
}

/// Terminate vesting schedule and return remaining tokens to treasury
pub fn terminate_vesting_schedule(
    env: &Env,
    schedule_id: Symbol,
    terminator: Address,
    reason: &str,
) -> Result<i128, Error> {
    // Check if terminator is authorized
    if !storage::is_authorized_terminator(env, &terminator) {
        return Err(Error::Unauthorized);
    }

    let mut schedule = storage::get_vesting_schedule(env, &schedule_id)?;
    if !schedule.is_active {
        return Err(Error::InvalidQuestStatus);
    }

    let current_time = env.ledger().timestamp();
    let total_vested = calculate_total_vested(env, &schedule, current_time)?;
    let unvested_amount = schedule.total_amount.saturating_sub(total_vested);
    let remaining_unclaimed = total_vested.saturating_sub(schedule.claimed_amount);

    // Mark schedule as inactive
    schedule.is_active = false;
    schedule.is_frozen = true;
    storage::set_vesting_schedule(env, &schedule_id, &schedule);

    // Return unvested amount to treasury
    if unvested_amount > 0 {
        let treasury = storage::get_treasury_address(env);
        let token_client = token::Client::new(env, &schedule.asset);
        let contract_address = env.current_contract_address();
        
        token_client.transfer(&contract_address, &treasury, &unvested_amount)?;
    }

    events::vesting_schedule_terminated(env, schedule_id, terminator, reason.to_string(), unvested_amount);

    Ok(unvested_amount)
}
