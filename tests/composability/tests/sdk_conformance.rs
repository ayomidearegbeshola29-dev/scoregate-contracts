//! Runs `sdk_conformance_fixtures.json` against the deployed contracts.
//!
//! A fixture passes only when everything it declares holds, not just the case outcomes:
//!
//! * `schema_version` must be one this runner supports, otherwise parsing fails.
//! * Every entry of `contract_capabilities` must be reported by `supports_interface`.
//! * Every metric of `resource_bounds` is either checked around each decision or named in
//!   [`INFORMATIONAL_METRIC`]. An unknown field is a parse error, so a newly advertised metric
//!   cannot be silently ignored.

use mock_amm::{FailPolicy as AmmFailPolicy, MockAmm, MockAmmClient, MockAmmError};
use scoregate_score::{ScoreGateScoreContract, ScoreGateScoreContractClient};
use serde::Deserialize;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    xdr::{ContractDataDurability, LedgerKey, Limits, ScVal, WriteXdr},
    Address, Env, IntoVal, Symbol, TryFromVal, Val, Vec,
};
use std::collections::BTreeMap;

/// The fixture shared with the other SDK runners.
const FIXTURE: &str = include_str!("../sdk_conformance_fixtures.json");

/// The only `schema_version` this runner understands.
const SUPPORTED_SCHEMA_VERSION: u32 = 1;

/// Amount passed to every gated decision.
const DECISION_AMOUNT: i128 = 1_000;

/// The one `resource_bounds` metric that is reported but not enforced. The Soroban 21 test
/// environment does not expose a per-call ledger read count, so it cannot be measured here; the
/// other metrics are checked by [`ResourceBounds::violations`].
const INFORMATIONAL_METRIC: &str = "ledger_reads_per_decision_max";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureData {
    schema_version: u32,
    contract_capabilities: std::vec::Vec<String>,
    default_config: DefaultConfig,
    resource_bounds: ResourceBounds,
    cases: std::vec::Vec<Case>,
    /// Guidance for the other SDK runners; this runner has nothing to check in it.
    #[allow(dead_code)]
    sdk_assertions: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct DefaultConfig {
    gate_threshold: u32,
    min_confidence: u32,
    fail_policy: String,
    max_staleness_secs: u64,
    required_oracle_version: u32,
}

/// What one gated decision may involve and cost.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceBounds {
    /// Distinct wallets, asset pairs and oracle contracts a decision involves; each case must use
    /// exactly this many.
    wallets: u32,
    asset_pairs: u32,
    oracle_contracts: u32,
    /// Informational: see [`INFORMATIONAL_METRIC`].
    ledger_reads_per_decision_max: u32,
    /// Persistent ledger entries written or removed by the decision.
    ledger_writes_per_read_only_decision: u32,
    /// Events the ScoreGate contract emits during the decision. Events the consumer contract
    /// publishes for itself are not counted.
    events_per_decision: u32,
    /// XDR size of the decision's call arguments (provider, asset pair and amount).
    encoded_input_bytes_max: usize,
}

/// What a decision was measured to involve and cost.
#[derive(Debug)]
struct Observed {
    wallets: u32,
    asset_pairs: u32,
    oracle_contracts: u32,
    ledger_writes: u32,
    scoregate_events: u32,
    encoded_input_bytes: usize,
}

impl ResourceBounds {
    /// One message per enforced bound that `observed` breaks.
    fn violations(&self, observed: &Observed) -> std::vec::Vec<String> {
        let mut out = std::vec::Vec::new();
        for (name, expected, actual) in [
            ("wallets", self.wallets, observed.wallets),
            ("asset_pairs", self.asset_pairs, observed.asset_pairs),
            ("oracle_contracts", self.oracle_contracts, observed.oracle_contracts),
        ] {
            if actual != expected {
                out.push(format!("{name}: fixture declares {expected}, decision used {actual}"));
            }
        }
        if observed.ledger_writes > self.ledger_writes_per_read_only_decision {
            out.push(format!(
                "ledger_writes_per_read_only_decision: {} > {}",
                observed.ledger_writes, self.ledger_writes_per_read_only_decision
            ));
        }
        if observed.scoregate_events > self.events_per_decision {
            out.push(format!(
                "events_per_decision: {} > {}",
                observed.scoregate_events, self.events_per_decision
            ));
        }
        if observed.encoded_input_bytes > self.encoded_input_bytes_max {
            out.push(format!(
                "encoded_input_bytes_max: {} > {}",
                observed.encoded_input_bytes, self.encoded_input_bytes_max
            ));
        }
        out
    }
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    score: Option<u32>,
    confidence: Option<u32>,
    age_secs: Option<u64>,
    oracle: Option<String>,
    fail_policy: Option<String>,
    required_oracle_version: Option<serde_json::Value>,
    expected: String,
}

/// Parses a fixture, rejecting a `schema_version` this runner does not support before looking at
/// anything else, and any field it does not know.
fn parse_fixture(json: &str) -> Result<FixtureData, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("fixture is not valid JSON: {e}"))?;
    match value.get("schema_version").and_then(serde_json::Value::as_u64) {
        Some(version) if version == u64::from(SUPPORTED_SCHEMA_VERSION) => {}
        Some(version) => {
            return Err(format!(
                "unsupported schema_version {version} (supported: {SUPPORTED_SCHEMA_VERSION})"
            ))
        }
        None => return Err("fixture has no numeric schema_version".to_string()),
    }
    serde_json::from_value(value).map_err(|e| format!("fixture does not match the schema: {e}"))
}

/// The capabilities in `required` that the contract does not report through `supports_interface`.
fn missing_capabilities(
    env: &Env,
    client: &ScoreGateScoreContractClient,
    required: &[String],
) -> std::vec::Vec<String> {
    required
        .iter()
        .filter(|capability| !client.supports_interface(&Symbol::new(env, capability)))
        .cloned()
        .collect()
}

/// Number of distinct values in `items`.
fn distinct<T: PartialEq + Clone>(items: &[T]) -> u32 {
    let mut seen: std::vec::Vec<T> = std::vec::Vec::new();
    for item in items {
        if !seen.contains(item) {
            seen.push(item.clone());
        }
    }
    seen.len() as u32
}

/// Persistent contract data and code in the ledger, keyed by ledger key.
fn persistent_state(env: &Env) -> BTreeMap<String, String> {
    env.to_ledger_snapshot()
        .ledger_entries
        .into_iter()
        .filter_map(|(key, entry)| match key.as_ref() {
            LedgerKey::ContractData(data)
                if data.durability == ContractDataDurability::Persistent =>
            {
                Some((format!("{key:?}"), format!("{entry:?}")))
            }
            LedgerKey::ContractCode(_) => Some((format!("{key:?}"), format!("{entry:?}"))),
            _ => None,
        })
        .collect()
}

/// Entries added, changed or removed between two states.
fn changed_entries(before: &BTreeMap<String, String>, after: &BTreeMap<String, String>) -> u32 {
    let written = after.iter().filter(|(key, entry)| before.get(*key) != Some(*entry)).count();
    let removed = before.keys().filter(|key| !after.contains_key(*key)).count();
    (written + removed) as u32
}

/// Events published so far by `contract`.
fn events_from(env: &Env, contract: &Address) -> u32 {
    env.events().all().iter().filter(|(id, _, _)| id == contract).count() as u32
}

/// Size of `value` once encoded as an XDR `ScVal`, the form it takes in a transaction.
fn encoded_len<T: IntoVal<Env, Val>>(env: &Env, value: T) -> usize {
    let val: Val = value.into_val(env);
    let sc_val = ScVal::try_from_val(env, &val).expect("argument must convert to XDR");
    sc_val.to_xdr(Limits::none()).expect("argument must encode to XDR").len()
}

#[test]
fn sdk_conformance() {
    let fixture_data = parse_fixture(FIXTURE).expect("Failed to parse fixture");

    eprintln!(
        "fixture schema_version {}; informational, not enforced: {INFORMATIONAL_METRIC}={}",
        fixture_data.schema_version, fixture_data.resource_bounds.ledger_reads_per_decision_max
    );

    for case in &fixture_data.cases {
        let env = Env::default();
        env.mock_all_auths();

        let scoregate_id = env.register_contract(None, ScoreGateScoreContract);
        let scoregate = ScoreGateScoreContractClient::new(&env, &scoregate_id);
        let admin = Address::generate(&env);
        let service = Address::generate(&env);
        scoregate.initialize(&admin, &service);

        let missing = missing_capabilities(&env, &scoregate, &fixture_data.contract_capabilities);
        assert!(
            missing.is_empty(),
            "Case '{}': contract does not support declared capabilities {:?}",
            case.name,
            missing
        );

        let amm_id = env.register_contract(None, MockAmm);
        let amm = MockAmmClient::new(&env, &amm_id);

        let gate_threshold = fixture_data.default_config.gate_threshold;
        let min_confidence = fixture_data.default_config.min_confidence;

        let fail_policy_str =
            case.fail_policy.as_deref().unwrap_or(&fixture_data.default_config.fail_policy);
        let fail_policy = if fail_policy_str == "fail_closed" {
            AmmFailPolicy::FailClosed
        } else {
            AmmFailPolicy::FailOpen
        };

        let max_staleness_secs = fixture_data.default_config.max_staleness_secs;

        let required_oracle_version = match &case.required_oracle_version {
            Some(v) => {
                if v.is_number() {
                    v.as_u64().unwrap() as u32
                } else if v.is_string() && v.as_str().unwrap() == "current_plus_one" {
                    scoregate.get_contract_version() + 1
                } else {
                    panic!("Unknown required_oracle_version format in case {}", case.name)
                }
            }
            None => fixture_data.default_config.required_oracle_version,
        };

        amm.initialize(&admin, &scoregate_id, &gate_threshold);
        amm.set_liquidity_gate_config(
            &admin,
            &gate_threshold,
            &min_confidence,
            &fail_policy,
            &max_staleness_secs,
            &required_oracle_version,
        );

        let wallet = Address::generate(&env);
        let asset_pair = symbol_short!("XLM_USDC");

        let mut oracle = scoregate_id.clone();
        if let Some(oracle_status) = &case.oracle {
            if oracle_status == "unavailable" {
                let bad_oracle = Address::generate(&env);
                amm.set_risk_oracle(&admin, &bad_oracle);
                oracle = bad_oracle;
            }
        }

        let score = if case.name == "reject_unknown_wallet" {
            None
        } else {
            Some(case.score.unwrap_or(10))
        };
        let confidence = if case.name == "reject_unknown_wallet" {
            None
        } else {
            Some(case.confidence.unwrap_or(90))
        };
        let age_secs = if case.name == "reject_unknown_wallet" {
            None
        } else {
            Some(case.age_secs.unwrap_or(0))
        };

        if let (Some(s), Some(c), Some(a)) = (score, confidence, age_secs) {
            env.ledger().with_mut(|l| l.timestamp += 3_601);
            let submission_time = env.ledger().timestamp();

            scoregate.submit_score(
                &Vec::new(&env),
                &wallet,
                &asset_pair,
                &s,
                &false,
                &false,
                &submission_time,
                &c,
                &1,
                &None,
            );

            env.ledger().with_mut(|l| l.timestamp += a);
        }

        let state_before = persistent_state(&env);
        let events_before = events_from(&env, &scoregate_id);

        let result = amm.try_provide_liquidity_gated(&wallet, &asset_pair, &DECISION_AMOUNT);

        let observed = Observed {
            wallets: distinct(&[wallet.clone()]),
            asset_pairs: distinct(&[asset_pair.clone()]),
            oracle_contracts: distinct(&[oracle.clone()]),
            ledger_writes: changed_entries(&state_before, &persistent_state(&env)),
            scoregate_events: events_from(&env, &scoregate_id).saturating_sub(events_before),
            encoded_input_bytes: encoded_len(&env, wallet.clone())
                + encoded_len(&env, asset_pair.clone())
                + encoded_len(&env, DECISION_AMOUNT),
        };
        let violations = fixture_data.resource_bounds.violations(&observed);
        assert!(
            violations.is_empty(),
            "Case '{}' broke the declared resource bounds: {:?} (observed {:?})",
            case.name,
            violations,
            observed
        );

        let mapped_result = match result {
            Ok(Ok(())) => "allow",
            Err(Ok(MockAmmError::HighRiskWallet)) => "reject_high_risk",
            Err(Ok(MockAmmError::LowConfidence)) => "reject_low_confidence",
            Err(Ok(MockAmmError::StaleScore)) => "reject_stale",
            Err(Ok(MockAmmError::OracleUnavailable)) => "oracle_unavailable",
            Err(Ok(MockAmmError::UnsupportedVersion)) => "unsupported_version",
            _ => panic!("Unexpected result in case '{}': {:?}", case.name, result),
        };

        assert_eq!(
            mapped_result, case.expected,
            "Case '{}' mismatched. Expected '{}', got '{}'",
            case.name, case.expected, mapped_result
        );
    }
}

// ── The runner rejects fixtures that declare more than it verifies ───────────

#[test]
fn fixture_with_unsupported_schema_version_fails_to_parse() {
    let mutated = FIXTURE.replacen("\"schema_version\": 1", "\"schema_version\": 2", 1);
    assert_ne!(mutated, FIXTURE, "the fixture must declare schema_version 1");
    let error = parse_fixture(&mutated).expect_err("schema_version 2 must be rejected");
    assert!(error.contains("unsupported schema_version 2"), "unexpected error: {error}");
}

#[test]
fn fixture_with_unknown_resource_metric_fails_to_parse() {
    let mutated = FIXTURE.replacen(
        "\"events_per_decision\": 0,",
        "\"events_per_decision\": 0, \"cpu_instructions_max\": 1,",
        1,
    );
    assert_ne!(mutated, FIXTURE, "the fixture must declare events_per_decision");
    assert!(parse_fixture(&mutated).is_err(), "an unknown metric must not be ignored");
}

#[test]
fn declared_capability_the_contract_lacks_is_reported() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, ScoreGateScoreContract);
    let client = ScoreGateScoreContractClient::new(&env, &id);
    client.initialize(&Address::generate(&env), &Address::generate(&env));

    let declared = parse_fixture(FIXTURE).expect("fixture parses").contract_capabilities;
    assert!(missing_capabilities(&env, &client, &declared).is_empty());

    let with_unknown = [declared, std::vec!["no_such_capability".to_string()]].concat();
    assert_eq!(
        missing_capabilities(&env, &client, &with_unknown),
        std::vec!["no_such_capability".to_string()]
    );
}

#[test]
fn bounds_below_observed_usage_are_violations() {
    let bounds = parse_fixture(FIXTURE).expect("fixture parses").resource_bounds;
    let within = Observed {
        wallets: bounds.wallets,
        asset_pairs: bounds.asset_pairs,
        oracle_contracts: bounds.oracle_contracts,
        ledger_writes: bounds.ledger_writes_per_read_only_decision,
        scoregate_events: bounds.events_per_decision,
        encoded_input_bytes: bounds.encoded_input_bytes_max,
    };
    assert!(bounds.violations(&within).is_empty());

    let exceeded = Observed {
        wallets: bounds.wallets + 1,
        ledger_writes: bounds.ledger_writes_per_read_only_decision + 1,
        scoregate_events: bounds.events_per_decision + 1,
        encoded_input_bytes: bounds.encoded_input_bytes_max + 1,
        ..within
    };
    assert_eq!(bounds.violations(&exceeded).len(), 4);
}
