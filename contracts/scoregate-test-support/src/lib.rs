//! Shared test helpers for the ScoreGate contracts.
//!
//! Two flavours of environment are provided so tests can be explicit about
//! authorization:
//!
//! - [`strict_test_env`] returns a configured [`Env`] with **normal
//!   authorization enabled** — nothing is mocked. Use it for authorization
//!   regression tests where an unsigned call must be rejected.
//! - [`test_env`] (and [`test_env_unlimited_budget`]) return an [`Env`] with
//!   **blanket-mocked authorization** via `mock_all_auths`. Use them for
//!   positive-path fixtures that do not care about auth.

use soroban_sdk::{testutils::Address as _, Address, Env};

/// Returns a configured test [`Env`] with **normal authorization enabled**.
///
/// Unlike [`test_env`], this does *not* call `mock_all_auths`, so calls that
/// require authorization must be explicitly authorized by the caller. Use this
/// for focused authorization regression tests.
pub fn strict_test_env() -> Env {
    let env = Env::default();
    env.budget().reset_unlimited();
    env
}

/// Returns a configured test [`Env`] with **blanket-mocked authorization**.
///
/// This calls `mock_all_auths`, so every authorization check passes. Use it for
/// positive-path fixtures; use [`strict_test_env`] when authorization matters.
pub fn test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env
}

/// Returns a configured test [`Env`] with **blanket-mocked authorization** and
/// an unlimited budget.
///
/// This calls `mock_all_auths`, so every authorization check passes. Use it for
/// positive-path fixtures; use [`strict_test_env`] when authorization matters.
pub fn test_env_unlimited_budget() -> Env {
    let env = test_env();
    env.budget().reset_unlimited();
    env
}

/// Generates an `(admin, service)` role pair for the score contract.
pub fn generate_score_roles(env: &Env) -> (Address, Address) {
    (Address::generate(env), Address::generate(env))
}
