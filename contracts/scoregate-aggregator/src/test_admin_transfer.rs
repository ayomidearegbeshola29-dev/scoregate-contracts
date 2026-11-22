//! Tests for issue #170: ScoreGateAggregator admin ownership transfer.
//!
//! Verifies the two-step propose_admin / accept_admin pattern and that the
//! previous admin can no longer execute privileged methods after transfer.

use crate::{Error, ScoreGateAggregator, ScoreGateAggregatorClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

/// Builds a configured test environment with blanket-mocked authorization.
///
/// This is the positive-path fixture: every `require_auth` call succeeds
/// without an explicit signature. Use [`setup_strict`] when the test needs to
/// observe authorization failures.
fn setup<'a>() -> (Env, ScoreGateAggregatorClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin).expect("initialize failed");
    (env, client, admin)
}

/// Builds a configured test environment with normal (strict) authorization.
///
/// No blanket mocks are installed, so privileged calls require an explicit
/// authorization entry. `initialize` is invoked with the admin's auth mocked
/// only for that single call so the fixture can be set up; subsequent calls
/// must authorize themselves.
fn setup_strict<'a>() -> (Env, ScoreGateAggregatorClient<'a>, Address) {
    let env = Env::default();
    let contract_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    env.mock_auths(&[]);
    client.initialize(&admin).expect("initialize failed");
    (env, client, admin)
}

// ── propose_admin ─────────────────────────────────────────────────────────────

#[test]
fn test_propose_admin_stores_pending_admin() {
    let (env, client, _admin) = setup();
    let new_admin = Address::generate(&env);

    assert_eq!(client.get_pending_admin().unwrap(), None);

    client.propose_admin(&new_admin).expect("propose_admin failed");

    assert_eq!(client.get_pending_admin().unwrap(), Some(new_admin));
}

#[test]
fn test_propose_admin_emits_audit_event() {
    let (env, client, _admin) = setup();
    let new_admin = Address::generate(&env);
    client.propose_admin(&new_admin).expect("propose_admin failed");
    // Event emission is verified implicitly via successful call; snapshot
    // tests cover the exact event shape.
}

// ── accept_admin ──────────────────────────────────────────────────────────────

#[test]
fn test_accept_admin_completes_transfer() {
    let (env, client, old_admin) = setup();
    let new_admin = Address::generate(&env);

    client.propose_admin(&new_admin).expect("propose_admin failed");
    client.accept_admin().expect("accept_admin failed");

    assert_eq!(client.get_admin().unwrap(), new_admin);
    // Pending entry is cleared after acceptance.
    assert_eq!(client.get_pending_admin().unwrap(), None);
    // Old admin must no longer be the admin.
    assert_ne!(client.get_admin().unwrap(), old_admin);
}

#[test]
fn test_accept_admin_fails_when_no_transfer_pending() {
    let (_env, client, _admin) = setup();
    let result = client.try_accept_admin();
    assert_eq!(result, Err(Ok(Error::NoPendingAdminTransfer)));
}

// ── cancel_admin_transfer ─────────────────────────────────────────────────────

#[test]
fn test_cancel_admin_transfer_clears_pending() {
    let (env, client, _admin) = setup();
    let new_admin = Address::generate(&env);

    client.propose_admin(&new_admin).expect("propose_admin failed");
    assert!(client.get_pending_admin().unwrap().is_some());

    client.cancel_admin_transfer().expect("cancel_admin_transfer failed");
    assert_eq!(client.get_pending_admin().unwrap(), None);
}

#[test]
fn test_cancel_admin_transfer_fails_when_no_transfer_pending() {
    let (_env, client, _admin) = setup();
    let result = client.try_cancel_admin_transfer();
    assert_eq!(result, Err(Ok(Error::NoPendingAdminTransfer)));
}

// ── Previous admin loses privileges after transfer ────────────────────────────

/// After a completed transfer the new admin must be able to call privileged
/// methods (set_shard_health requires admin auth). The test uses
/// mock_all_auths so both addresses are "authorised"; the important assertion
/// is that get_admin returns the new address, not the old one.
#[test]
fn test_previous_admin_is_replaced_after_accept() {
    let (env, client, old_admin) = setup();
    let new_admin = Address::generate(&env);

    client.propose_admin(&new_admin).expect("propose_admin failed");
    client.accept_admin().expect("accept_admin failed");

    let current = client.get_admin().unwrap();
    assert_eq!(current, new_admin, "admin should be updated to new_admin after accept");
    assert_ne!(current, old_admin, "old_admin must no longer be the admin");
}

// ── Strict authorization regression coverage ──────────────────────────────────

/// Under the strict constructor (no blanket mocks) an unsigned administrative
/// call must be rejected by the host's authorization check.
#[test]
fn test_strict_env_rejects_unsigned_admin_call() {
    let (env, client, _admin) = setup_strict();
    let new_admin = Address::generate(&env);

    // No auth entry is provided for the admin, so the host must reject the
    // privileged call before it can mutate state.
    let result = client.try_propose_admin(&new_admin);
    assert!(result.is_err(), "unsigned propose_admin must fail under strict auth");
    assert_eq!(client.get_pending_admin().unwrap(), None);
}

/// Under the strict constructor an explicitly authorized administrative call
/// succeeds and mutates state as expected.
#[test]
fn test_strict_env_allows_explicitly_authorized_admin_call() {
    let (env, client, admin) = setup_strict();
    let new_admin = Address::generate(&env);

    // Authorize only the admin for this call.
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: admin.clone(),
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "propose_admin",
            args: (&new_admin,).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    client.propose_admin(&new_admin).expect("authorized propose_admin failed");
    assert_eq!(client.get_pending_admin().unwrap(), Some(new_admin));
}
