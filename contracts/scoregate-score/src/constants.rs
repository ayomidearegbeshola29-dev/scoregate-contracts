#![cfg_attr(target_family = "wasm", allow(dead_code))]

pub const SCORE_TTL_THRESHOLD: u32 = 518_400;
pub const SCORE_TTL_EXTEND_TO: u32 = 777_600;

/// Maximum number of allowed gate callers in the allowlist.
pub const MAX_GATE_CALLERS: u32 = 20;

/// Hard lower bound for all score values submitted to the contract.
/// `submit_score` accepts scores in `[MIN_SCORE, MAX_SCORE]`; any value
/// below this is rejected with [`Error::InvalidScore`].
pub const MIN_SCORE: u32 = 0;

/// Hard upper bound for all score values submitted to the contract.
pub const MAX_SCORE: u32 = 100;

/// Hard ceiling on the ring-buffer depth to bound storage costs.
pub const MAX_HISTORY_DEPTH: u32 = 50;
pub const DEFAULT_HISTORY_MAX_DEPTH: u32 = 10;
pub const MAX_BATCH_SIZE: u32 = 20;

/// Upper bound on `page_size` for paginated exports (#18).
///
/// Without a cap, `page_size: u32::MAX` makes the export loop walk the entire
/// entry index in a single call, exhausting the VM CPU/memory budget and
/// turning a read endpoint into a way to make the contract run out of gas.
pub const MAX_EXPORT_PAGE_SIZE: u32 = 200;
pub const MAX_ASSET_PAIR_BYTES: u32 = 9;
pub const MAX_ASSET_PAIR_LEN: u32 = MAX_ASSET_PAIR_BYTES;
pub const MAX_SCORE_COMMITMENT_BYTES: u32 = 32;
pub const MAX_DISPUTE_BOND_PREIMAGE_BYTES: u32 = 80;
pub const MAX_DISPUTE_BOND_SALT_BYTES: u32 = 64;

/// Maximum number of entries accepted in a single batch score read call.
pub const BATCH_READ_MAX: u32 = 50;

/// Default risk threshold used when no threshold has been configured by admin.
pub const DEFAULT_RISK_THRESHOLD: u32 = 75;

/// Default threshold for score jump anomaly detection.
pub const DEFAULT_JUMP_THRESHOLD: u32 = 30;

/// Absolute jump threshold used when the rolling sample variance of a
/// wallet's score history is zero — i.e. a flatline series where every
/// stored score is identical (issue #189). Computing a delta-over-variance
/// anomaly score in that case would be an integer division by zero, so the
/// delta is compared against this fixed value instead.
pub const MIN_ABSOLUTE_JUMP_THRESHOLD: u32 = 15;

/// Semantic contract version; bump on breaking ABI changes.
///
/// History:
///
/// * `1` — initial release (`submit_score` / `get_score`).
/// * `2` — `submit_score` gained the `attestation: Option<ScoreAttestation>`
///   parameter and `set_service_pubkey` / `get_service_pubkey` were added
///   (see `docs/attestation-spec.md`).
/// * `3` — `submit_scores_batch_attested` and the `batch_attested`
///   `supports_interface` capability were added (see
///   `docs/batch-attestation-spec.md`).
/// * `4` — Added contract_id and contract_version binding to attestations (#200),
///   Merkle audit chain for admin actions (#201), configurable decay profiles (#202),
///   and multi-dimensional risk scores with sub-components (#203).
/// * `5` — Added post-incident replay and reconciliation workflow (#631): emergency
///   freeze/unfreeze, deterministic state checksums, paginated score export, on-chain
///   snapshot history, reconciliation verification, and off-chain recovery tooling.
pub const CONTRACT_VERSION: u32 = 5;

/// Hard upper bound on Merkle proof length.
pub const MAX_MERKLE_PROOF_DEPTH: u32 = 30;
pub const MAX_WALLET_PAIRS: u32 = 20;
pub const DEFAULT_COOLDOWN_SECS: u64 = 3_600;
pub const MIN_COOLDOWN_SECS: u64 = 60;
pub const MAX_COOLDOWN_SECS: u64 = 86_400;
/// Maximum number of tokens allowed in a rate-limit burst bucket.
pub const MAX_BURST_CAPACITY: u32 = 3;
pub const MIN_UPGRADE_DELAY_SECS: u64 = 172_800;
pub const MAX_UPGRADE_DELAY_SECS: u64 = 1_209_600;
pub const DEFAULT_UPGRADE_DELAY_SECS: u64 = 172_800;
pub const MAX_SERVICE_SIGNERS: u32 = 10;
pub const MAX_ADMIN_SIGNERS: u32 = 5;
/// Grace period (seconds) before a newly added signer transitions from Pending to Active (issue #691).
pub const DEFAULT_SIGNER_GRACE_PERIOD_SECS: u64 = 3_600; // 1 hour
pub const DEFAULT_STALENESS_WINDOW_SECS: u64 = 604_800;

/// Default maximum age (seconds) of an oracle's last price update before the
/// oracle is considered stale and `get_effective_score` falls back to
/// unadjusted confidence.  Default: 1 hour, matching `FAILOVER_STALENESS_WINDOW`.
/// Configurable via `set_oracle_staleness_threshold` / `get_oracle_staleness_threshold`.
pub const DEFAULT_ORACLE_STALENESS_THRESHOLD_SECS: u64 = 3_600;

/// Maximum age (seconds) of a secondary score for it to be accepted during
/// failover. Secondary scores older than this window cause the gate to
/// return `false` (fail-closed). Default: 1 hour.
pub const FAILOVER_STALENESS_WINDOW: u64 = 3_600;

pub const MAX_PAUSED_PAIRS: u32 = 50;
pub const DECAY_FIXED_POINT_SCALE: u64 = 1_000_000;
pub const DEFAULT_DECAY_LAMBDA_NUM: u64 = 0;
pub const DEFAULT_DECAY_LAMBDA_DEN: u64 = 1;
pub const MAX_DECAY_LAMBDA_NUM: u64 = 1;
pub const MAX_DECAY_LAMBDA_DEN: u64 = 1;

/// Maximum decay horizon in seconds (365 days) (issue #188).
///
/// Elapsed times are clamped to this horizon before the fixed-point decay
/// and interpolation math scales them, so arbitrarily large timestamps
/// (multi-year gaps, simulated future timestamps) can never overflow the
/// intermediates. Scores older than the horizon evaluate to the same
/// (saturated) value as a score exactly at the horizon — a fully decayed
/// neutral baseline for any practically configured λ.
pub const MAX_DECAY_HORIZON_SECS: u64 = 31_536_000;

/// Maximum number of counterparty links allowed per wallet per asset pair.
pub const MAX_COUNTERPARTY_LINKS_PER_WALLET: u32 = 50;

/// Maximum delegation chain depth to prevent unbounded traversal.
/// Prevents DoS attacks via deep circular delegation chains.
pub const MAX_DELEGATION_DEPTH: u32 = 5;

// ── Score submission floor ─────────────────────────────────────────────────────

/// Default high-water mark for the score floor policy.
pub const DEFAULT_SCORE_FLOOR_HWM: u32 = 80;
pub const DEFAULT_SCORE_FLOOR_MIN: u32 = 20;
pub const MIN_SCORE_FLOOR_HWM: u32 = 50;
pub const MAX_SCORE_FLOOR_HWM: u32 = 100;
pub const MAX_HYSTERESIS_MARGIN: u32 = 50;

/// Bounds of a static pair correlation coefficient, scaled ×10 000 (so `-10_000` is ρ = -1 and
/// `10_000` is ρ = 1). Both bounds are valid values.
pub const MIN_PAIR_CORRELATION: i32 = -10_000;
pub const MAX_PAIR_CORRELATION: i32 = 10_000;
pub const BAND_STATE_TTL_THRESHOLD: u32 = 518_400;
pub const BAND_STATE_TTL_EXTEND_TO: u32 = 777_600;
pub const EMBARGO_TTL_THRESHOLD: u32 = 1_555_200;
pub const EMBARGO_TTL_EXTEND_TO: u32 = 3_110_400;

/// Hard ceiling on the `EmbargoedWalletIndex` so `revoke_all_embargoes` stays
/// within a single transaction's resource budget.
pub const MAX_EMBARGOED_WALLETS: u32 = 100;
pub const DEFAULT_CONSENSUS_THRESHOLD_K: u32 = 2;
pub const DEFAULT_CONSENSUS_EPSILON: u32 = 5;
/// Minimum spread required between lower and upper adaptive epsilon bounds (#190).
pub const MIN_EPSILON_SPREAD: u32 = 5;

// ── Escalation / consecutive breach ──────────────────────────────────────────

pub const ESCALATION_BREACH_TTL_THRESHOLD: u32 = 518_400;
pub const ESCALATION_BREACH_TTL_EXTEND_TO: u32 = 777_600;
pub const DEFAULT_ESCALATION_THRESHOLD: u32 = 5;
pub const MIN_ESCALATION_THRESHOLD: u32 = 2;
pub const MAX_ESCALATION_THRESHOLD: u32 = 20;

// ── Model version registry ────────────────────────────────────────────────────

/// Hard upper bound on the number of model versions that can be registered.
pub const MAX_MODEL_VERSIONS: u32 = 20;

/// Challenge period in seconds (7 days).
pub const DISPUTE_CHALLENGE_PERIOD_SECS: u64 = 604_800;

/// Bonus percentage added to the returned bond on timeout settlement.
pub const DISPUTE_BONUS_PCT: i128 = 10;

/// Maximum simultaneously open disputes.
pub const MAX_OPEN_DISPUTES: u32 = 100;

/// Maximum concurrently open disputes allowed per single challenger actor.
pub const MAX_DISPUTES_PER_ACTOR: u32 = 5;

pub const DISPUTE_TTL_THRESHOLD: u32 = 518_400;
pub const DISPUTE_TTL_EXTEND_TO: u32 = 777_600;

/// Default reveal window for sealed-bid dispute bond: 10 minutes (600 seconds).
pub const DEFAULT_DISPUTE_REVEAL_WINDOW_SECS: u64 = 600;

// ── Finality buffer (pending score commit window) ────────────────────────────

/// Maximum finality buffer duration — 24 hours.
pub const MAX_FINALITY_BUFFER_SECS: u64 = 86_400;

// ── Cluster membership ────────────────────────────────────────────────────────

/// Number of score buckets used to partition wallets into clusters.
///
/// A wallet's cluster is derived from its score via
/// `score / CLUSTER_BUCKET_WIDTH`, so scores `0..=9` map to cluster `0`,
/// `10..=19` to cluster `1`, and so on up to `100` (cluster `10`).
/// Assignment and membership enumeration must both use this same definition.
pub const CLUSTER_BUCKET_WIDTH: u32 = 10;

/// Maximum number of members returned by a single `get_cluster_members` call.
pub const MAX_CLUSTER_MEMBERS: u32 = 200;
