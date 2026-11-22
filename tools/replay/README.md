# Replay Harness

A deterministic replay tool for regression testing the ScoreGate contract with real Stellar mainnet trade history.

## Overview

The replay harness reads a Horizon API snapshot (NDJSON format) and feeds it through the off-chain detection pipeline simulation and contract submission functions. It validates:

- **No panics**: all contract calls complete without panicking
- **Score range**: all accepted scores remain in [0, 100]
- **Rate limits**: repeated submissions for the same (wallet, pair) are rate-limited as expected
- **Numerical stability**: no arithmetic overflow in aggregate score computations
- **Determinism**: identical input produces identical contract state and call sequences

## Snapshot Format

Each line is JSON:
```json
{"wallet":"wallet_id","asset_pair":"XLM_USDC","trades":[{"price":0.12},{"price":0.13}]}
```

- `wallet` (string): Horizon account ID or identifier
- `asset_pair` (string): asset pair symbol (e.g., "XLM_USDC")
- `trades` (array, optional): trade records with `price` field

## Building

```bash
cargo build -p replay
```

## Running

```bash
cargo run -p replay --manifest-path tools/replay/Cargo.toml
```

By default, the binary reads `testdata/reference.ndjson` relative to the replay
crate, so it works when launched from either the workspace root or this directory.
An explicit snapshot path is still interpreted relative to the caller's current
working directory.

### Configuration drift detection

The same binary can compare an approved deployment manifest against an
observed live snapshot:

```bash
cargo run -p replay --manifest-path tools/replay/Cargo.toml -- \
  config-drift approved.json observed.json
```

To emit a template containing the supported config surface:

```bash
cargo run -p replay --manifest-path tools/replay/Cargo.toml -- \
  config-drift --template
```

Diff output is deterministic JSON sorted by field name. Unknown approved
fields, unknown observed fields, missing observed fields, and value drift are
reported separately so operators can distinguish manifest mistakes from
unauthorized on-chain changes.

## Traceability Report

`tools/replay/traceability_report.py` produces a **mapping-only** traceability
report. It links requirement IDs to the test IDs you supply; it does **not**
run tests and does **not** ingest test results. Consequently the report never
claims that a test was executed or passed — it only records that a test ID is
*referenced* by a requirement.

Because the tool cannot verify that a referenced test ID actually exists, a
nonexistent test ID is still reported as a reference (not as a successful
execution). Treat the output as a coverage map, not as evidence of passing
tests.

### Coverage metric

`coverage` is the fraction of requirements that reference at least one test ID:

```
coverage = requirements_with_at_least_one_reference / total_requirements
```

It measures how much of the requirement set is *linked* to tests. It says
nothing about whether those tests exist, run, or pass. Both the JSON and the
human-readable output use the same referenced/linked terminology so they
describe the same evidence level.

## Testing

Integration tests verify replay logic and contract call safety:

```bash
cargo test -p replay
```

## Sample Data

`testdata/reference.ndjson` contains 25 entries across 20 wallets and 5 asset pairs, exercising:
- Empty trade lists
- Single and multi-trade entries
- Repeated wallets across multiple pairs
- Realistic price ranges

## CI Integration

The workflow `.github/workflows/replay-regression.yml` runs the replay harness on every PR to detect regressions in score submissions or contract behavior.
