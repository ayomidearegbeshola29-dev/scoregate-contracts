# ScoreGate Contract Architecture & Governance Lifecycle

This document outlines the architecture ownership, mandatory technical reviewer
pipeline, and parameter change execution workflows in `scoregate-score`.

## Architecture Ownership and Mandatory Reviewers

The contract maintains an architecture owner (`ArchitectureDataKey::ArchOwner`)
and a bounded allowlist of mandatory technical reviewers
(`ArchitectureDataKey::MandatoryReviewers`, up to `MAX_MANDATORY_REVIEWERS = 10`).

- `set_arch_owner(new_owner)` / `get_arch_owner()` — transfers or queries the
  architecture owner role (authorized by the existing architecture owner, or by
  the contract admin if none is configured).
- `set_mandatory_reviewers(reviewers)` / `get_mandatory_reviewers()` — updates
  or queries the list of mandatory technical reviewers.
- `approve_proposal_review(reviewer, target)` /
  `get_proposal_review_approvals(target)` — records and queries per-target
  mandatory reviewer sign-offs (`ArchitectureDataKey::ReviewApprovals(target)`).

### Enforcement Across Governance Pipelines

When `get_mandatory_reviewers()` is empty (the default), governance proposals
require only the standard admin or policy quorum and time-lock delay.

When `get_mandatory_reviewers()` is non-empty, every configured reviewer must
call `approve_proposal_review(reviewer, target)` (which enforces
`reviewer.require_auth()` and verifies membership in `MandatoryReviewers`)
before any of the following execution entry points can succeed:

| Pipeline | Execution Entry Point | Review `target` Symbol |
|----------|-----------------------|------------------------|
| WASM Upgrade Governance | `execute_upgrade` | `upgrade` |
| Policy Bundle Governance | `apply_policy_bundle` | `pol_bdl` |
| Time-Locked Parameter Setters | `apply_param_change(key)` | `key` (`risk_thr`, `hist_dep`, `upg_dly`, `cooldown`, `stale_w`) |
| Parameter Proposal Governance | `execute_parameter_change(admin_signers, proposal_id)` | `proposal.param_key` |

If any configured mandatory reviewer has not approved `target`, execution fails
with `Error::MissingMandatoryReviewerApprovals`. Recorded approvals for a target
are cleared whenever a new proposal is created, executed/applied, or vetoed.

## Time-Locked Parameter Changes (`apply_param_change`)

Direct parameter configuration setters stage a `ParamChangeProposal` in
instance storage (`DataKey::PendingParamChange(key)`) with
`apply_after = now + get_param_change_delay()` and emit `param_change_proposed`
(topic `pc_prop`):

| Parameter Key (`Symbol`) | Setter | Value Variant | Storage Target |
|--------------------------|--------|---------------|----------------|
| `risk_thr` | `set_risk_threshold` | `ParamValue::U32(threshold)` | `DataKey::RiskThreshold` |
| `hist_dep` | `set_history_max_depth` | `ParamValue::U32(depth)` | `DataKey::HistoryMaxDepth` |
| `upg_dly` | `set_upgrade_delay` | `ParamValue::U64(delay_secs)` | `DataKey::UpgradeDelaySecs` |
| `cooldown` | `propose_cooldown_change` (`set_cooldown` applies immediately and stages nothing) | `ParamValue::U64(secs)` | `DataKey::CooldownSecs` |
| `stale_w` | `set_staleness_window` | `ParamValue::U64(window_secs)` | `DataKey::StalenessWindowSecs` |

Once `env.ledger().timestamp() >= proposal.apply_after` (and all mandatory
reviewers have approved `key`, if configured), calling `apply_param_change(key)`:

1. Validates the parameter key and `ParamValue` variant.
2. Updates the corresponding contract storage entry and emits the
   parameter-specific update event (`risk_threshold_updated`,
   `history_max_depth_updated`, `upgrade_delay_updated`, `cooldown_updated`, or
   `staleness_window_updated`).
3. Clears the pending proposal and reviewer approvals for `key`.
4. Emits the unified execution audit event `param_change_applied` (topic
   `pc_appl`) with data `(key, new_value)`.

## Policy Bundle Lifecycle (`propose_policy_bundle` / `apply_policy_bundle` / `veto_policy_bundle`)

Policy bundles allow operators to propose and atomically apply paired updates to
`risk_threshold` and `cooldown_secs`:

1. **Propose**: `propose_policy_bundle(admin_signers, risk_threshold, cooldown_secs)`
   validates both parameters, stores `DataKey::PendingPolicyBundle`, and emits
   `pbdl_prop`.
2. **Veto**: During the time-lock window, `veto_policy_bundle(admin_signers, bundle_id)`
   verifies admin authorization, removes the pending bundle, clears any
   accumulated reviewer approvals for `pol_bdl`, and emits
   `policy_bundle_vetoed` (topic `pbdl_veto`) with `(bundle_id, timestamp)`.
3. **Apply**: After `apply_after` elapses (and mandatory reviewer approvals are
   satisfied), `apply_policy_bundle()` atomically updates both `RiskThreshold`
   and `CooldownSecs`, clears the pending bundle, and emits `pbdl_appl`.
