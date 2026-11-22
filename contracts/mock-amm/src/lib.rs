#![no_std]

//! Minimal mock AMM used to exercise ScoreGate's composability primitives
//! (`docs/interface-spec.md` §1.1–§1.2) from a genuinely separate, independently
//! deployed contract.
//!
//! This is **not** a real AMM — there are no reserves, no pricing curve, no
//! transfers. It exists solely to prove that `swap` / `provide_liquidity_gated`
//! can call ScoreGate gate functions and refuse risky wallets, mirroring the
//! patterns in `examples/amm_gate.rs` and `examples/amm_gate_example.rs`.
//!
//! ## Fail-closed gate result vs. invocation failure
//!
//! A gate function returning `bool` only describes the *decision* for a
//! reachable, compatible oracle: `false` means "refuse this wallet" (fail
//! closed). It does **not** guarantee the cross-contract call itself cannot
//! trap. If the configured oracle address is absent, is not a ScoreGate
//! deployment, or exposes an incompatible interface, the invocation fails
//! (trap / decode error) before any `bool` is produced. Consumers must treat
//! that as a distinct failure mode and handle it explicitly — e.g. via the
//! generated `try_*` client — rather than assuming the call always succeeds.
//!
//! Note that the oracle address is caller-supplied (`initialize` /
//! `set_risk_oracle`), so the gate target may be missing or incompatible at
//! runtime; the mock surfaces this as `MockAmmError::OracleUnavailable`.
//!
//! The mock intentionally focuses on confidence-gated access control. Real
//! integrations should layer their own max-age and pause-state checks on top
//! of the ScoreGate gate so a stale-but-safe score cannot bypass a high-value
//! action during detection lag.

mod events;
#[cfg(test)]
mod test;

use scoregate_score::ScoreGateScoreContractClient;
use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol};

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FailPolicy {
    FailClosed = 0,
    FailOpen = 1,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum MockAmmError {
    /// `initialize` / `set_risk_oracle` has not been called yet.
    NotConfigured = 1,
    /// ScoreGate's gate returned `false` because the provider's risk score is
    /// at or above the configured threshold, or no score exists (fail closed).
    HighRiskWallet = 2,
    /// Liquidity amount must be positive.
    InvalidAmount = 3,
    /// ScoreGate's gate returned `false` because the score's confidence is
    /// below the configured minimum.
    LowConfidence = 4,
    /// The configured oracle call trapped or could not be decoded.
    OracleUnavailable = 5,
    /// The stored score is older than the configured fixture freshness window.
    StaleScore = 6,
    /// The configured oracle does not expose the required contract version.
    UnsupportedVersion = 7,
    /// Caller is not the configured fixture admin.
    Unauthorized = 8,
    /// A gate threshold or confidence floor exceeds the valid 0-100 scale.
    InvalidThreshold = 9,
    /// The AMM protocol circuit breaker is active.
    ContractPaused = 10,
    /// `initialize` has already been called; re-initialization is rejected.
    AlreadyInitialized = 11,
}

#[contracttype]
enum DataKey {
    Admin,
    /// Contract ID of the ScoreGate score registry this AMM trusts.
    ScoreGate,
    /// Risk-gate threshold (0-100) this AMM enforces.
    GateThreshold,
    /// Minimum confidence (0-100) required of the score backing a gate decision.
    MinConfidence,
    FailPolicy,
    MaxStalenessSecs,
    RequiredOracleVersion,
    ExpandedRiskScore,
    Paused,
}

#[contract]
pub struct MockAmm;

#[contractimpl]
impl MockAmm {
    /// One-time wiring: record the ScoreGate deployment, admin, version
    /// expectation, bounded freshness window, and failure policy enforced by
    /// this SDK conformance fixture.
    ///
    /// Re-initialization is rejected with `AlreadyInitialized` so an unrelated
    /// authenticated account cannot replace an established configuration.
    pub fn initialize(
        env: Env,
        admin: Address,
        scoregate: Address,
        gate_threshold: u32,
    ) -> Result<(), MockAmmError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(MockAmmError::AlreadyInitialized);
        }
        if gate_threshold > 100 {
            return Err(MockAmmError::InvalidThreshold);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::ScoreGate, &scoregate);
        env.storage().instance().set(&DataKey::GateThreshold, &gate_threshold);
        env.storage().instance().set(&DataKey::MinConfidence, &0u32);
        env.storage().instance().set(&DataKey::FailPolicy, &FailPolicy::FailClosed);
        env.storage().instance().set(&DataKey::MaxStalenessSecs, &604_800u64);
        env.storage().instance().set(&DataKey::RequiredOracleVersion, &0u32);
        let expanded_score = Self::oracle_has_expanded_score(&env, &scoregate);
        env.storage().instance().set(&DataKey::ExpandedRiskScore, &expanded_score);
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    /// Transfer administrative authority to `new_admin`. Requires authorization
    /// of the current admin and emits an `adm_upd` event. (Issue #202)
    pub fn set_admin(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), MockAmmError> {
        Self::require_admin(&env, &current_admin)?;
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        events::admin_updated(&env, &new_admin);
        Ok(())
    }

    /// Read the currently configured fixture admin.
    pub fn get_admin(env: Env) -> Result<Address, MockAmmError> {
        env.storage().instance().get(&DataKey::Admin).ok_or(MockAmmError::NotConfigured)
    }

    /// Pause or unpause the mock AMM. When paused, `swap` and
    /// `provide_liquidity_gated` reject invocations with `ContractPaused`. (Issue #203)
    pub fn set_paused(env: Env, admin: Address, paused: bool) -> Result<(), MockAmmError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &paused);
        events::paused_updated(&env, &paused);
        Ok(())
    }

    /// Returns `true` if the mock AMM is currently paused.
    pub fn is_paused(env: Env) -> bool {
        env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
    }

    /// Register or rotate the ScoreGate oracle this AMM consults for gate checks.
    pub fn set_risk_oracle(env: Env, admin: Address, oracle: Address) -> Result<(), MockAmmError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::ScoreGate, &oracle);
        let expanded_score = Self::oracle_has_expanded_score(&env, &oracle);
        env.storage().instance().set(&DataKey::ExpandedRiskScore, &expanded_score);
        events::oracle_updated(&env, &oracle);
        Ok(())
    }

    /// Re-probe the *currently configured* oracle's capabilities without
    /// changing its address. Needed when the oracle contract is upgraded in
    /// place (redeployed at the same address) — `set_risk_oracle` re-probes
    /// on an address change, but nothing previously re-probed a same-address
    /// upgrade. (Issue #120)
    pub fn refresh_oracle_capabilities(env: Env, admin: Address) -> Result<bool, MockAmmError> {
        Self::require_admin(&env, &admin)?;
        let scoregate: Address =
            env.storage().instance().get(&DataKey::ScoreGate).ok_or(MockAmmError::NotConfigured)?;
        let expanded_score = Self::oracle_has_expanded_score(&env, &scoregate);
        env.storage().instance().set(&DataKey::ExpandedRiskScore, &expanded_score);
        events::capabilities_refreshed(&env, expanded_score);
        Ok(expanded_score)
    }

    /// Configure the score and confidence floors enforced by
    /// `provide_liquidity_gated`.
    pub fn set_liquidity_gate_config(
        env: Env,
        admin: Address,
        gate_threshold: u32,
        min_confidence: u32,
        fail_policy: FailPolicy,
        max_staleness_secs: u64,
        required_oracle_version: u32,
    ) -> Result<(), MockAmmError> {
        Self::require_admin(&env, &admin)?;
        if gate_threshold > 100 || min_confidence > 100 {
            return Err(MockAmmError::InvalidThreshold);
        }
        env.storage().instance().set(&DataKey::GateThreshold, &gate_threshold);
        env.storage().instance().set(&DataKey::MinConfidence, &min_confidence);
        env.storage().instance().set(&DataKey::FailPolicy, &fail_policy);
        env.storage().instance().set(&DataKey::MaxStalenessSecs, &max_staleness_secs);
        env.storage().instance().set(&DataKey::RequiredOracleVersion, &required_oracle_version);
        events::gate_config_updated(&env, gate_threshold, min_confidence);
        Ok(())
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), MockAmmError> {
        let configured: Address =
            env.storage().instance().get(&DataKey::Admin).ok_or(MockAmmError::NotConfigured)?;
        if &configured != admin {
            return Err(MockAmmError::Unauthorized);
        }
        admin.

/* … truncated 7047 chars — edit only what you need near the top … */
