# Check upgrade approval snapshot safety in the formal model (#413)

## Summary

The TLA+ model already explored `upg_approvals`, `upg_signer_snap` and `upg_live_signers`, but `ScoreGate.cfg` had no
invariant or property constraining them. This change adds four machine-checkable properties covering approval
subsetting, snapshot/live-set consistency, reset-after-mutation and the explicit empty state, registers them in TLC,
and documents both the Rust refinement and the mutation validation.

## What changed

### `spec/ScoreGate.tla`
- **INV-UPG-1 `UpgradeApprovalsWithinSnapshot`** — every approval must belong to the signer set captured when
  accumulation began. This is the direct check for approvals collected outside the captured signer set.
- **INV-UPG-2 `UpgradeSnapshotConsistent`** — `upg_signer_snap ⊆ upg_live_signers`, and approvals/snapshot are empty
  together, so a stale snapshot cannot survive a reset.
- **INV-UPG-3 `UpgradeApprovalsResetAfterMutation`** — a box-property stating that any change to `upg_live_signers`
  atomically clears both approvals and snapshot. Removing the clearing from `MutateAdminSet` yields a counterexample.
- **INV-UPG-4 `UpgradeEmptyStateIsConsistent`** — asserts the empty-state equivalence explicitly, including stuttering.

### `spec/ScoreGate.cfg`
- Registered INV-UPG-1/2 under `INVARIANT` and INV-UPG-3/4 under `PROPERTY`.

### Validation
- **`spec/check_upgrade_invariants.py`** — exact exhaustive explorer for the upgrade variable slice. The three upgrade
  actions leave every other variable `UNCHANGED` and nothing else reads/writes them, so a complete 3-signer exploration
  is possible without TLC.
  - Unmutated model: **27 reachable states, 0 violations**.
  - Mutated model (clearing removed): **160 states, 115 violations**; first trace is
    `AddUpgradeApproval(S1)` → `MutateAdminSet(S1)`, leaving `approvals={S1}`, `snapshot={S1,S2,S3}`,
    `live={S2,S3}` (stale snapshot signer).
  - Reset-after-mutation counterexamples: 456 states where the signer set changes with a non-empty accumulator.
- **`spec/upgrade-approval-validation.md`** — records the property table, TLC commands, the mutation trace, state
  counts and the executed slice-check output. TLC itself is not installed in this environment, so the TLA+ run is
  provided as a reproducible plan rather than claimed as executed.

### Documentation
- `spec/README.md` — added invariants 26–29, the three `upg_*` variables, the three upgrade actions, and a link to the validation record.
- `spec/refinement-mapping.md` — added the abstract-action → Rust entry-point rows and invariant correspondence rows,
  plus references. Concretely: `DataKeyD::UpgradeApprovals`, `propose_upgrade()` membership checks,
  `add_admin_signer()`/`remove_admin_signer()` clearing approvals after admin-set changes, and
  `clear_upgrade_approvals()` on veto/execution. The explicit TLA+ snapshot refines to Rust's stronger strategy of
  invalidating the accumulator on every signer-set mutation.

## Acceptance criteria addressed

- **Normal model satisfies the properties** — exhaustive slice check passes with 0 violations; TLC commands provided.
- **Removing clearing produces a counterexample** — reproduced with an exact trace and state counts.
- **Empty approval state handled explicitly** — INV-UPG-2/4 and a dedicated empty-state clause; no unmutated violations.

## Testing

```bash
python3 spec/check_upgrade_invariants.py   # passes (exit 0)
# With TLA+ tooling available:
tlacli check spec/ScoreGate.cfg --spec Spec
```

No Rust sources were changed.

closes #413
