#!/usr/bin/env bash
# Fails when the replay analysis tests are not part of the crate's test list.
#
# `determinism` and `wasm_analysis` used to be source files that no module declared, so they were
# never compiled and their tests never ran. Listing the tests compiles the crate (a syntax error in
# either module fails here) and lets us check that representative tests are present.
set -euo pipefail

REQUIRED_TESTS=(
  "determinism::tests::test_identical_results_are_deterministic"
  "determinism::tests::test_state_divergence_detected"
  "wasm_analysis::tests::test_size_regression_detection"
  "wasm_analysis::tests::test_static_memory_offset"
  "wasm_analysis::tests::test_size_decrease_is_not_a_regression"
)

LISTING=$(cargo test -p replay --lib -- --list)

MISSING=0
for name in "${REQUIRED_TESTS[@]}"; do
  if ! grep -Fq "${name}: test" <<<"$LISTING"; then
    echo "Missing from the replay test list: ${name}" >&2
    MISSING=1
  fi
done

if [[ "$MISSING" -ne 0 ]]; then
  echo "The replay analysis modules must be declared in tools/replay/src/lib.rs." >&2
  exit 1
fi

echo "Replay analysis tests are in the test list."
