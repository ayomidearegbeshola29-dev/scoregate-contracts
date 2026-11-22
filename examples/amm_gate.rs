//! Reference integration: gating an AMM swap on a ScoreGate risk score.
//!
//! `ScoreGateGatedAmm` is a deliberately minimal contract — it is **not** a
//! real AMM. It exists purely to demonstrate the canonical composability
//! pattern from `docs/interface-spec.md`: call [`query_risk_gate`] inside your
//! guard clause and refuse to proceed for risky wallets.
//!
//! ## Fail-closed result vs. invocation failure
//!
//! `query_risk_gate` is **side-effect free** and returns a `bool`. A `false`
//! result is a *fail-closed gate decision*: ScoreGate ran successfully and
//! determined the wallet should be refused (unknown wallet, low confidence, or
//! score at/above the threshold). That is a normal, expected outcome.
//!
//! A `bool` return type does **not** mean the call cannot trap. Invoking a
//! contract that is absent, has been upgraded to an incompatible interface, or
//! otherwise errors will fail the invocation itself — the host raises an error
//! and, without a `try_*` wrapper, that error propagates and aborts the caller.
//! The gate target is caller-supplied (see [`ScoreGateGatedAmm::initialize`]),
//! so it may point at a missing or incompatible contract. Integrators must not
//! assume the call is infallible.
//!
//! The `swap` implementation below therefore uses the generated try client
//! (`try_query_risk_gate`) and maps invocation failure onto its own documented
//! error path ([`AmmError::GateUnavailable`]) rather than letting it trap.
//!
//! A consumer can also discover the published capability surface via
//! `get_interface_metadata` and use the `fail_closed` constraint to reason
//! about how unknown wallets and low-confidence scores should be handled.
//!
//! Build it as part of the workspace:
//!
//! ```text
//! cargo build --example amm_gate -p scoregate-score
//! ```
//!
//! [`query_risk_gate`]: scoregate_score::ScoreGateScoreContract::query_risk_gate

#![no_std]

use scoregate_score::ScoreGateScoreContractClient;
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env,
};

/// Errors surfaced by the gated AMM. `HighRiskWallet` is the one produced by
/// the ScoreGate guard clause; the rest are ordinary AMM bookkeeping.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum AmmError {
    /// The contract has not been pointed at a ScoreGate deployment yet.
    NotConfigured = 1,
    /// ScoreGate reports this wallet's risk score is at or above the gate
    /// threshold (or no score exists) — the swap is refused.
    HighRiskWallet = 2,
    /// Swap amount must be positive.
    InvalidAmount = 3,
    /// The ScoreGate call itself failed (absent or incompatible contract).
    /// This is an invocation failure, distinct from a fail-closed `false`.
    GateUnavailable = 4,
}

#[contracttype]
enum DataKey {
    /// Contract ID of the ScoreGate score registry this AMM trusts.
    ScoreGate,
}

/// The risk threshold this AMM enforces: wallets scoring `>= 75` (out of 100)
/// are turned away. Chosen to match ScoreGate's own default risk threshold.
const GATE_THRESHOLD: u32 = 75;

#[contract]
pub struct ScoreGateGatedAmm;

#[contractimpl]
impl ScoreGateGatedAmm {
    /// One-time wiring: record which ScoreGate deployment to consult.
    ///
    /// The address is caller-supplied, so it may be absent or point at an
    /// incompatible contract; `swap` handles that via `GateUnavailable`.
    /// A production AMM would protect this behind admin auth; omitted here to
    /// keep the example focused on the gating pattern.
    pub fn initialize(env: Env, scoregate: Address) {
        env.storage().instance().set(&DataKey::ScoreGate, &scoregate);
    }

    /// Process a swap, but only after clearing the swapper through ScoreGate.
    ///
    /// This is the pattern integrators should copy: build the generated client
    /// for the ScoreGate contract, ask `query_risk_gate` via the try client,
    /// and map both outcomes onto explicit domain errors. A `false` gate result
    /// becomes `HighRiskWallet`; an invocation failure (absent or incompatible
    /// dependency) becomes `GateUnavailable`. Neither traps the AMM.
    pub fn swap(env: Env, user: Address, amount: i128) -> Result<(), AmmError> {
        if amount <= 0 {
            return Err(AmmError::InvalidAmount);
        }

        let llens_contract: Address =
            env.storage().instance().get(&DataKey::ScoreGate).ok_or(AmmError::NotConfigured)?;

        let client = ScoreGateScoreContractClient::new(&env, &llens_contract);

        // Explicit consumer policy: treat invocation failure as unavailable.
        let is_safe = client
            .try_query_risk_gate(&user, &symbol_short!("XLM_USDC"), &GATE_THRESHOLD)
            .map_err(|_| AmmError::GateUnavailable)?
            .map_err(|_| AmmError::GateUnavailable)?;
        if !is_safe {
            return Err(AmmError::HighRiskWallet);
        }

        // ... rest of swap logic (reserves, pricing, transfers) would go here.
        Ok(())
    }
}
