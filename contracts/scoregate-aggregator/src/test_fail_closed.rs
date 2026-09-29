//! Tests for the aggregator fixes in #59, #60 and #61.
//!
//! - **#59** — `query_risk_gate` failed open when every registered shard was
//!   unhealthy: the loop skipped them all, never ran a check, and fell out
//!   returning `true`.
//! - **#60** — read-only queries wrote `LastShardFailure` to instance storage,
//!   which a simulated or view call must not do.
//! - **#61** — `get_decay_rate` and `get_consensus_threshold_k` hardcoded
//!   `shards.get(0)`, so taking the primary offline broke them even when every
//!   other shard was healthy.

use crate::{ScoreGateAggregator, ScoreGateAggregatorClient};
use scoregate_score::{ScoreGateScoreContract, ScoreGateScoreContractClient};
use scoregate_test_support::{generate_score_roles, test_env};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

/// Registers and initializes a real score shard.
fn setup_score_shard<'a>(env: &Env) -> (Address, ScoreGateScoreContractClient<'a>) {
    let id = env.register_contract(None, ScoreGateScoreContract);
    let client = ScoreGateScoreContractClient::new(env, &id);
    let (admin, service) = generate_score_roles(env);
    client.initialize(&admin, &service);
    (id, client)
}

/// A registered, initialized score shard.
fn shard(env: &Env) -> Address {
    setup_score_shard(env).0
}

/// A shard that also holds a low score for `wallet`, so the aggregator has
/// something to answer a gate query with.
fn shard_with_low_score(env: &Env, wallet: &Address) -> Address {
    let (address, client) = setup_score_shard(env);
    client.submit_score(
        &soroban_sdk::Vec::new(env),
        wallet,
        &symbol_short!("XLM_USDC"),
        &10, // comfortably under an 80 threshold
        &false,
        &false,
        &1,
        &80,
        &1,
        &None,
    );
    address
}

fn aggregator_with_shards<'a>(env: &Env, count: usize) -> ScoreGateAggregatorClient<'a> {
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(env, &agg_id);
    client.initialize(&Address::generate(env));
    for _ in 0..count {
        client.add_shard(&shard(env));
    }
    client
}

/// Same, but every shard holds a low score for `wallet`.
fn aggregator_with_scored_shards<'a>(
    env: &Env,
    wallet: &Address,
    count: usize,
) -> ScoreGateAggregatorClient<'a> {
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(env, &agg_id);
    client.initialize(&Address::generate(env));
    for _ in 0..count {
        client.add_shard(&shard_with_low_score(env, wallet));
    }
    client
}

fn mark_unhealthy(env: &Env, client: &ScoreGateAggregatorClient, shard: &Address) {
    env.mock_all_auths();
    client.set_shard_health(shard, &false);
    env.mock_all_auths();
}

// ── #59: the risk gate must fail closed ──────────────────────────────────────

/// #59: with every shard marked unhealthy the gate returned `true`, so a total
/// shard outage opened it. It must fail closed.
#[test]
fn query_risk_gate_fails_closed_when_all_shards_unhealthy() {
    let env = test_env();
    let client = aggregator_with_shards(&env, 2);

    let shards = client.get_shards();
    for i in 0..shards.len() {
        let s = shards.get(i).unwrap();
        mark_unhealthy(&env, &client, &s);
    }

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // No shard could vouch for this wallet, so the gate must not pass.
    assert!(!client.query_risk_gate(&wallet, &pair, &80));
}

/// #59: with one healthy shard the gate still works, so failing closed did not
/// turn every query into a denial.
#[test]
fn query_risk_gate_still_passes_with_one_healthy_shard() {
    let env = test_env();
    let wallet = Address::generate(&env);
    let client = aggregator_with_scored_shards(&env, &wallet, 2);

    // Only the first shard is taken out.
    let shards = client.get_shards();
    mark_unhealthy(&env, &client, &shards.get(0).unwrap());

    // The remaining shard holds a low score, so it answers and the gate passes
    // on that answer.
    assert!(client.query_risk_gate(&wallet, &symbol_short!("XLM_USDC"), &80));
}

/// #59: an empty shard list already failed closed; keep it that way.
#[test]
fn query_risk_gate_fails_closed_with_no_shards() {
    let env = test_env();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    client.initialize(&Address::generate(&env));

    let wallet = Address::generate(&env);
    assert!(!client.query_risk_gate(&wallet, &symbol_short!("XLM_USDC"), &80));
}

// ── #60: read queries must not write storage ────────────────────────────────

/// #60: a failing read query recorded `LastShardFailure` in instance storage.
/// A simulated or view call must be side-effect free, so it is now an event.
#[test]
fn read_queries_do_not_write_last_shard_failure() {
    let env = test_env();
    let client = aggregator_with_shards(&env, 1);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // A wallet with no score makes each read take its failure path.
    let _ = client.query_risk_gate(&wallet, &pair, &80);
    let _ = client.try_get_score(&wallet, &pair);
    let _ = client.try_get_aggregate_score(&wallet);

    // Nothing should have been recorded.
    assert_eq!(client.get_last_shard_failure(), None);
}

/// #60: reads that succeed also write nothing.
#[test]
fn healthy_read_queries_also_write_nothing() {
    let env = test_env();
    let client = aggregator_with_shards(&env, 1);

    let wallet = Address::generate(&env);
    let _ = client.try_get_score(&wallet, &symbol_short!("XLM_USDC"));
    assert_eq!(client.get_last_shard_failure(), None);
}

// ── #61: config getters must fall back past an unhealthy primary ────────────

/// #61: with shard 0 unhealthy, `get_decay_rate` used to return
/// `ScoreNotFound` even though the other shard was healthy.
#[test]
fn get_decay_rate_falls_back_to_a_healthy_shard() {
    let env = test_env();
    let client = aggregator_with_shards(&env, 2);

    let shards = client.get_shards();
    mark_unhealthy(&env, &client, &shards.get(0).unwrap());

    // Resolves from the remaining healthy shard instead of erroring.
    let _rate = client.get_decay_rate();
}

/// #61: same fallback for `get_consensus_threshold_k`.
#[test]
fn get_consensus_threshold_falls_back_to_a_healthy_shard() {
    let env = test_env();
    let client = aggregator_with_shards(&env, 2);

    let shards = client.get_shards();
    mark_unhealthy(&env, &client, &shards.get(0).unwrap());

    let _k = client.get_consensus_threshold_k();
}

/// #61: with every shard unhealthy there is nothing to read from, so the getter
/// must still report that rather than silently answering.
#[test]
fn get_decay_rate_errors_when_all_shards_unhealthy() {
    let env = test_env();
    let client = aggregator_with_shards(&env, 2);

    let shards = client.get_shards();
    for i in 0..shards.len() {
        let s = shards.get(i).unwrap();
        mark_unhealthy(&env, &client, &s);
    }

    assert!(client.try_get_decay_rate().is_err());
}
