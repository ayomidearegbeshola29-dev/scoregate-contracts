# Upgrade approval snapshot — model-check validation (issue #413)

## Properties added

| ID | Name | Kind | Meaning |
|---|---|---|---|
| INV-UPG-1 | `UpgradeApprovalsWithinSnapshot` | invariant | Every approval is a member of the signer set captured when accumulation began. |
| INV-UPG-2 | `UpgradeSnapshotConsistent` | invariant | The captured snapshot is a subset of the live admin set, and approval/snapshot are empty together. |
| INV-UPG-3 | `UpgradeApprovalsResetAfterMutation` | property | Any change to the live signer set atomically resets approvals and snapshot. |
| INV-UPG-4 | `UpgradeEmptyStateIsConsistent` | property | The empty accumulator state is explicit and preserved, including stuttering steps. |

All four are registered in `spec/ScoreGate.cfg` (`INVARIANT` for 1–2, `PROPERTY` for 3–4).

## TLC validation plan

TLC is not installed in the environment used to prepare this change, so the
TLA+ model itself was **not** executed here. Run the following in an
environment with the TLA+ toolbox or `tlacli`:

```bash
# 1. Normal model — all new invariants/properties must be satisfied.
tlacli check spec/ScoreGate.cfg --spec Spec \
  --invariant HistoricalMaxMonotonicity UpgradeApprovalsWithinSnapshot \
             UpgradeSnapshotConsistent \
  --property UpgradeApprovalsResetAfterMutation UpgradeEmptyStateIsConsistent

# 2. Mutation — remove the two clearing lines from MutateAdminSet
#    (upg_approvals' = {} / upg_signer_snap' = {}) in a copy of the spec.
#    TLC must report a counterexample for INV-UPG-2 (and INV-UPG-3),
#    typically:
#      state 1: approvals = {"S1"}, snapshot = {"S1","S2","S3"},
#               live = {"S1","S2","S3"}
#      state 2: AddUpgradeApproval("S1")
#      state 3: MutateAdminSet("S1")
#      violation: {"S1"} remains in upg_approvals / snapshot contains a
#                 signer that is no longer live
```

The expected failing trace for the mutation is reproduced exactly by the
exhaustive checker below.

## Exhaustive slice check (executed)

`spec/check_upgrade_invariants.py` explores the complete reachable state space
of the upgrade variables (`upg_approvals`, `upg_signer_snap`,
`upg_live_signers`) over the same three-signer domain as `ScoreGate.cfg`.
This is exact rather than sampled because the upgrade actions leave every
other variable `UNCHANGED` and no other action reads or writes them.

Command:

```bash
python3 spec/check_upgrade_invariants.py
```

Recorded output:

```
=== Unmutated model (MutateAdminSet clears approvals) ===
reachable states explored: 27
invariant violations: 0
INV-UPG-1, INV-UPG-2, INV-UPG-4: PASS on the complete reachable state space

=== Mutated model (clearing removed from MutateAdminSet) ===
reachable states explored: 160
invariant violations: 115
first counterexample: approvals=['S1'], snapshot=['S1', 'S2', 'S3'], live=['S2', 'S3']
  INV-UPG-2 snapshot signer no longer live: ['S1']
counterexample trace:
  AddUpgradeApproval(S1): approvals=['S1'], snapshot=['S1', 'S2', 'S3'], live=['S1', 'S2', 'S3']
  MutateAdminSet(S1): approvals=['S1'], snapshot=['S1', 'S2', 'S3'], live=['S2', 'S3']

=== Reset-after-mutation property ===
mutated states where signer set changes with a non-empty accumulator: 456
expected: > 0 (each is an INV-UPG-3 counterexample)
```

This satisfies the validation plan's requirement to record state counts and
the expected failing trace for the mutation, and confirms that the empty state
is handled explicitly (0 violations in the unmutated model).

## Refinement to Rust

See `spec/refinement-mapping.md` §6 for the concrete correspondence:

- `upg_approvals` ↔ `DataKeyD::UpgradeApprovals` / `get_upgrade_approvals()`.
- `upg_live_signers` ↔ `DataKey::AdminSet` / `get_admin_set()`.
- `AddUpgradeApproval(s)` ↔ `propose_upgrade()`: `require_policy_auth` verifies
  the signer is a current set member before the address is appended, and the
  accumulator is cleared when the threshold is met.
- `MutateAdminSet(s)` ↔ `add_admin_signer()` / `remove_admin_signer()`: both
  call `clear_upgrade_approvals()` immediately after updating the admin set.
- `ClearUpgradeApprovals` ↔ `clear_upgrade_approvals()` (also used by veto and
  upgrade execution).

The TLA+ model keeps an explicit `upg_signer_snap`; the Rust implementation
achieves the same guarantee by invalidating the accumulator on every signer-set
mutation, which is a strictly stronger enforcement of the same refinement.
