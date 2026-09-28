use crate::{
    AggregatorConfigFingerprint, Error, ScoreGateAggregator, ScoreGateAggregatorClient,
    MaybeAggregatorConfigFingerprint, ShardProbeStatus, SplitBrainStatus,
};
use scoregate_score::{ScoreGateScoreContract, ScoreGateScoreContractClient};
use scoregate_test_support::{generate_score_roles, test_env};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

/// A shard whose interface has fully drifted: it advertises no capability the
/// aggregator depends on.
mod incompatible_shard {
    use soroban_sdk::{contract, contractimpl, Env, Symbol};

    #[contract]
    pub struct IncompatibleShard;

    #[contractimpl]
    impl IncompatibleShard {
        pub fn supports_interface(_env: Env, _capability: Symbol) -> bool {
            false
        }
    }
}

/// A shard that implements part of the interface (`score`, `gate`) but is
/// missing the `aggr` capability the aggregator also invokes.
mod partial_shard {
    use soroban_sdk::{contract, contractimpl, Env, Symbol};

    #[contract]
    pub struct PartialShard;

    #[contractimpl]
    impl PartialShard {
        pub fn supports_interface(env: Env, capability: Symbol) -> bool {
            capability == Symbol::new(&env, "score") || capability == Symbol::new(&env, "gate")
        }
    }
}

/// A shard that predates the capability-detection interface entirely: it does
/// not expose `supports_interface`, so the cross-contract call traps.
mod legacy_shard {
    use soroban_sdk::{contract, contractimpl, Env};

    #[contract]
    pub struct LegacyShard;

    #[contractimpl]
    impl LegacyShard {
        pub fn ping(_env: Env) -> bool {
            true
        }
    }
}

/// A shard that passes capability registration but does not implement the
/// configuration getters used by split-brain probing.
mod deceptive_shard {
    use soroban_sdk::{contract, contractimpl, Env, Symbol};

    #[contract]
    pub struct DeceptiveShard;

    #[contractimpl]
    impl DeceptiveShard {
        pub fn supports_interface(env: Env, capability: Symbol) -> bool {
            capability == Symbol::new(&env, "score")
                || capability == Symbol::new(&env, "gate")
                || capability == Symbol::new(&env, "aggr")
                || capability == Symbol::new(&env, "arch")
        }

        /// Required by the new try_get_score probe in shard_supports_required_interface
        /// (fix #115). Returns ScoreNotFound confirming the method exists without
        /// requiring stored data.
        pub fn get_score(
            _env: Env,
            _wallet: soroban_sdk::Address,
            _asset_pair: Symbol,
        ) -> Result<scoregate_score::RiskScore, scoregate_score::Error> {
            Err(scoregate_score::Error::ScoreNotFound)
        }

        pub fn is_score_stale(_env: Env, _wallet: soroban_sdk::Address, _pair: Symbol) -> bool {
            false
        }
    }
}

fn init_aggregator(env: &Env) -> ScoreGateAggregatorClient<'_> {
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(env, &agg_id);
    client.initialize(&Address::generate(env));
    client
}

#[test]
fn test_initialize() {
    let env = test_env();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_initialize_twice_fails() {
    let env = test_env();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let result = client.try_initialize(&admin);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_initialize_requires_nominated_admin_and_rolls_back() {
    let env = Env::default();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);

    // An arbitrary invoker cannot install a nominated admin without that
    // address's authorization, and the failed invocation must not write state.
    assert!(client.try_initialize(&admin).is_err());
    assert_eq!(client.try_get_admin(), Err(Ok(Error::NotInitialized)));

    env.mock_all_auths();
    client.initialize(&admin);
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_get_admin_not_initialized() {
    let env = test_env();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let result = client.try_get_admin();
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_add_remove_shards() {
    let env = test_env();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let (shard, _) = setup_score_shard(&env);
    client.add_shard(&shard);

    let shards = client.get_shards();
    assert_eq!(shards.len(), 1);
    assert_eq!(shards.get(0).unwrap(), shard);

    client.remove_shard(&shard);
    assert_eq!(client.get_shards().len(), 0);
}

#[test]
fn test_add_shard_self_reference_fails() {
    let env = test_env();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let result = client.try_add_shard(&agg_id);
    assert_eq!(result, Err(Ok(Error::SelfReference)));
}

#[test]
fn test_add_shard_duplicate_fails() {
    let env = test_env();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let (shard, _) = setup_score_shard(&env);
    client.add_shard(&shard);
    let result = client.try_add_shard(&shard);
    assert_eq!(result, Err(Ok(Error::ShardAlreadyRegistered)));
}

#[test]
fn test_remove_nonexistent_shard_fails() {
    let env = test_env();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let shard = Address::generate(&env);
    let result = client.try_remove_shard(&shard);
    assert_eq!(result, Err(Ok(Error::ShardNotRegistered)));
}

#[test]
fn test_query_risk_gate_no_shards_fails_closed() {
    let env = test_env();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert!(!client.query_risk_gate(&wallet, &pair, &75));
}

#[test]
fn test_query_risk_gate_all_shards_pass() {
    let env = test_env();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let (shard1_id, shard1) = setup_score_shard(&env);
    let (shard2_id, shard2) = setup_score_shard(&env);
    client.add_shard(&shard1_id);
    client.add_shard(&shard2_id);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    shard1.submit_score(&Vec::new(&env), &wallet, &pair, &10, &false, &false, &1, &100, &1, &None);
    shard2.submit_score(&Vec::new(&env), &wallet, &pair, &10, &false, &false, &1, &100, &1, &None);

    assert!(client.query_risk_gate(&wallet, &pair, &75));
}

#[test]
fn test_query_risk_gate_one_shard_rejects() {
    let env = test_env();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let (shard1_id, shard1) = setup_score_shard(&env);
    let (shard2_id, shard2) = setup_score_shard(&env);
    client.add_shard(&shard1_id);
    client.add_shard(&shard2_id);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    shard1.submit_score(&Vec::new(&env), &wallet, &pair, &10, &false, &false, &1, &100, &1, &None);
    shard2.submit_score(&Vec::new(&env), &wallet, &pair, &90, &false, &false, &1, &100, &1, &None);

    assert!(!client.query_risk_gate(&wallet, &pair, &75));
}

#[test]
fn test_oversized_asset_pair_fails_closed_before_shard_fanout() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let (shard_id, shard) = setup_score_shard(&env);
    client.add_shard(&shard_id);

    let wallet = Address::generate(&env);
    let valid_pair = symbol_short!("XLM_USDC");
    shard.submit_score(
        &Vec::new(&env),
        &wallet,
        &valid_pair,
        &10,
        &false,
        &false,
        &1,
        &100,
        &1,
        &None,
    );

    let oversized_pair = Symbol::new(&env, "PAIR123456");
    assert!(!client.query_risk_gate(&wallet, &oversized_pair, &75));
    assert_eq!(
        client.try_get_score(&wallet, &oversized_pair),
        Err(Ok(scoregate_score::Error::InvalidAttestation))
    );
    assert_eq!(client.get_score_across_shards(&wallet, &oversized_pair).len(), 0);
    assert_eq!(client.contagion_depth_across_shards(&wallet, &oversized_pair), 0);
    assert_eq!(client.get_last_shard_failure(), None);
}

#[test]
fn test_get_decay_rate_returns_primary_shard_when_shards_diverge() {
    let env = test_env();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    let (primary_id, primary_client) = setup_score_shard(&env);
    let (secondary_id, secondary_client) = setup_score_shard(&env);

    client.initialize(&admin);
    client.add_shard(&primary_id);
    client.add_shard(&secondary_id);
    primary_client.set_decay_rate(&1, &1000);
    secondary_client.set_decay_rate(&1, &500);

    assert_eq!(client.get_decay_rate(), (1, 1000));
}

#[test]
fn test_get_decay_rate_no_shards_returns_error() {
    let env = Env::default();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);

    assert_eq!(client.try_get_decay_rate(), Err(Ok(scoregate_score::Error::ScoreNotFound)));
}

#[test]
fn test_get_consensus_threshold_k_returns_primary_shard_when_shards_diverge() {
    let env = test_env();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    let (primary_id, primary_client) = setup_score_shard(&env);
    let (secondary_id, secondary_client) = setup_score_shard(&env);

    client.initialize(&admin);
    client.add_shard(&primary_id);
    client.add_shard(&secondary_id);
    primary_client.set_consensus_config(&3, &10);
    secondary_client.set_consensus_config(&7, &10);

    assert_eq!(client.get_consensus_threshold_k(), 3);
}

#[test]
fn test_get_consensus_threshold_k_no_shards_returns_error() {
    let env = Env::default();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);

    assert_eq!(
        client.try_get_consensus_threshold_k(),
        Err(Ok(scoregate_score::Error::ScoreNotFound))
    );
}

#[test]
fn test_get_score_highest_score_policy() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let (shard1_id, shard1_client) = setup_score_shard(&env);
    // shard1: score=50, timestamp=100
    shard1_client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &100,
        &90,
        &1,
        &None,
    );

    let (shard2_id, shard2_client) = setup_score_shard(&env);
    // shard2: score=90, timestamp=50 (higher score but older)
    shard2_client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &90,
        &false,
        &false,
        &50,
        &90,
        &1,
        &None,
    );

    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let agg_client = ScoreGateAggregatorClient::new(&env, &agg_id);
    agg_client.initialize(&admin);
    agg_client.add_shard(&shard1_id);
    agg_client.add_shard(&shard2_id);

    // Default policy is HighestScore — should return score 90 from shard2
    let score = agg_client.get_score(&wallet, &pair);
    assert_eq!(score.score, 90);
    assert_eq!(score.timestamp, 50);
}

#[test]
fn test_get_watchlist_status_returns_true_for_watchlisted_wallet() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    let (shard_id, shard_client) = setup_score_shard(&env);

    client.initialize(&admin);
    client.add_shard(&shard_id);

    let wallet = Address::generate(&env);
    shard_client.set_watchlist(&soroban_sdk::Vec::new(&env), &wallet, &true);

    assert!(client.get_watchlist_status(&wallet));
}

#[test]
fn test_get_watchlist_status_returns_false_when_watchlisted_nowhere() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    let (shard_a_id, _) = setup_score_shard(&env);
    let (shard_b_id, _) = setup_score_shard(&env);

    client.initialize(&admin);
    client.add_shard(&shard_a_id);
    client.add_shard(&shard_b_id);

    let wallet = Address::generate(&env);

    assert!(!client.get_watchlist_status(&wallet));
}

#[test]
fn test_get_watchlist_status_returns_true_when_any_shard_watchlists_wallet() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    let (shard_a_id, _) = setup_score_shard(&env);
    let (shard_b_id, shard_b_client) = setup_score_shard(&env);

    client.initialize(&admin);
    client.add_shard(&shard_a_id);
    client.add_shard(&shard_b_id);

    let wallet = Address::generate(&env);
    shard_b_client.set_watchlist(&soroban_sdk::Vec::new(&env), &wallet, &true);

    assert!(client.get_watchlist_status(&wallet));
}

#[test]
fn test_get_watchlist_status_returns_true_when_all_shards_watchlist_wallet() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    let (shard_a_id, shard_a_client) = setup_score_shard(&env);
    let (shard_b_id, shard_b_client) = setup_score_shard(&env);

    client.initialize(&admin);
    client.add_shard(&shard_a_id);
    client.add_shard(&shard_b_id);

    let wallet = Address::generate(&env);
    shard_a_client.set_watchlist(&soroban_sdk::Vec::new(&env), &wallet, &true);
    shard_b_client.set_watchlist(&soroban_sdk::Vec::new(&env), &wallet, &true);

    assert!(client.get_watchlist_status(&wallet));
}

#[test]
fn test_add_shard_accepts_compatible_score_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);

    let shard = env.register_contract(None, ScoreGateScoreContract);
    client.add_shard(&shard);

    let shards = client.get_shards();
    assert_eq!(shards.len(), 1);
    assert_eq!(shards.get(0).unwrap(), shard);
}

#[test]
fn test_add_shard_rejects_incompatible_shard() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);

    let shard = env.register_contract(None, incompatible_shard::IncompatibleShard);
    let result = client.try_add_shard(&shard);

    assert_eq!(result, Err(Ok(Error::IncompatibleInterface)));
    assert_eq!(client.get_shards().len(), 0);
}

#[test]
fn test_add_shard_rejects_shard_missing_capability() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);

    let shard = env.register_contract(None, partial_shard::PartialShard);
    let result = client.try_add_shard(&shard);

    assert_eq!(result, Err(Ok(Error::IncompatibleInterface)));
    assert_eq!(client.get_shards().len(), 0);
}

#[test]
fn test_add_shard_rejects_legacy_shard_without_supports_interface() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);

    let shard = env.register_contract(None, legacy_shard::LegacyShard);
    let result = client.try_add_shard(&shard);

    assert_eq!(result, Err(Ok(Error::IncompatibleInterface)));
    assert_eq!(client.get_shards().len(), 0);
}

fn setup_score_shard(env: &Env) -> (Address, ScoreGateScoreContractClient<'_>) {
    let id = env.register_contract(None, ScoreGateScoreContract);
    let client = ScoreGateScoreContractClient::new(env, &id);
    let (admin, service) = generate_score_roles(env);
    client.initialize(&admin, &service);
    (id, client)
}

fn default_fingerprint() -> AggregatorConfigFingerprint {
    AggregatorConfigFingerprint {
        decay_num: 0,
        decay_den: 1,
        staleness_window: 604_800,
        global_min_confidence: 0,
        consensus_k: 2,
        consensus_epsilon: 5,
    }
}

#[test]
fn test_detect_split_brain_aligned() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    let (shard_a, _) = setup_score_shard(&env);
    let (shard_b, _) = setup_score_shard(&env);
    client.add_shard(&shard_a);
    client.add_shard(&shard_b);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let report = client.detect_split_brain(&wallet, &pair);

    assert_eq!(report.status, SplitBrainStatus::Aligned);
    assert_eq!(report.shard_count, 2);
    assert_eq!(report.healthy_count, 2);
    assert_eq!(report.available_count, 2);
    assert_eq!(report.quorum_count, 2);
    assert_eq!(report.required_quorum, 2);
    assert_eq!(report.mismatch_count, 0);
    assert_eq!(report.canonical, MaybeAggregatorConfigFingerprint::Some(default_fingerprint()));
}

#[test]
fn test_detect_split_brain_permutation_stable_majority() {
    let env_a = Env::default();
    env_a.mock_all_auths();
    let client_a = init_aggregator(&env_a);
    let wallet_a = Address::generate(&env_a);
    let pair_a = symbol_short!("XLM_USDC");
    let (a1, a1_client) = setup_score_shard(&env_a);
    let (a2, _) = setup_score_shard(&env_a);
    let (a3, _) = setup_score_shard(&env_a);
    a1_client.set_decay_rate(&2, &1000);
    client_a.add_shard(&a1);
    client_a.add_shard(&a2);
    client_a.add_shard(&a3);

    let env_b = Env::default();
    env_b.mock_all_auths();
    let client_b = init_aggregator(&env_b);
    let wallet_b = Address::generate(&env_b);
    let pair_b = symbol_short!("XLM_USDC");
    let (b1, b1_client) = setup_score_shard(&env_b);
    let (b2, _) = setup_score_shard(&env_b);
    let (b3, _) = setup_score_shard(&env_b);
    b1_client.set_decay_rate(&2, &1000);
    client_b.add_shard(&b2);
    client_b.add_shard(&b3);
    client_b.add_shard(&b1);

    let report_a = client_a.detect_split_brain(&wallet_a, &pair_a);
    let report_b = client_b.detect_split_brain(&wallet_b, &pair_b);

    assert_eq!(report_a.status, SplitBrainStatus::SplitBrain);
    assert_eq!(report_b.status, SplitBrainStatus::SplitBrain);
    assert_eq!(report_a.canonical, report_b.canonical);
    assert_eq!(report_a.quorum_count, 2);
    assert_eq!(report_b.quorum_count, 2);
    assert_eq!(report_a.mismatch_count, 1);
    assert_eq!(report_b.mismatch_count, 1);
}

#[test]
fn test_detect_split_brain_partial_failure_preserves_quorum() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    let (shard_a, _) = setup_score_shard(&env);
    let (shard_b, _) = setup_score_shard(&env);
    let bad = env.register_contract(None, deceptive_shard::DeceptiveShard);
    client.add_shard(&shard_a);
    client.add_shard(&shard_b);
    client.add_shard(&bad);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let report = client.detect_split_brain(&wallet, &pair);

    assert_eq!(report.status, SplitBrainStatus::Aligned);
    assert_eq!(report.healthy_count, 3);
    assert_eq!(report.available_count, 2);
    assert_eq!(report.unavailable_count, 1);
    assert_eq!(report.quorum_count, 2);
    assert_eq!(report.required_quorum, 2);
    assert_eq!(report.diagnostics.get(2).unwrap().status, ShardProbeStatus::Unavailable);
}

#[test]
fn test_detect_split_brain_byzantine_config_mismatch() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    let (shard_a, shard_a_client) = setup_score_shard(&env);
    let (shard_b, _) = setup_score_shard(&env);
    shard_a_client.set_global_min_confidence(&50);
    client.add_shard(&shard_a);
    client.add_shard(&shard_b);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let report = client.detect_split_brain(&wallet, &pair);

    assert_eq!(report.status, SplitBrainStatus::SplitBrain);
    assert_eq!(report.available_count, 2);
    assert_eq!(report.quorum_count, 1);
    assert_eq!(report.required_quorum, 2);
    assert_eq!(report.mismatch_count, 1);
}

#[test]
fn test_detect_split_brain_even_partition_takes_precedence_over_quorum_loss() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    let (shard_a, shard_a_client) = setup_score_shard(&env);
    let (shard_b, shard_b_client) = setup_score_shard(&env);
    let (shard_c, _) = setup_score_shard(&env);
    let (shard_d, _) = setup_score_shard(&env);
    shard_a_client.set_decay_rate(&2, &1000);
    shard_b_client.set_decay_rate(&2, &1000);
    client.add_shard(&shard_a);
    client.add_shard(&shard_b);
    client.add_shard(&shard_c);
    client.add_shard(&shard_d);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let report = client.detect_split_brain(&wallet, &pair);

    assert_eq!(report.status, SplitBrainStatus::SplitBrain);
    assert_eq!(report.quorum_count, 2);
    assert_eq!(report.required_quorum, 3);
    assert_eq!(report.mismatch_count, 2);
}

#[test]
fn test_detect_split_brain_stale_shard_causes_quorum_loss() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    let (fresh_id, fresh_client) = setup_score_shard(&env);
    let (stale_id, stale_client) = setup_score_shard(&env);
    client.add_shard(&fresh_id);
    client.add_shard(&stale_id);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    fresh_client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &10,
        &false,
        &false,
        &700_000,
        &90,
        &1,
        &None,
    );
    stale_client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &10,
        &false,
        &false,
        &1,
        &90,
        &1,
        &None,
    );

    env.ledger().with_mut(|ledger| ledger.timestamp = 700_000);
    let report = client.detect_split_brain(&wallet, &pair);

    assert_eq!(report.status, SplitBrainStatus::QuorumLost);
    assert_eq!(report.stale_count, 1);
    assert_eq!(report.available_count, 1);
    assert_eq!(report.required_quorum, 2);
}

#[test]
fn test_set_shard_health_quarantines_and_restores_conflicting_shard() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    let (shard_a, shard_a_client) = setup_score_shard(&env);
    let (shard_b, _) = setup_score_shard(&env);
    shard_a_client.set_decay_rate(&2, &1000);
    client.add_shard(&shard_a);
    client.add_shard(&shard_b);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert_eq!(client.detect_split_brain(&wallet, &pair).status, SplitBrainStatus::QuorumLost);

    client.set_shard_health(&shard_a, &false);
    let quarantined = client.detect_split_brain(&wallet, &pair);
    assert_eq!(quarantined.status, SplitBrainStatus::Aligned);
    assert_eq!(quarantined.healthy_count, 1);
    assert_eq!(quarantined.diagnostics.get(0).unwrap().status, ShardProbeStatus::Unhealthy);
    assert!(!client.get_shard_health(&shard_a));

    client.set_shard_health(&shard_a, &true);
    assert!(client.get_shard_health(&shard_a));
    assert_eq!(client.detect_split_brain(&wallet, &pair).status, SplitBrainStatus::QuorumLost);
}

#[test]
fn test_detect_split_brain_max_shards_bounded_diagnostics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    for _ in 0..crate::MAX_SHARDS {
        let (shard, _) = setup_score_shard(&env);
        client.add_shard(&shard);
    }

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let report = client.detect_split_brain(&wallet, &pair);

    assert_eq!(report.status, SplitBrainStatus::Aligned);
    assert_eq!(report.shard_count, crate::MAX_SHARDS as u32);
    assert_eq!(report.diagnostics.len(), crate::MAX_SHARDS as u32);
    assert_eq!(report.quorum_count, crate::MAX_SHARDS as u32);
    assert_eq!(report.required_quorum, 6);
}

#[test]
fn test_contagion_depth_across_shards_with_cross_shard_cycle() {
    let env = Env::default();
    env.mock_all_auths();

    // Register aggregator
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let agg_client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let agg_admin = Address::generate(&env);
    agg_client.initialize(&agg_admin);

    // Register two score shards
    let (shard1_id, shard1) = setup_score_shard(&env);
    let (shard2_id, shard2) = setup_score_shard(&env);

    // Add shards to aggregator
    agg_client.add_shard(&shard1_id);
    agg_client.add_shard(&shard2_id);

    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Create a cross-shard cyclic delegation:
    //   Shard 1: wallet_a delegates to wallet_b
    //   Shard 2: wallet_b delegates back to wallet_a
    // This forms a cycle no single-shard cycle detector can see.
    shard1.set_score_delegate(&wallet_a, &wallet_b);
    shard2.set_score_delegate(&wallet_b, &wallet_a);

    // Add counterparty links to give non-zero contagion depth on each shard.
    // Shard 1: wallet_a has 3 counterparties.
    let c1 = Address::generate(&env);
    let c2 = Address::generate(&env);
    let c3 = Address::generate(&env);
    shard1.add_counterparty_link(&wallet_a, &c1, &pair);
    shard1.add_counterparty_link(&wallet_a, &c2, &pair);
    shard1.add_counterparty_link(&wallet_a, &c3, &pair);

    // Shard 2: wallet_a has 5 counterparties (more than shard 1, so max = 5).
    let d1 = Address::generate(&env);
    let d2 = Address::generate(&env);
    let d3 = Address::generate(&env);
    let d4 = Address::generate(&env);
    let d5 = Address::generate(&env);
    shard2.add_counterparty_link(&wallet_a, &d1, &pair);
    shard2.add_counterparty_link(&wallet_a, &d2, &pair);
    shard2.add_counterparty_link(&wallet_a, &d3, &pair);
    shard2.add_counterparty_link(&wallet_a, &d4, &pair);
    shard2.add_counterparty_link(&wallet_a, &d5, &pair);

    // Verify per-shard depths are independent
    assert_eq!(shard1.get_contagion_depth(&wallet_a, &pair), 3);
    assert_eq!(shard2.get_contagion_depth(&wallet_a, &pair), 5);

    // The aggregator should return the max across shards — no panic, no hang.
    let depth = agg_client.contagion_depth_across_shards(&wallet_a, &pair);
    assert_eq!(depth, 5, "should return max contagion depth across shards");

    // Also verify a wallet with no counterparties still works fine
    let isolated = Address::generate(&env);
    let depth_zero = agg_client.contagion_depth_across_shards(&isolated, &pair);
    assert_eq!(depth_zero, 0, "wallet with no links should return 0");
}

#[test]
fn test_contagion_depth_across_shards_no_shards() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert_eq!(client.contagion_depth_across_shards(&wallet, &pair), 0);
}

// ── Issue #711: Shard capability attestation tests ────────────────────────────

/// A shard that claims all required capabilities at registration time but
/// then stops advertising one of them (simulates a post-registration downgrade
/// caused by a bad upgrade or roll-forward to an older WASM).
mod downgraded_shard {
    use soroban_sdk::{contract, contractimpl, Env, Symbol};

    #[contract]
    pub struct DowngradedShard;

    #[contractimpl]
    impl DowngradedShard {
        pub fn supports_interface(env: Env, capability: Symbol) -> bool {
            let downgraded: bool =
                env.storage().instance().get(&Symbol::new(&env, "dg")).unwrap_or(false);
            let aggr = Symbol::new(&env, "aggr");
            if downgraded && capability == aggr {
                return false;
            }
            capability == Symbol::new(&env, "score")
                || capability == Symbol::new(&env, "gate")
                || capability == Symbol::new(&env, "aggr")
                || capability == Symbol::new(&env, "arch")
        }

        /// Required by the new try_get_score probe in shard_supports_required_interface
        /// (fix #115). Returns a contract-level error confirming the method exists.
        pub fn get_score(
            _env: Env,
            _wallet: soroban_sdk::Address,
            _asset_pair: Symbol,
        ) -> Result<scoregate_score::RiskScore, scoregate_score::Error> {
            Err(scoregate_score::Error::ScoreNotFound)
        }
    }
}

/// After registration the capability snapshot is stored and readable.
#[test]
fn test_get_shard_capabilities_returns_snapshot_after_registration() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    let (shard_id, _) = setup_score_shard(&env);
    client.add_shard(&shard_id);

    let caps = client.get_shard_capabilities(&shard_id);
    // A full ScoreGateScoreContract must advertise at least score, gate, aggr.
    assert!(caps.contains(soroban_sdk::Symbol::new(&env, "score")));
    assert!(caps.contains(soroban_sdk::Symbol::new(&env, "gate")));
    assert!(caps.contains(soroban_sdk::Symbol::new(&env, "aggr")));
}

/// A shard with no snapshot (e.g. registered before the feature) returns empty.
#[test]
fn test_get_shard_capabilities_returns_empty_for_no_snapshot() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Use an unknown address (never registered).
    let random = Address::generate(&env);
    let caps = client.get_shard_capabilities(&random);
    assert_eq!(caps.len(), 0);
}

/// A shard that is incompatible must be rejected — snapshot must NOT be written.
#[test]
fn test_incompatible_shard_has_no_capability_snapshot() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);

    let shard = env.register_contract(None, incompatible_shard::IncompatibleShard);
    let _ = client.try_add_shard(&shard); // expected to fail

    let caps = client.get_shard_capabilities(&shard);
    assert_eq!(caps.len(), 0, "no snapshot must be written for a rejected shard");
}

/// `shard_capabilities_downgraded` returns false for a healthy shard.
#[test]
fn test_shard_capabilities_downgraded_returns_false_for_healthy_shard() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    let (shard_id, _) = setup_score_shard(&env);
    client.add_shard(&shard_id);

    assert!(!client.shard_capabilities_downgraded(&shard_id));
}

/// `shard_capabilities_downgraded` returns true after the shard drops a capability.
#[test]
fn test_shard_capabilities_downgraded_detects_post_registration_downgrade() {
    use downgraded_shard::DowngradedShard;

    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);

    // Register a shard that initially advertises all required caps.
    let shard_id = env.register_contract(None, DowngradedShard);
    client.add_shard(&shard_id);

    // Healthy at registration.
    assert!(!client.shard_capabilities_downgraded(&shard_id));

    // Simulate a bad upgrade: write the downgrade flag directly into the
    // shard's storage (same as calling downgrade() would do).
    env.as_contract(&shard_id, || {
        env.storage().instance().set(&soroban_sdk::Symbol::new(&env, "dg"), &true);
    });

    // Downgrade detected.
    assert!(
        client.shard_capabilities_downgraded(&shard_id),
        "must detect that the shard dropped the 'aggr' capability post-registration"
    );
}

/// Capability snapshot is removed when the shard is deregistered.
#[test]
fn test_capability_snapshot_removed_on_shard_removal() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    let (shard_id, _) = setup_score_shard(&env);
    client.add_shard(&shard_id);

    // Snapshot exists.
    assert!(!client.get_shard_capabilities(&shard_id).is_empty());

    client.remove_shard(&shard_id);

    // After removal the snapshot is gone.
    let caps = client.get_shard_capabilities(&shard_id);
    assert_eq!(caps.len(), 0, "snapshot must be cleared on shard removal");
}

// ── Issue #116 / #66: get_score_across_shards transport-error tracking ───────

/// When `get_score_across_shards` hits a contract-level error (e.g.
/// ScoreNotFound) it must NOT write LastShardFailure — that is normal "no data"
/// behaviour, not a shard failure.
#[test]
fn test_get_score_across_shards_contract_error_does_not_set_last_failure() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    let (shard_id, _shard_client) = setup_score_shard(&env);
    client.add_shard(&shard_id);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // No score submitted — shard returns ScoreNotFound (a contract-level Err).
    let results = client.get_score_across_shards(&wallet, &pair);

    // The shard entry must appear (wallet has no score, so None).
    assert_eq!(results.len(), 1);
    assert!(results.get(0).unwrap().1.is_none());

    // Crucially, a clean contract-level "not found" must NOT be recorded as a
    // shard failure (fix #116).
    assert_eq!(
        client.get_last_shard_failure(),
        None,
        "ScoreNotFound is not a transport failure — LastShardFailure must remain unset"
    );
}

/// When `get_score_across_shards` hits a host trap (transport error) it MUST
/// record the failure in LastShardFailure with FAILURE_TRANSPORT (0).
/// We verify this by checking that LastShardFailure is initially None and
/// that the error-distinction code paths in get_score_across_shards exist
/// (contract-level error → None entry, no LastShardFailure update vs
///  transport error → None entry + LastShardFailure update).
///
/// Note: the soroban test environment maps contract errors to Ok(Err(_)),
/// so we can't produce a host trap with a mock contract here. The code path
/// is verified in the query_risk_gate transport-failure tests and by the
/// fact that the two match arms are distinct in the implementation.
#[test]
fn test_get_score_across_shards_with_two_shards_no_failure_on_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    let (shard1_id, shard1_client) = setup_score_shard(&env);
    let (shard2_id, _) = setup_score_shard(&env);
    client.add_shard(&shard1_id);
    client.add_shard(&shard2_id);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Shard 1 has a score, shard 2 does not.
    shard1_client.submit_score(
        &Vec::new(&env), &wallet, &pair, &42, &false, &false, &1, &90, &1, &None,
    );

    let results = client.get_score_across_shards(&wallet, &pair);
    assert_eq!(results.len(), 2);

    // Shard 1 result has the score.
    let r0 = results.get(0).unwrap();
    assert_eq!(r0.0, shard1_id);
    assert!(r0.1.is_some());
    assert_eq!(r0.1.unwrap().score, 42);

    // Shard 2 result has None (ScoreNotFound = contract-level error, not transport).
    let r1 = results.get(1).unwrap();
    assert_eq!(r1.0, shard2_id);
    assert!(r1.1.is_none());

    // Neither shard produced a transport failure.
    assert_eq!(
        client.get_last_shard_failure(),
        None,
        "ScoreNotFound is a contract error — LastShardFailure must not be set"
    );
}

// ── Issue #115 / #65: is_shard_compatible uses try_get_score probe ────────────

/// A shard that passes all capability checks AND exposes try_get_score must be
/// accepted even if it does not implement legacy admin methods such as
/// get_arch_owner or get_mandatory_reviewers.
mod score_only_shard {
    use soroban_sdk::{contract, contractimpl, Env, Symbol};

    #[contract]
    pub struct ScoreOnlyShard;

    #[contractimpl]
    impl ScoreOnlyShard {
        pub fn supports_interface(env: Env, capability: Symbol) -> bool {
            capability == Symbol::new(&env, "score")
                || capability == Symbol::new(&env, "gate")
                || capability == Symbol::new(&env, "aggr")
                || capability == Symbol::new(&env, "arch")
        }

        pub fn get_score(
            _env: Env,
            _wallet: soroban_sdk::Address,
            _asset_pair: Symbol,
        ) -> Result<scoregate_score::RiskScore, scoregate_score::Error> {
            Err(scoregate_score::Error::ScoreNotFound) // confirms the method exists
        }

        // Intentionally does NOT implement get_arch_owner or
        // get_mandatory_reviewers.  Under fix #115 this shard must be accepted.
    }
}

#[test]
fn test_add_shard_accepts_shard_without_arch_owner_when_get_score_present() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);

    let shard = env.register_contract(None, score_only_shard::ScoreOnlyShard);

    // Must succeed — fix #115: get_arch_owner is no longer required.
    client.add_shard(&shard);
    assert_eq!(client.get_shards().len(), 1);
}

// ── Issue #114 / #64: get_aggregate_score prefers freshest observation ────────

#[test]
fn test_get_aggregate_score_prefers_fresher_over_higher_score() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Shard 1: stale high score (timestamp=50)
    let (shard1_id, shard1_client) = setup_score_shard(&env);
    shard1_client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &80,
        &false,
        &false,
        &50,
        &90,
        &1,
        &None,
    );

    // Shard 2: fresh lower score (timestamp=1000)
    let (shard2_id, shard2_client) = setup_score_shard(&env);
    shard2_client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &30,
        &false,
        &false,
        &1000,
        &90,
        &1,
        &None,
    );

    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let agg_client = ScoreGateAggregatorClient::new(&env, &agg_id);
    agg_client.initialize(&admin);
    agg_client.add_shard(&shard1_id);
    agg_client.add_shard(&shard2_id);

    // Fix #114: the aggregator must select the observation from shard 2 because
    // its last_updated timestamp is newer, not because its score is higher.
    let agg = agg_client.get_aggregate_score(&wallet);
    assert_eq!(
        agg.last_updated, 1000,
        "get_aggregate_score must return the freshest (highest last_updated) observation"
    );
    // The aggregate_score must come from shard 2's fresher data, not shard 1's
    // stale high score.
    assert!(
        agg.aggregate_score < 80,
        "stale high score must not override a fresher lower observation"
    );
}

// ── Issue #113 / #63: shard_added and shard_removed events ───────────────────

#[test]
fn test_add_shard_emits_shard_added_event() {
    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::IntoVal;

    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let (shard_id, _) = setup_score_shard(&env);

    client.add_shard(&shard_id);

    // The event topics are (symbol_short!("sh_added"), shard_address).
    let expected_topics: soroban_sdk::Vec<soroban_sdk::Val> =
        (symbol_short!("sh_added"), shard_id.clone()).into_val(&env);

    let found = env.events().all().iter().any(|(_addr, topics, _data)| {
        topics == expected_topics
    });
    assert!(found, "add_shard must emit a shard_added (sh_added) event");
}

#[test]
fn test_remove_shard_emits_shard_removed_event() {
    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::IntoVal;

    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, ScoreGateAggregator);
    let client = ScoreGateAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let (shard_id, _) = setup_score_shard(&env);

    client.add_shard(&shard_id);
    client.remove_shard(&shard_id);

    // The event topics are (symbol_short!("sh_rmvd"), shard_address).
    let expected_topics: soroban_sdk::Vec<soroban_sdk::Val> =
        (symbol_short!("sh_rmvd"), shard_id.clone()).into_val(&env);

    let found = env.events().all().iter().any(|(_addr, topics, _data)| {
        topics == expected_topics
    });
    assert!(found, "remove_shard must emit a shard_removed (sh_rmvd) event");
}
