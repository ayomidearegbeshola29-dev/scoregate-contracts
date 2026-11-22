#![no_std]

//! Minimal mock lending protocol used to exercise ScoreGate's
//! confidence-aware composability primitive (`docs/interface-spec.md` §1.2)
//! from a genuinely separate, independently deployed contract.
//!
//! This is **not** a real lending market — there is no collateral, no
//! interest, no liquidation. It exists solely to prove that `borrow` can
//! call `query_risk_gate_with_confidence` with its own `min_confidence`
//! floor and refuse the borrow when the wallet is too risky or the score
//! isn't backed by enough confidence.
//!
//! Production integrations should add a freshness bound and fail closed when
//! the score is stale, the oracle is silent, or the protocol's own pause state
//! is active.

mod events;
#[cfg(test)]
mod test;

use scoregate_score::ScoreGateScoreContractClient;
use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol};

pub const DEFAULT_MAX_BORROW_AMOUNT: i128 = 10_000_000_000_000_000_000;

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FailPolicy {
    FailClosed = 0,
    FailOpen = 1,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum MockLendingError {
    /// `initialize` has not been called yet.
    NotConfigured = 1,
    /// ScoreGate's `query_risk_gate_with_confidence` returned `false` for
    /// this wallet — either too risky, no score, or insufficient confidence.
    RiskGateRejected = 2,
    /// Borrow amount must be positive and within the maximum borrow ceiling.
    InvalidAmount = 3,
    OracleUnavailable = 4,
    StaleScore = 5,
    UnsupportedVersion = 6,
    Unauthorized = 7,
    /// A gate threshold or confidence floor exceeds the valid 0-100 scale.
    InvalidThreshold = 8,
    ContractPaused = 9,
    /// `initialize` was called after the fixture was already configured.
    AlreadyInitialized = 10,
}

#[contracttype]
enum DataKey {
    Admin,
    /// Contract ID of the ScoreGate score registry this market trusts.
    ScoreGate,
    /// Risk-gate threshold (0-100) this market enforces on borrows.
    GateThreshold,
    /// Minimum confidence (0-100) this market requires of the score backing
    /// a borrow decision.
    MinConfidence,
    FailPolicy,
    MaxStalenessSecs,
    RequiredOracleVersion,
    ExpandedRiskScore,
    MaxBorrowAmount,
    Paused,
}

#[contract]
pub struct MockLending;

#[contractimpl]
impl MockLending {
    /// One-time wiring: record the ScoreGate deployment plus the risk
    /// threshold and confidence floor this market enforces.
    pub fn initialize(
        env: Env,
        admin: Address,
        scoregate: Address,
        gate_threshold: u32,
        min_confidence: u32,
    ) -> Result<(), MockLendingError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(MockLendingError::AlreadyInitialized);
        }
        if gate_threshold > 100 || min_confidence > 100 {
            return Err(MockLendingError::InvalidThreshold);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::ScoreGate, &scoregate);
        env.storage().instance().set(&DataKey::GateThreshold, &gate_threshold);
        env.storage().instance().set(&DataKey::MinConfidence, &min_confidence);
        env.storage().instance().set(&DataKey::FailPolicy, &FailPolicy::FailClosed);
        env.storage().instance().set(&DataKey::MaxStalenessSecs, &604_800u64);
        env.storage().instance().set(&DataKey::RequiredOracleVersion, &0u32);
        let client = ScoreGateScoreContractClient::new(&env, &scoregate);
        let expanded_score = matches!(client.try_get_version(), Ok(Ok(version)) if version >= 5);
        env.storage().instance().set(&DataKey::ExpandedRiskScore, &expanded_score);
        env.storage().instance().set(&DataKey::MaxBorrowAmount, &DEFAULT_MAX_BORROW_AMOUNT);
        Ok(())
    }

    /// Transfer administrative authority to `new_admin`. Requires authorization
    /// of the current admin and emits an `adm_upd` event. (Issue #202)
    pub fn set_admin(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), MockLendingError> {
        Self::require_admin(&env, &current_admin)?;
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        events::admin_updated(&env, &new_admin);
        Ok(())
    }

    /// Read the currently configured fixture admin.
    pub fn get_admin(env: Env) -> Result<Address, MockLendingError> {
        env.storage().instance().get(&DataKey::Admin).ok_or(MockLendingError::NotConfigured)
    }

    /// Configure the maximum borrow amount ceiling. (Issue #204)
    pub fn set_max_borrow_amount(
        env: Env,
        admin: Address,
        max_borrow_amount: i128,
    ) -> Result<(), MockLendingError> {
        user.require_auth();
        Self::require_admin(&env, &admin)?;
        if Self::is_paused(env.clone()) { return Err(MockLendingError::ContractPaused); }
        if max_borrow_amount <= 0 {
            return Err(MockLendingError::InvalidAmount);
        }
        env.storage().instance().set(&DataKey::MaxBorrowAmount, &max_borrow_amount);
        events::max_borrow_updated(&env, max_borrow_amount);
        Ok(())
    }

    /// Read the configured maximum borrow amount ceiling.
    pub fn get_max_borrow_amount(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::MaxBorrowAmount).unwrap_or(DEFAULT_MAX_BORROW_AMOUNT)
    }

    /// Register or rotate the ScoreGate oracle this market consults for gate
    /// checks. Mirrors `MockAmm::set_risk_oracle`. (Issue #117)
    pub fn set_risk_oracle(
        env: Env,
        admin: Address,
        oracle: Address,
    ) -> Result<(), MockLendingError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::ScoreGate, &oracle);
        let client = ScoreGateScoreContractClient::new(&env, &oracle);
        let expanded_score = matches!(client.try_get_version(), Ok(Ok(version)) if version >= 5);
        env.storage().instance().set(&DataKey::ExpandedRiskScore, &expanded_score);
        events::oracle_updated(&env, &oracle);
        Ok(())
    }

    /// Re-probe the *currently configured* oracle's capabilities without
    /// changing its address. Needed when the oracle contract is upgraded in
    /// place (redeployed at the same address) — `set_risk_oracle` re-probes
    /// on an address change, but nothing previously re-probed a same-address
    /// upgrade. (Issue #120)
    pub fn refresh_oracle_capabilities(env: Env, admin: Address) -> Result<bool, MockLendingError> {
        Self::require_admin(&env, &admin)?;
        let scoregate: Address = env
            .storage()
            .instance()
            .get(&DataKey::ScoreGate)
            .ok_or(MockLendingError::NotConfigured)?;
        let client = ScoreGateScoreContractClient::new(&env, &scoregate);
        let expanded_score = matches!(client.try_get_version(), Ok(Ok(version)) if version >= 5);
        env.storage().instance().set(&DataKey::ExpandedRiskScore, &expanded_score);
        events::capabilities_refreshed(&env, expanded_score);
        Ok(expanded_score)
    }

    pub fn set_borrow_gate_config(
        env: Env,
        admin: Address,
        gate_threshold: u32,
        min_confidence: u32,
        fail_policy: FailPolicy,
        max_staleness_secs: u64,
        required_oracle_version: u32,
    ) -> Result<(), MockLendingError> {
        Self::require_admin(&env, &admin)?;
        if gate_threshold > 100 || min_confidence > 100 {
            return Err(MockLendingError::InvalidThreshold);
        }
        env.storage().instance().set(&DataKey::GateThreshold, &gate_threshold);
        env.storage().instance().set(&DataKey::MinConfidence, &min_confidence);
        env.storage().instance().set(&DataKey::FailPolicy, &fail_policy);
        env.storage().instance().set(&DataKey::MaxStalenessSecs, &max_staleness_secs);
        env.storage().instance().set(&DataKey::RequiredOracleVersion, &required_oracle_version);
        events::gate_config_updated(
            &env,
            gate_threshold,
            min_confidence,
            fa

/* … truncated 2513 chars — edit only what you need near the top … */
