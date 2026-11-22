//! Criterion benchmark for `get_private_aggregate_score`.
//!
//! Run: `cargo bench -p scoregate-score --bench get_private_aggregate_score`
//!
//! Measures the CPU/memory cost of computing a differentially-private aggregate
//! score for a wallet via `get_private_aggregate_score`.  The cost is
//! dominated by `compute_aggregate_score`, which iterates over all wallet
//! pairs and reads each pair's score from storage.  The differential privacy
//! noise computation is O(1) given the aggregate score, and it only runs when
//! the privacy epsilon is above zero (`set_privacy_epsilon`), so the two
//! paths are measured separately:
//!   • `noise_disabled` — epsilon is 0 (the default); the query returns the
//!     plain aggregate.
//!   • `noise_enabled` — epsilon is `NOISE_EPSILON_BPS`; the query also hashes
//!     the seed and adds the noise.
//!
//! Two input-size cases are benchmarked for each path:
//!   • Small: 1 asset pair submitted for the wallet — minimal cost.
//!   • Near-limit: 20 asset pairs submitted for the wallet — exercises
//!     the full O(N) iteration loop over all wallet pairs, which is the
//!     documented worst-case bound (`debug_assert!(pairs.len() <= 20)`).
//!
//! Setup (contract registration, score submission, epsilon configuration) runs
//! once per case, outside the timed closure, and the query only reads storage,
//! so every measured call starts from the same state. The cost of building the
//! fixture is reported on its own in `get_private_aggregate_score_fixture_setup`.
//!
//! The measured cost is relevant to the resource budget a caller pays for,
//! since `get_private_aggregate_score` computes a per-wallet aggregate that
//! scales with the number of asset pairs the wallet has scores for.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use scoregate_score::{ScoreGateScoreContract, ScoreGateScoreContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

const START_TS: u64 = 1_700_000_000;

/// Epsilon for the noise-enabled case, in the unit `set_privacy_epsilon` takes
/// (ε × 100, so 100 is ε = 1.0).
const NOISE_EPSILON_BPS: u32 = 100;

/// The two paths of the query, with the epsilon that selects each one.
const NOISE_PATHS: [(&str, u32); 2] = [("noise_disabled", 0), ("noise_enabled", NOISE_EPSILON_BPS)];

/// Seed passed to every measured query.
const SEED: u32 = 1;

fn setup(env: &Env) -> (ScoreGateScoreContractClient<'_>, Address) {
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, ScoreGateScoreContract);
    let client = ScoreGateScoreContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let service = Address::generate(env);
    client.initialize(&admin, &service);

    let wallet = Address::generate(env);
    (client, wallet)
}

fn submit_pairs(
    env: &Env,
    client: &ScoreGateScoreContractClient,
    wallet: &Address,
    pair_count: u32,
) {
    for i in 0..pair_count {
        let pair = Symbol::new(env, &format!("ASSET_{}", i));
        client.submit_score(
            &Vec::new(env),
            wallet,
            &pair,
            &50u32,
            &false,
            &false,
            &START_TS,
            &80u32,
            &1u32,
            &None,
        );
    }
}

/// A wallet with `pair_count` scored pairs and the privacy epsilon set to
/// `epsilon_bps` (0 leaves noise disabled). Asserts the state the benchmark
/// case claims to measure.
fn prepare(
    env: &Env,
    pair_count: u32,
    epsilon_bps: u32,
) -> (ScoreGateScoreContractClient<'_>, Address) {
    let (client, wallet) = setup(env);
    submit_pairs(env, &client, &wallet, pair_count);
    if epsilon_bps > 0 {
        client.set_privacy_epsilon(&Vec::new(env), &epsilon_bps);
    }

    assert_eq!(client.get_privacy_epsilon(), epsilon_bps);
    assert_eq!(client.get_wallet_pair_list(&wallet).len(), pair_count);
    (client, wallet)
}

fn bench_get_private_aggregate_score(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_private_aggregate_score");
    group.sample_size(10);

    for &pair_count in &[1u32, 20u32] {
        for (path, epsilon_bps) in NOISE_PATHS {
            group.bench_with_input(
                BenchmarkId::new(path, pair_count),
                &pair_count,
                |b, &pair_count| {
                    let env = Env::default();
                    let (client, wallet) = prepare(&env, pair_count, epsilon_bps);
                    b.iter(|| black_box(client.get_private_aggregate_score(&wallet, &SEED)));
                },
            );
        }
    }

    group.finish();
}

/// The cost of building the fixture above, so it can be subtracted from any
/// measurement that includes it.
fn bench_fixture_setup(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_private_aggregate_score_fixture_setup");
    group.sample_size(10);

    for &pair_count in &[1u32, 20u32] {
        group.bench_with_input(
            BenchmarkId::new("pair_count", pair_count),
            &pair_count,
            |b, &pair_count| {
                b.iter(|| {
                    let env = Env::default();
                    let (client, wallet) = setup(&env);
                    submit_pairs(&env, &client, &wallet, pair_count);
                    black_box((&client, &wallet));
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_get_private_aggregate_score, bench_fixture_setup);
criterion_main!(benches);
