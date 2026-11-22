// Example only — not production code

//! Minimal AMM swap implementation with ScoreGate risk gating.
//!
//! This example demonstrates:
//! - Importing the ScoreGate contract client.
//! - Calling `query_risk_gate` from within a swap function.
//! - Handling the gate result and rejecting high-risk wallets.
//! - Distinguishing a fail-closed gate result from an invocation failure.
//!
//! # Fail-closed result vs. invocation failure
//!
//! `query_risk_gate` returns a `bool`: `true` when the score is below the
//! threshold, `false` otherwise. All *gate-level* failure cases (too risky,
//! embargoed, unknown wallet) collapse to `false` — the gate is fail-closed.
//!
//! A boolean return type does **not** mean the cross-contract call itself
//! cannot trap. If the `scoregate_id` passed to `swap` points at an address
//! that is absent, not a ScoreGate contract, or otherwise incompatible, the
//! invocation fails (traps) before any `bool` is produced. Because `SimpleAMM`
//! accepts the oracle address as a caller-supplied argument, the gate target
//! may be missing or incompatible at runtime.
//!
//! Consumers must therefore handle both outcomes explicitly. This example uses
//! the generated try client (`try_query_risk_gate`) so that an invocation
//! failure is surfaced through the documented error path
//! (`AmmError::GateUnavailable`) instead of trapping the swap.

#![no_std]

use scoregate_score::ScoreGateScoreContractClient;
use soroban_sdk::{contract, contracterror, contractimpl, Address, Env, Symbol};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AmmError {
    /// The gate returned `false` (too risky, embargoed, or unknown wallet).
    UserHighRisk = 1,
    /// The gate invocation failed: the target is absent or incompatible.
    GateUnavailable = 2,
}

#[contract]
pub struct SimpleAMM;

#[contractimpl]
impl SimpleAMM {
    /// Execute a swap between two tokens, enforcing ScoreGate risk gating.
    ///
    /// Before proceeding with the swap, this function calls `query_risk_gate`
    /// to verify that the user's risk score is below the specified threshold.
    ///
    /// Two distinct failure modes are handled:
    /// - The gate returns `false` (user is too risky, embargoed, or unknown):
    ///   the swap is rejected with `AmmError::UserHighRisk`.
    /// - The gate invocation itself fails (the `scoregate_id` is absent or
    ///   incompatible): the swap is rejected with `AmmError::GateUnavailable`.
    ///
    /// The `scoregate_id` is caller-supplied, so the gate target may be missing
    /// or incompatible; a boolean return type does not prevent the call from
    /// trapping. The try client is used to convert that trap into a documented
    /// error.
    pub fn swap(
        env: Env,
        user: Address,
        asset_pair: Symbol,
        amount_in: u64,
        scoregate_id: Address,
        gate_threshold: u32,
    ) -> Result<u64, AmmError> {
        // 1. Build the ScoreGate client
        let client = ScoreGateScoreContractClient::new(&env, &scoregate_id);

        // 2. Call query_risk_gate via the try client. A `false` result is a
        //    fail-closed gate outcome; an `Err` is an invocation failure from
        //    an absent or incompatible contract. Both are handled explicitly.
        let passes_gate = match client.try_query_risk_gate(&user, &asset_pair, &gate_threshold) {
            Ok(passes) => passes,
            Err(_) => return Err(AmmError::GateUnavailable),
        };

        // 3. Reject the swap if the user's risk gate fails
        if !passes_gate {
            return Err(AmmError::UserHighRisk);
        }

        // 4. User passed the gate; proceed with swap logic
        // (In a real AMM, this would include pool checks, reserve calculations, etc.)
        let output_amount = Self::compute_swap_output(amount_in);

        Ok(output_amount)
    }

    /// Simplified swap output calculation (not production logic).
    fn compute_swap_output(amount_in: u64) -> u64 {
        // Example: simple 1:1 swap with a 0.3% fee.
        // Perform the intermediate multiplication in u128 so the full u64
        // input domain is supported without overflow; the quotient is bounded
        // by u64::MAX and fits back into u64. Floor rounding is preserved.
        let numerator = (amount_in as u128) * 997;
        (numerator / 1000) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scoregate_score::ScoreGateScoreContract;
    use soroban_sdk::testutils::{Address as _, Ledger as _};
    use soroban_sdk::{contract, contractimpl, symbol_short, Vec};

    const GATE_THRESHOLD: u32 = 75;

    struct Fixture<'a> {
        env: Env,
        scoregate: ScoreGateScoreContractClient<'a>,
        scoregate_id: Address,
        amm: SimpleAMMClient<'a>,
    }

    /// Deploys a real `ScoreGateScoreContract` plus a real `SimpleAMM` in the
    /// same `Env`, so `swap` exercises the actual cross-contract call to
    /// `query_risk_gate` rather than a mocked gate check.
    fn setup<'a>() -> Fixture<'a> {
        let env = Env::default();
        env.mock_all_auths();

        let scoregate_id = env.register_contract(None, ScoreGateScoreContract);
        let scoregate = ScoreGateScoreContractClient::new(&env, &scoregate_id);
        let admin = Address::generate(&env);
        let service = Address::generate(&env);
        scoregate.initialize(&admin, &service);

        let amm_id = env.register_contract(None, SimpleAMM);
        let amm = SimpleAMMClient::new(&env, &amm_id);

        Fixture { env, scoregate, scoregate_id, amm }
    }

    /// Submits a score for `wallet`, advancing the ledger past the 1-hour
    /// cooldown first so repeated submissions in the same test never collide.
    fn submit_score(fixture: &Fixture, wallet: &Address, score: u32) {
        fixture.env.ledger().with_mut(|l| l.timestamp += 3_601);
        fixture.scoregate.submit_score(
            &Vec::new(&fixture.env),
            wallet,
            &symbol_short!("XLM_USDC"),
            &score,
            &false,
            &false,
            &fixture.env.ledger().timestamp(),
            &95,
            &1,
            &None,
        );
    }

    /// u128 reference implementation of the fee calculation, used to validate
    /// the contract's output across the full u64 input domain.
    fn reference_swap_output(amount_in: u64) -> u64 {
        ((amount_in as u128) * 997 / 1000) as u64
    }

    /// Incompatible stub contract: exposes no `query_risk_gate` entry point, so
    /// invoking it as a gate traps. Used to prove the consumer policy surfaces
    /// an invocation failure through the documented error path.
    #[contract]
    pub struct IncompatibleGate;

    #[contractimpl]
    impl IncompatibleGate {
        pub fn not_a_gate(_env: Env) -> u32 {
            0
        }
    }

    #[test]
    fn test_swap_with_passing_gate() {
        let fixture = setup();
        let user = Address::generate(&fixture.env);
        submit_score(&fixture, &user, 10); // 10 < GATE_THRESHOLD(75)

        let result = fixture.amm.try_swap(
            &user,
            &symbol_short!("XLM_USDC"),
            &1_000_000,
            &fixture.scoregate_id,
            &GATE_THRESHOLD,
        );

        assert_eq!(result, Ok(Ok(997_000)));
    }

    #[test]
    fn test_swap_with_failing_gate() {
        let fixture = setup();
        let user = Address::generate(&fixture.env);
        submit_score(&fixture, &user, 90); // 90 >= GATE_THRESHOLD(75)

        let result = fixture.amm.try_swap(
            &user,
            &symbol_short!("XLM_USDC"),
            &1_000_000,
            &fixture.scoregate_id,
            &GATE_THRESHOLD,
        );

        assert_eq!(result, Err(Ok(AmmError::UserHighRisk)));
    }

    #[test]
    fn test_swap_with_unknown_wallet() {
        let fixture = setup();
        let user = Address::generate(&fixture.env); // never scored

        let result = fixture.amm.try_swap(
            &user,
            &symbol_short!("XLM_USDC"),
            &1_000_000,
            &fixture.scoregate_id,
            &GATE_THRESHOLD,
        );

        assert_eq!(result, Err(Ok(AmmError::UserHighRisk)));
    }

    #[test]
    fn test_swap_with_unregistered_gate() {
        let fixture = setup();
        let user = Address::generate(&fixture.env);
        submit_score(&fixture, &user, 10);

        // An address with no contract deployed: the invocation traps, and the
        // consumer policy maps it to the documented error path.
        let missing_gate = Address::generate(&fixture.env);

        let result = fixture.amm.try_swap(
            &user,
            &symbol_short!("XLM_USDC"),
            &1_000_000,
            &missing_gate,
            &GATE_THRESHOLD,
        );

        assert_eq!(result, Err(Ok(AmmError::GateUnavailable)));
    }

    #[test]
    fn test_swap_with_incompatible_gate() {
        let fixture = setup();
        let user = Address::generate(&fixture.env);
        submit_score(&fixture, &user, 10);

        // A deployed contract that does not implement `query_risk_gate`: the
        // invocation traps, and the consumer policy maps it to the documented
        // error path rather than propagating the trap.
        let incompatible_gate = fixture.env.register_contract(None, IncompatibleGate);

        let result = fixture.amm.try_swap(
            &user,
            &symbol_short!("XLM_USDC"),
            &1_000_000,
            &incompatible_gate,
            &GATE_THRESHOLD,
        );

        assert_eq!(result, Err(Ok(AmmError::GateUnavailable)));
    }

    #[test]
    fn test_compute_swap_output_no_overflow() {
        // Boundary values around the u64::MAX / 997 overflow threshold.
        let threshold = u64::MAX / 997;
        let cases = [0u64, 1, threshold, threshold + 1, u64::MAX];

        for &amount_in in cases.iter() {
            let expected = reference_swap_output(amount_in);
            let actual = SimpleAMM::compute_swap_output(amount_in);
            assert_eq!(actual, expected, "mismatch for amount_in = {}", amount_in);
        }
    }

    #[test]
    fn test_compute_swap_output_preserves_example() {
        assert_eq!(SimpleAMM::compute_swap_output(1_000_000), 997_000);
    }
}
