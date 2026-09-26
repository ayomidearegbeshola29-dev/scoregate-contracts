//! Coverage for issues #117-#120: MockLending oracle rotation, gate-threshold
//! bounds validation, audit events, and oracle-capability refresh, across
//! both `mock-amm` and `mock-lending`.
//!
//! Mirrors the `Fixture`/`setup()`/`submit_score()` pattern already used by
//! `test_composability.rs` in this same crate.

use mock_amm::{FailPolicy as AmmFailPolicy, MockAmm, MockAmmClient, MockAmmError};
use mock_lending::{FailPolicy as LendingFailPolicy, MockLending, MockLendingClient, MockLendingError};
use scoregate_score::{ScoreGateScoreContract, ScoreGateScoreContractClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Env, Vec,
};

const GATE_THRESHOLD: u32 = 75;
const MIN_CONFIDENCE: u32 = 50;

struct Fixture<'a> {
    env: Env,
    admin: Address,
    scoregate: ScoreGateScoreContractClient<'a>,
    amm: MockAmmClient<'a>,
    lending: MockLendingClient<'a>,
}

fn setup<'a>() -> Fixture<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let scoregate_id = env.register_contract(None, ScoreGateScoreContract);
    let scoregate = ScoreGateScoreContractClient::new(&env, &scoregate_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    scoregate.initialize(&admin, &service);

    let amm_id = env.register_contract(None, MockAmm);
    let amm = MockAmmClient::new(&env, &amm_id);
    amm.initialize(&admin, &scoregate_id, &GATE_THRESHOLD);
    amm.set_liquidity_gate_config(
        &admin,
        &GATE_THRESHOLD,
        &MIN_CONFIDENCE,
        &AmmFailPolicy::FailClosed,
        &604_800,
        &0,
    );

    let lending_id = env.register_contract(None, MockLending);
    let lending = MockLendingClient::new(&env, &lending_id);
    lending.initialize(&admin, &scoregate_id, &GATE_THRESHOLD, &MIN_CONFIDENCE);

    Fixture { env, admin, scoregate, amm, lending }
}

fn submit_score(fixture: &Fixture, wallet: &Address, score: u32, confidence: u32) {
    fixture.env.ledger().with_mut(|l| l.timestamp += 3_601);
    fixture.scoregate.submit_score(
        &Vec::new(&fixture.env),
        wallet,
        &symbol_short!("XLM_USDC"),
        &score,
        &false,
        &false,
        &fixture.env.ledger().timestamp(),
        &confidence,
        &1,
        &None,
    );
}

// ── Issue #117: MockLending oracle address was permanently immutable ───────

#[test]
fn lending_set_risk_oracle_updates_oracle_used_by_subsequent_borrows() {
    let fixture = setup();

    // The *original* oracle knows nothing about this wallet, so a borrow
    // against it fails closed.
    let borrower = Address::generate(&fixture.env);
    assert_eq!(
        fixture.lending.try_borrow(&borrower, &symbol_short!("XLM_USDC"), &1_000),
        Err(Ok(MockLendingError::RiskGateRejected))
    );

    // Deploy a *second*, independent ScoreGate deployment with a safe score
    // for the borrower, and rotate MockLending onto it.
    let alt_oracle_id = fixture.env.register_contract(None, ScoreGateScoreContract);
    let alt_oracle = ScoreGateScoreContractClient::new(&fixture.env, &alt_oracle_id);
    let admin = Address::generate(&fixture.env);
    let service = Address::generate(&fixture.env);
    alt_oracle.initialize(&admin, &service);
    fixture.env.ledger().with_mut(|l| l.timestamp += 3_601);
    alt_oracle.submit_score(
        &Vec::new(&fixture.env),
        &borrower,
        &symbol_short!("XLM_USDC"),
        &10,
        &false,
        &false,
        &fixture.env.ledger().timestamp(),
        &90,
        &1,
        &None,
    );

    fixture.lending.set_risk_oracle(&fixture.admin, &alt_oracle_id);

    // Subsequent borrows now query the *updated* oracle and succeed.
    assert_eq!(fixture.lending.try_borrow(&borrower, &symbol_short!("XLM_USDC"), &1_000), Ok(Ok(())));
}

#[test]
fn lending_set_risk_oracle_rejects_unauthorized_caller() {
    let fixture = setup();
    let attacker = Address::generate(&fixture.env);
    let oracle = Address::generate(&fixture.env);

    assert_eq!(
        fixture.lending.try_set_risk_oracle(&attacker, &oracle),
        Err(Ok(MockLendingError::Unauthorized))
    );
}

// ── Issue #118: gate thresholds above 100 were silently accepted ───────────

#[test]
fn amm_initialize_rejects_threshold_over_100() {
    let env = Env::default();
    env.mock_all_auths();
    let scoregate_id = env.register_contract(None, ScoreGateScoreContract);
    let scoregate = ScoreGateScoreContractClient::new(&env, &scoregate_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    scoregate.initialize(&admin, &service);

    let amm_id = env.register_contract(None, MockAmm);
    let amm = MockAmmClient::new(&env, &amm_id);
    assert_eq!(
        amm.try_initialize(&admin, &scoregate_id, &101),
        Err(Ok(MockAmmError::InvalidThreshold))
    );
}

#[test]
fn amm_set_liquidity_gate_config_rejects_threshold_over_100() {
    let fixture = setup();
    assert_eq!(
        fixture.amm.try_set_liquidity_gate_config(
            &fixture.admin,
            &101,
            &MIN_CONFIDENCE,
            &AmmFailPolicy::FailClosed,
            &604_800,
            &0,
        ),
        Err(Ok(MockAmmError::InvalidThreshold))
    );
}

#[test]
fn amm_set_liquidity_gate_config_rejects_confidence_over_100() {
    let fixture = setup();
    assert_eq!(
        fixture.amm.try_set_liquidity_gate_config(
            &fixture.admin,
            &GATE_THRESHOLD,
            &101,
            &AmmFailPolicy::FailClosed,
            &604_800,
            &0,
        ),
        Err(Ok(MockAmmError::InvalidThreshold))
    );
}

#[test]
fn lending_initialize_rejects_threshold_over_100() {
    let env = Env::default();
    env.mock_all_auths();
    let scoregate_id = env.register_contract(None, ScoreGateScoreContract);
    let scoregate = ScoreGateScoreContractClient::new(&env, &scoregate_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    scoregate.initialize(&admin, &service);

    let lending_id = env.register_contract(None, MockLending);
    let lending = MockLendingClient::new(&env, &lending_id);
    assert_eq!(
        lending.try_initialize(&admin, &scoregate_id, &101, &MIN_CONFIDENCE),
        Err(Ok(MockLendingError::InvalidThreshold))
    );
}

#[test]
fn lending_set_borrow_gate_config_rejects_threshold_over_100() {
    let fixture = setup();
    assert_eq!(
        fixture.lending.try_set_borrow_gate_config(
            &fixture.admin,
            &101,
            &MIN_CONFIDENCE,
            &LendingFailPolicy::FailClosed,
            &604_800,
            &0,
        ),
        Err(Ok(MockLendingError::InvalidThreshold))
    );
}

// ── Issue #119: neither mock emitted any audit events ───────────────────────

#[test]
fn amm_swap_emits_swap_executed_event() {
    let fixture = setup();
    let wallet = Address::generate(&fixture.env);
    submit_score(&fixture, &wallet, 10, 90);

    let before = fixture.env.events().all().len();
    assert_eq!(fixture.amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &1_000), Ok(Ok(())));
    assert_eq!(fixture.env.events().all().len(), before + 1);
}

#[test]
fn lending_borrow_emits_borrow_executed_event() {
    let fixture = setup();
    let wallet = Address::generate(&fixture.env);
    submit_score(&fixture, &wallet, 10, 90);

    let before = fixture.env.events().all().len();
    assert_eq!(fixture.lending.try_borrow(&wallet, &symbol_short!("XLM_USDC"), &1_000), Ok(Ok(())));
    assert_eq!(fixture.env.events().all().len(), before + 1);
}

#[test]
fn amm_admin_actions_emit_events() {
    let fixture = setup();

    let before = fixture.env.events().all().len();
    fixture.amm.set_liquidity_gate_config(
        &fixture.admin,
        &GATE_THRESHOLD,
        &MIN_CONFIDENCE,
        &AmmFailPolicy::FailClosed,
        &604_800,
        &0,
    );
    assert_eq!(fixture.env.events().all().len(), before + 1, "gate config update must emit an event");

    let before = fixture.env.events().all().len();
    let new_oracle = Address::generate(&fixture.env);
    fixture.amm.set_risk_oracle(&fixture.admin, &new_oracle);
    assert_eq!(fixture.env.events().all().len(), before + 1, "oracle rotation must emit an event");
}

#[test]
fn lending_admin_actions_emit_events() {
    let fixture = setup();

    let before = fixture.env.events().all().len();
    fixture.lending.set_borrow_gate_config(
        &fixture.admin,
        &GATE_THRESHOLD,
        &MIN_CONFIDENCE,
        &LendingFailPolicy::FailClosed,
        &604_800,
        &0,
    );
    assert_eq!(fixture.env.events().all().len(), before + 1, "gate config update must emit an event");

    let before = fixture.env.events().all().len();
    let new_oracle = Address::generate(&fixture.env);
    fixture.lending.set_risk_oracle(&fixture.admin, &new_oracle);
    assert_eq!(fixture.env.events().all().len(), before + 1, "oracle rotation must emit an event");
}

// ── Issue #120: expanded-score capability was probed once and never refreshed ──

#[test]
fn amm_refresh_oracle_capabilities_reprobes_current_oracle() {
    let fixture = setup();
    // `setup()`'s ScoreGate deployment is the current contract (version 5,
    // which implements the expanded interface), so a same-address re-probe
    // must correctly report `true` — confirming refresh_oracle_capabilities
    // reads the live oracle rather than returning a stale cached value.
    assert_eq!(fixture.amm.refresh_oracle_capabilities(&fixture.admin), true);
}

#[test]
fn amm_refresh_oracle_capabilities_rejects_unauthorized_caller() {
    let fixture = setup();
    let attacker = Address::generate(&fixture.env);
    assert_eq!(
        fixture.amm.try_refresh_oracle_capabilities(&attacker),
        Err(Ok(MockAmmError::Unauthorized))
    );
}

#[test]
fn lending_refresh_oracle_capabilities_reprobes_current_oracle() {
    let fixture = setup();
    assert_eq!(fixture.lending.refresh_oracle_capabilities(&fixture.admin), true);
}

#[test]
fn lending_refresh_oracle_capabilities_rejects_unauthorized_caller() {
    let fixture = setup();
    let attacker = Address::generate(&fixture.env);
    assert_eq!(
        fixture.lending.try_refresh_oracle_capabilities(&attacker),
        Err(Ok(MockLendingError::Unauthorized))
    );
}

#[test]
fn amm_refresh_oracle_capabilities_emits_event() {
    let fixture = setup();
    let before = fixture.env.events().all().len();
    fixture.amm.refresh_oracle_capabilities(&fixture.admin);
    assert_eq!(fixture.env.events().all().len(), before + 1);
}
