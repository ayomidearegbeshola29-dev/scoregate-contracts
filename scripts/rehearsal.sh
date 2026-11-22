#!/usr/bin/env bash
# Post-incident replay & reconciliation workflow — testnet rehearsal.
#
# Validates the full incident response lifecycle on an isolated Stellar
# testnet deployment:
#   1. Deploy a fresh contract instance
#   2. Submit sample scores
#   3. Take a state snapshot (pre-incident baseline)
#   4. Simulate an incident (freeze contract)
#   5. Take a second snapshot (post-incident)
#   6. Reconcile the two snapshots
#   7. Verify state checksum
#   8. Unfreeze and verify resumption
#
# Usage:
#   ./scripts/rehearsal.sh [--dry-run]
#
# Prerequisites:
#   - soroban CLI configured with testnet access
#   - Rust toolchain (wasm32-unknown-unknown target)
#   - jq installed

set -euo pipefail

DRY_RUN=false
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=true ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
    *) echo "Usage: $0 [--dry-run]"; exit 1 ;;
  esac
done

# ── Configuration ────────────────────────────────────────────────────────────

NETWORK="${NETWORK:-testnet}"
ADMIN_IDENTITY="${ADMIN_IDENTITY:-rehearsal-admin}"
SERVICE_ADDRESS="${SERVICE_ADDRESS:-$(soroban keys address "$ADMIN_IDENTITY" 2>/dev/null || echo "GBPLP...MISSING")}"

WASM_PATH="target/wasm32-unknown-unknown/release/scoregate_score.wasm"
OPT_WASM_PATH="target/wasm32-unknown-unknown/release/scoregate_score.optimized.wasm"

TIMESTAMP=$(date +%s)
SNAPSHOT_PRE="/tmp/rehearsal-snapshot-pre-${TIMESTAMP}.json"
SNAPSHOT_POST="/tmp/rehearsal-snapshot-post-${TIMESTAMP}.json"
RECONCILIATION_REPORT="/tmp/rehearsal-reconciliation-${TIMESTAMP}.json"

PASS=0
FAIL=0

log()  { echo "[$(date +%H:%M:%S)] $*"; }
pass() { echo "  ✅ $1"; PASS=$((PASS + 1)); }
fail() { echo "  ❌ $1"; FAIL=$((FAIL + 1)); }

run() {
  if [ "$DRY_RUN" = true ]; then
    echo "[dry-run] $*"
  else
    "$@"
  fi
}

# ── Snapshot helpers ─────────────────────────────────────────────────────────

# take_snapshot <file> <label>: call compute_state_checksum and keep the StateSnapshot it returns
# as evidence, normalised to the shape `recovery reconcile` reads (hex roots, numeric counters).
# A call that fails, or output that is not a well-formed snapshot, fails the rehearsal.
take_snapshot() {
  local file="$1" label="$2" output
  if [ "$DRY_RUN" = true ]; then
    echo "[dry-run] soroban contract invoke --id $CONTRACT_ID -- compute_state_checksum  > $file"
    return 0
  fi
  if ! output=$(soroban contract invoke \
    --id "$CONTRACT_ID" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK" \
    -- \
    compute_state_checksum \
    --admin_signers '["'"$ADMIN_ADDRESS"'"]' 2>/dev/null); then
    fail "$label snapshot: compute_state_checksum failed"
    return 0
  fi
  if printf '%s' "$output" | jq -e '
      select(all(.score_root, .config_root, .auth_root; type == "string" and test("^[0-9a-fA-F]{64}$")))
      | {
          score_root, config_root, auth_root,
          entry_count: (.entry_count | tonumber),
          ledger_seq: (.ledger_seq | tonumber),
          timestamp: (.timestamp | tonumber)
        }' > "$file" 2>/dev/null && [ -s "$file" ]; then
    pass "$label snapshot saved: $file"
  else
    rm -f "$file"
    fail "$label snapshot is not a well-formed StateSnapshot (compute_state_checksum returned: $output)"
  fi
}

# reconcile_snapshots: compare the two snapshots with the recovery tool and fail on any
# difference, naming each root or counter that changed. Missing evidence fails too.
reconcile_snapshots() {
  if [ "$DRY_RUN" = true ]; then
    echo "[dry-run] cargo run -q -p recovery -- reconcile $SNAPSHOT_PRE $SNAPSHOT_POST -o $RECONCILIATION_REPORT"
    return 0
  fi
  if [ ! -s "$SNAPSHOT_PRE" ] || [ ! -s "$SNAPSHOT_POST" ]; then
    fail "Cannot reconcile: snapshot evidence is missing ($SNAPSHOT_PRE, $SNAPSHOT_POST)"
    return 0
  fi
  if ! cargo run -q -p recovery -- reconcile "$SNAPSHOT_PRE" "$SNAPSHOT_POST" -o "$RECONCILIATION_REPORT" 2>/dev/null \
    || [ ! -s "$RECONCILIATION_REPORT" ]; then
    fail "recovery reconcile did not produce a report at $RECONCILIATION_REPORT"
    return 0
  fi
  local entry label field
  for entry in "score root:score_roots_match" "config root:config_roots_match" \
    "auth root:auth_roots_match" "entry count:entry_counts_match"; do
    label="${entry%%:*}"
    field="${entry##*:}"
    if jq -e ".${field} == true" "$RECONCILIATION_REPORT" >/dev/null 2>&1; then
      pass "The $label is unchanged between the pre- and post-incident snapshots"
    else
      fail "The $label DIVERGED between the pre- and post-incident snapshots"
    fi
  done
}

# ── Step 0: Build contract ───────────────────────────────────────────────────

log "Building contract WASM..."
run cargo build --target wasm32-unknown-unknown --release -p scoregate-score
run soroban contract optimize --wasm "$WASM_PATH"

# ── Step 1: Deploy ───────────────────────────────────────────────────────────

log "Deploying to $NETWORK..."
if [ "$DRY_RUN" = false ]; then
  CONTRACT_ID=$(soroban contract deploy \
    --wasm "$OPT_WASM_PATH" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK")
  log "Contract deployed: $CONTRACT_ID"
else
  CONTRACT_ID="CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABTF"
fi
pass "Deployed contract: $CONTRACT_ID"

# ── Step 2: Initialize ───────────────────────────────────────────────────────

log "Initializing contract..."
ADMIN_ADDRESS=$(soroban keys address "$ADMIN_IDENTITY" 2>/dev/null || echo "<ADMIN_ADDRESS>")
run soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  initialize \
  --admin "$ADMIN_ADDRESS" \
  --service "$SERVICE_ADDRESS"
pass "Contract initialized"

# ── Step 3: Submit sample scores ─────────────────────────────────────────────

log "Submitting sample scores..."
WALLET_1="$(soroban keys address "$ADMIN_IDENTITY")"
# We just use the admin as a sample wallet for rehearsal
WALLET_2="$(soroban keys address "$ADMIN_IDENTITY")"

run soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  submit_score \
  --signants '[]' \
  --wallet "$WALLET_1" \
  --asset_pair '"XLM_USDC"' \
  --score 42 \
  --benford_flag false \
  --ml_flag false \
  --timestamp 1 \
  --confidence 90 \
  --model_version 1 \
  --attestation_input '~None'

pass "Submitted sample scores"

# ── Step 4: Take pre-incident snapshot ───────────────────────────────────────

log "Taking pre-incident state snapshot..."
run soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_version > /dev/null 2>&1
pass "Query endpoints responsive"

take_snapshot "$SNAPSHOT_PRE" "Pre-incident"

log "Checking compute_state_checksum is accessible..."
run soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  supports_interface \
  --capability '"checksum"' 2>/dev/null | rg -q "true" && pass "checksum interface supported" || fail "checksum interface NOT supported"

# ── Step 5: Freeze contract ──────────────────────────────────────────────────

log "Testing freeze_contract..."
run soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  freeze_contract \
  --admin_signants '["'"$ADMIN_ADDRESS"'"]' 2>/dev/null && pass "freeze_contract succeeded" || fail "freeze_contract failed"

# Verify frozen
log "Verifying is_frozen..."
run soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  is_frozen 2>/dev/null | rg -q "true" && pass "Contract is frozen" || fail "Contract is NOT frozen"

# Verify submit_score is rejected when frozen
log "Verifying submit_score blocked during freeze..."
run soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  submit_score \
  --signants '[]' \
  --wallet "$WALLET_1" \
  --asset_pair '"XLM_USDC"' \
  --score 50 \
  --benford_flag false \
  --ml_flag false \
  --timestamp 2 \
  --confidence 85 \
  --model_version 1 \
  --attestation_input '~None' 2>/dev/null && fail "submit_score was NOT blocked by freeze" || pass "submit_score correctly blocked"

# ── Step 6: Unfreeze ─────────────────────────────────────────────────────────

log "Testing unfreeze_contract..."
run soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  unfreeze_contract \
  --admin_signants '["'"$ADMIN_ADDRESS"'"]' 2>/dev/null && pass "unfreeze_contract succeeded" || fail "unfreeze_contract failed"

run soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  is_frozen 2>/dev/null | rg -q "false" && pass "Contract is unfrozen" || fail "Contract is still frozen"

# ── Step 6b: Post-incident snapshot and reconciliation ───────────────────────

log "Taking post-incident state snapshot..."
take_snapshot "$SNAPSHOT_POST" "Post-incident"

log "Reconciling the pre- and post-incident snapshots..."
reconcile_snapshots

# ── Step 7: Verify export endpoints ──────────────────────────────────────────

log "Checking export interfaces..."
run soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  supports_interface \
  --capability '"export_score"' 2>/dev/null | rg -q "true" && pass "export_score interface supported" || fail "export_score interface NOT supported"

run soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  supports_interface \
  --capability '"snapshot"' 2>/dev/null | rg -q "true" && pass "snapshot interface supported" || fail "snapshot interface NOT supported"

run soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  supports_interface \
  --capability '"freeze"' 2>/dev/null | rg -q "true" && pass "freeze interface supported" || fail "freeze interface NOT supported"

# ── Step 8: Version check ────────────────────────────────────────────────────

log "Verifying contract version..."
VERSION=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_version 2>/dev/null || echo "0")
if [ "$VERSION" = "5" ]; then
  pass "Contract version is 5 (post-incident reconciliation)"
else
  fail "Contract version is $VERSION, expected 5"
fi

# ── Summary ───────────────────────────────────────────────────────────────────

echo ""
echo "  ── Rehearsal Results ──────────────────────────────────"
echo "  Passed: $PASS"
echo "  Failed: $FAIL"
echo "  Contract: $CONTRACT_ID"
echo "  Network:  $NETWORK"
echo "  Snapshots:      $SNAPSHOT_PRE"
echo "                  $SNAPSHOT_POST"
echo "  Reconciliation: $RECONCILIATION_REPORT"
echo "  ───────────────────────────────────────────────────────"
echo ""

if [ "$FAIL" -gt 0 ]; then
  echo "❌ Some checks failed. Review the output above."
  exit 1
else
  echo "✅ All rehearsal checks passed."
fi
