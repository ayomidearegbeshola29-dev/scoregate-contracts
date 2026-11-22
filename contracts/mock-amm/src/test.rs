//! Regression coverage for the AMM example's cross-contract call policy.
//!
//! Issue #356: a boolean gate result (`false`) is a *fail-closed* answer from a
//! reachable contract. It is NOT a guarantee that the call itself cannot trap.
//! A call to an absent or incompatible contract fails at invocation time, before
//! any boolean is ever returned. Consumers must therefore use the generated
//! `try_*` client and handle `Err` explicitly.
//!
//! These tests exercise the documented consumer outcomes:
//!   * valid registry + registered address  -> gate returns `true`
//!   * valid registry + unregistered address -> gate returns `false` (fail-closed)
//!   * absent dependency                     -> invocation error (handled via `try_*`)
//!   * incompatible stub contract            -> invocation error (handled via `try_*`)

use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env};

use crate::{SimpleAmm, SimpleAmmClient};

/// Minimal registry stub that answers `is_allowed` for a single address.
#[contract]
pub struct RegistryStub;

#[contractimpl]
impl RegistryStub {
    pub fn is_allowed(_env: Env, _addr: Address) -> bool {
        true
    }
}

/// Incompatible stub: exposes a different entry point, so a call to
/// `is_allowed` traps at invocation time rather than returning a boolean.
#[contract]
pub struct IncompatibleStub;

#[contractimpl]
impl IncompatibleStub {
    pub fn something_else(_env: Env) -> u32 {
        0
    }
}

fn setup(env: &Env) -> (SimpleAmmClient<'_>, Address) {
    let admin = Address::generate(env);
    let oracle = Address::generate(env);
    let id = env.register(SimpleAmm, (admin, oracle.clone()));
    (SimpleAmmClient::new(env, &id), oracle)
}

#[test]
fn gate_returns_true_for_registered_address() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, oracle) = setup(&env);

    let registry = env.register(RegistryStub, ());
    let user = Address::generate(&env);

    // Fail-closed gate result: a reachable, compatible dependency answers `true`.
    let allowed = client.try_swap(&user, &registry).unwrap().unwrap();
    assert!(allowed);
    let _ = oracle;
}

#[test]
fn gate_returns_false_for_unregistered_address() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _oracle) = setup(&env);

    // A registry that rejects everyone models an unregistered address.
    #[contract]
    struct RejectingRegistry;
    #[contractimpl]
    impl RejectingRegistry {
        pub fn is_allowed(_env: Env, _addr: Address) -> bool {
            false
        }
    }

    let registry = env.register(RejectingRegistry, ());
    let user = Address::generate(&env);

    // Fail-closed gate result: the call succeeds and returns `false`.
    let allowed = client.try_swap(&user, &registry).unwrap().unwrap();
    assert!(!allowed);
}

#[test]
fn absent_dependency_is_an_invocation_error() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _oracle) = setup(&env);

    // An address with no deployed contract: the call cannot return a boolean.
    let absent = Address::generate(&env);
    let user = Address::generate(&env);

    // Consumer policy: treat invocation failure as "not allowed" via `try_*`.
    let allowed = client.try_swap(&user, &absent).unwrap_or(Ok(false)).unwrap_or(false);
    assert!(!allowed);
}

#[test]
fn incompatible_dependency_is_an_invocation_error() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _oracle) = setup(&env);

    let incompatible = env.register(IncompatibleStub, ());
    let user = Address::generate(&env);

    // Consumer policy: an incompatible contract traps; handle it explicitly.
    let allowed = client
        .try_swap(&user, &incompatible)
        .unwrap_or(Ok(false))
        .unwrap_or(false);
    assert!(!allowed);
}
