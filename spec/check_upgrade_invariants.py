#!/usr/bin/env python3
"""Exhaustive checker for the upgrade-approval slice of spec/ScoreGate.tla.

The three upgrade variables (upg_approvals, upg_signer_snap,
upg_live_signers) are independent of the rest of the TLA+ model: the three
actions that touch them leave all other variables UNCHANGED, and no other
action reads or writes them. This makes it possible to explore the complete
reachable state space of that slice exactly, with no bounds other than the
three-signer domain used in ScoreGate.cfg.

Run with: python3 spec/check_upgrade_invariants.py

A non-zero exit code means an invariant was violated in the unmutated model
or that the mutated model failed to produce the expected counterexample.
"""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass
from itertools import combinations
from typing import Callable, Iterable

SIGNERS = ("S1", "S2", "S3")

State = tuple[frozenset[str], frozenset[str], frozenset[str]]


@dataclass(frozen=True)
class Step:
    action: str
    signer: str
    before: State
    after: State


def add_upgrade_approval(state: State, signer: str) -> State | None:
    approvals, snap, live = state
    if signer not in live or signer in approvals:
        return None
    effective_snap = live if not approvals else snap
    if effective_snap != live:
        return frozenset({signer}), frozenset(live), live
    return frozenset(set(approvals) | {signer}), effective_snap, live


def mutate_admin_set(state: State, signer: str, *, clear: bool) -> State:
    approvals, snap, live = state
    new_live = frozenset(set(live) - {signer} if signer in live else set(live) | {signer})
    if clear:
        return frozenset(), frozenset(), new_live
    # Mutation under test: the signer set changes but approvals survive.
    return approvals, snap, new_live


def clear_upgrade_approvals(state: State) -> State:
    _, _, live = state
    return frozenset(), frozenset(), live


def successors(state: State, *, clear_on_mutation: bool) -> Iterable[tuple[str, str, State]]:
    for signer in SIGNERS:
        after = add_upgrade_approval(state, signer)
        if after is not None:
            yield "AddUpgradeApproval", signer, after
        yield (
            "MutateAdminSet",
            signer,
            mutate_admin_set(state, signer, clear=clear_on_mutation),
        )
    yield "ClearUpgradeApprovals", "-", clear_upgrade_approvals(state)


def explore(*, clear_on_mutation: bool) -> tuple[list[State], dict[State, Step]]:
    initial: State = (frozenset(), frozenset(), frozenset(SIGNERS))
    seen: dict[State, Step] = {initial: Step("Init", "-", initial, initial)}
    queue = deque([initial])
    order: list[State] = [initial]

    while queue:
        state = queue.popleft()
        for action, signer, after in successors(state, clear_on_mutation=clear_on_mutation):
            if after not in seen:
                seen[after] = Step(action, signer, state, after)
                order.append(after)
                queue.append(after)
    return order, seen


def violations(state: State) -> list[str]:
    approvals, snap, live = state
    problems: list[str] = []
    if not approvals <= snap:
        problems.append(f"INV-UPG-1 approvals outside snapshot: {sorted(approvals - snap)}")
    if not snap <= live:
        problems.append(f"INV-UPG-2 snapshot signer no longer live: {sorted(snap - live)}")
    if (not approvals) != (not snap):
        problems.append(
            f"INV-UPG-2/4 empty-state mismatch: approvals={sorted(approvals)} snap={sorted(snap)}"
        )
    return problems


def trace_to(steps: dict[State, Step], target: State) -> list[Step]:
    trace: list[Step] = []
    cursor = target
    while cursor in steps and steps[cursor].action != "Init":
        step = steps[cursor]
        trace.append(step)
        cursor = step.before
    trace.reverse()
    return trace


def format_state(state: State) -> str:
    approvals, snap, live = state
    return (
        f"approvals={sorted(approvals) or ['-']}, "
        f"snapshot={sorted(snap) or ['-']}, "
        f"live={sorted(live) or ['-']}"
    )


def main() -> int:
    ok = True

    states, steps = explore(clear_on_mutation=True)
    broken = [state for state in states if violations(state)]
    print("=== Unmutated model (MutateAdminSet clears approvals) ===")
    print(f"reachable states explored: {len(states)}")
    print(f"invariant violations: {len(broken)}")
    if broken:
        ok = False
        for state in broken[:5]:
            print(f"  {format_state(state)} -> {violations(state)}")
    else:
        print("INV-UPG-1, INV-UPG-2, INV-UPG-4: PASS on the complete reachable state space")

    mutated, mutated_steps = explore(clear_on_mutation=False)
    mutated_broken = [state for state in mutated if violations(state)]
    print()
    print("=== Mutated model (clearing removed from MutateAdminSet) ===")
    print(f"reachable states explored: {len(mutated)}")
    print(f"invariant violations: {len(mutated_broken)}")
    if not mutated_broken:
        print("ERROR: expected a counterexample but the mutation was not detected")
        ok = False
    else:
        target = mutated_broken[0]
        print(f"first counterexample: {format_state(target)}")
        for problem in violations(target):
            print(f"  {problem}")
        print("counterexample trace:")
        for step in trace_to(mutated_steps, target):
            print(f"  {step.action}({step.signer}): {format_state(step.after)}")

    print()
    print("=== Reset-after-mutation property ===")
    reset_failures = 0
    for state in mutated:
        for signer in SIGNERS:
            before_live = state[2]
            after_live = mutate_admin_set(state, signer, clear=False)[2]
            if after_live != before_live and (state[0] or state[1]):
                reset_failures += 1
    print(f"mutated states where signer set changes with a non-empty accumulator: {reset_failures}")
    print("expected: > 0 (each is an INV-UPG-3 counterexample)")
    if reset_failures == 0:
        ok = False

    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
