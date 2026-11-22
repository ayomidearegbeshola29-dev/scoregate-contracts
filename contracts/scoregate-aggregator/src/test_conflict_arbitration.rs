use crate::{ScoreGateAggregator, ScoreGateAggregatorClient};
use scoregate_score::{ScoreGateScoreContract, ScoreGateScoreContractClient};
use scoregate_test_support::generate_score_roles;
use soroban_sdk::{testutils::{Address as _, Ledger as _}, Address, Env, Symbol, Vec};

fn setup_shard(env: &Env) -> (Address, ScoreGateScoreContractClient<'_>) {
    let shard = env.register_contract(None, ScoreGateScoreContract);
    let client = ScoreGateScoreContractClient::new(env, &shard);
    let (admin, service) = generate_score_roles(env);
    client.initialize(&admin, &service);
    (shard, client)
}

fn setup_aggregator(env: &Env) -> ScoreGateAggregatorClient<'_> {
    let aggregator = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(env, &aggregator);
    client.initialize(&Address::generate(env));
    client
}

fn submit_score(
    env: &Env,
    shard: &ScoreGateScoreContractClient<'_>,
    wallet: &Address,
    pair: &Symbol,
    score: u32,
    timestamp: u64,
) {
    shard.submit_score(
        &Vec::new(env),
        wallet,
        pair,
        &score,
        &false,
        &false,
        &timestamp,
        &90,
        &1,
        &None,
    );
}

#[test]
fn get_score_arbitrates_real_shard_contracts() {
    let env = Env::default();
    env.mock_all_auths();
    let aggregator = setup_aggregator(&env);
    let (first_id, first) = setup_shard(&env);
    let (second_id, second) = setup_shard(&env);
    aggregator.add_shard(&first_id);
    aggregator.add_shard(&second_id);

    let wallet = Address::generate(&env);
    let pair = Symbol::new(&env, "XLM_USDC");
    submit_score(&env, &first, &wallet, &pair, 75, 1_000);
    submit_score(&env, &second, &wallet, &pair, 75, 1_000);

    let selected = aggregator.get_score(&wallet, &pair);
    assert_eq!(selected.score, 75);
    assert_eq!(selected.timestamp, 1_000);
}

#[test]
fn get_score_ignores_stale_high_score_from_real_shard() {
    let env = Env::default();
    env.mock_all_auths();
    let aggregator = setup_aggregator(&env);
    let (fresh_id, fresh) = setup_shard(&env);
    let (stale_id, stale) = setup_shard(&env);
    aggregator.add_shard(&fresh_id);
    aggregator.add_shard(&stale_id);

    let wallet = Address::generate(&env);
    let pair = Symbol::new(&env, "XLM_USDC");
    submit_score(&env, &fresh, &wallet, &pair, 25, 900_000);
    submit_score(&env, &stale, &wallet, &pair, 95, 1);
    env.ledger().with_mut(|ledger| ledger.timestamp = 1_000_000);

    let selected = aggregator.get_score(&wallet, &pair);
    assert_eq!(selected.score, 25);
    assert_eq!(selected.timestamp, 900_000);
}

#[test]
fn get_aggregate_score_equal_timestamp_prefers_higher_risk_regardless_of_order() {
    let env = Env::default();
    env.mock_all_auths();
    let wallet = Address::generate(&env);
    let pair = Symbol::new(&env, "XLM_USDC");

    // Order A: low-risk shard registered first.
    let aggregator_a = setup_aggregator(&env);
    let (low_id, low) = setup_shard(&env);
    let (high_id, high) = setup_shard(&env);
    aggregator_a.add_shard(&low_id);
    aggregator_a.add_shard(&high_id);
    submit_score(&env, &low, &wallet, &pair, 20, 5_000);
    submit_score(&env, &high, &wallet, &pair, 90, 5_000);

    let selected_a = aggregator_a.get_aggregate_score(&wallet, &pair).unwrap();
    assert_eq!(selected_a.score, 90);
    assert_eq!(selected_a.timestamp, 5_000);

    // Order B: high-risk shard registered first.
    let aggregator_b = setup_aggregator(&env);
    let (high_id_b, high_b) = setup_shard(&env);
    let (low_id_b, low_b) = setup_shard(&env);
    aggregator_b.add_shard(&high_id_b);
    aggregator_b.add_shard(&low_id_b);
    submit_score(&env, &high_b, &wallet, &pair, 90, 5_000);
    submit_score(&env, &low_b, &wallet, &pair, 20, 5_000);

    let selected_b = aggregator_b.get_aggregate_score(&wallet, &pair).unwrap();
    assert_eq!(selected_b.score, 90);
    assert_eq!(selected_b.timestamp, 5_000);
}

#[test]
fn get_aggregate_score_equal_timestamp_and_score_is_order_independent() {
    let env = Env::default();
    env.mock_all_auths();
    let wallet = Address::generate(&env);
    let pair = Symbol::new(&env, "XLM_USDC");

    let aggregator = setup_aggregator(&env);
    let (first_id, first) = setup_shard(&env);
    let (second_id, second) = setup_shard(&env);
    aggregator.add_shard(&first_id);
    aggregator.add_shard(&second_id);
    submit_score(&env, &first, &wallet, &pair, 60, 7_000);
    submit_score(&env, &second, &wallet, &pair, 60, 7_000);

    let selected = aggregator.get_aggregate_score(&wallet, &pair).unwrap();
    assert_eq!(selected.score, 60);
    assert_eq!(selected.timestamp, 7_000);
}

#[test]
fn get_aggregate_score_fresher_lower_score_wins() {
    let env = Env::default();
    env.mock_all_auths();
    let wallet = Address::generate(&env);
    let pair = Symbol::new(&env, "XLM_USDC");

    let aggregator = setup_aggregator(&env);
    let (fresh_id, fresh) = setup_shard(&env);
    let (stale_id, stale) = setup_shard(&env);
    aggregator.add_shard(&fresh_id);
    aggregator.add_shard(&stale_id);
    submit_score(&env, &fresh, &wallet, &pair, 25, 900_000);
    submit_score(&env, &stale, &wallet, &pair, 95, 1);

    let selected = aggregator.get_aggregate_score(&wallet, &pair).unwrap();
    assert_eq!(selected.score, 25);
    assert_eq!(selected.timestamp, 900_000);
}
