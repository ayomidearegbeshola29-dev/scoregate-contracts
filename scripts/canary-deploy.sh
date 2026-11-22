#!/usr/bin/env bash
set -euo pipefail

CANARY_DEPLOY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$CANARY_DEPLOY_DIR/.." && pwd)"

DRY_RUN=false
POSITIONAL=()

for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=true ;;
    --help)
      sed -n '3,25p' "$0"
      exit 0
      ;;
    *) POSITIONAL+=("$arg") ;;
  esac
done
set -- "${POSITIONAL[@]+"${POSITIONAL[@]}"}"

NETWORK="${1:?ERROR: network argument is required (testnet|futurenet)}"
ADMIN_IDENTITY="${2:?ERROR: admin-identity argument is required}"
SERVICE_ADDRESS="${3:?ERROR: service-address argument is required}"
SERVICE_IDENTITY="${4:-$ADMIN_IDENTITY}"

WASM_PATH="$PROJECT_ROOT/target/wasm32-unknown-unknown/release/scoregate_score.wasm"
OPTIMIZED_WASM_PATH="$PROJECT_ROOT/target/wasm32-unknown-unknown/release/scoregate_score.optimized.wasm"

CONTRACT_ID_FILE="$PROJECT_ROOT/.canary-$(echo "$NETWORK" | tr '[:lower:]' '[:upper:]').cid"
LOG_FILE="$PROJECT_ROOT/.canary-$(echo "$NETWORK" | tr '[:lower:]' '[:upper:]').log"

run() {
  if [ "$DRY_RUN" = true ]; then
    echo "[dry-run] $*"
  else
    "$@"
  fi
}

log() {
  local ts
  ts=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
  if [ "$DRY_RUN" = true ]; then
    echo "[$ts] $*"
  else
    echo "[$ts] $*" | tee -a "$LOG_FILE"
  fi
}

# ── Validate network ──────────────────────────────────────────────────
case "$NETWORK" in
  testnet|futurenet) ;;
  *)
    echo "ERROR: canary deployment only supported on testnet and futurenet, not '$NETWORK'." >&2
    exit 1
    ;;
esac

# ── Build ─────────────────────────────────────────────────────────────
log "Building contract for canary deployment (network=$NETWORK)"
run cargo build --target wasm32-unknown-unknown --release -p scoregate-score --locked

log "Optimizing WASM"
run soroban contract optimize --wasm "$WASM_PATH"

# ── Verify WASM ───────────────────────────────────────────────────────
log "Computing WASM hash for canary verification"
if [ "$DRY_RUN" = true ]; then
  CANARY_WASM_HASH="<DRY_RUN_WASM_HASH_PLACEHOLDER>"
else
  CANARY_WASM_HASH=$(sha256sum "$OPTIMIZED_WASM_PATH" | awk '{print $1}')
fi
log "Canary WASM SHA-256: $CANARY_WASM_HASH"

# ── Deploy canary ─────────────────────────────────────────────────────
log "Deploying canary to $NETWORK"
if [ "$DRY_RUN" = true ]; then
  CANARY_CONTRACT_ID="<CANARY_CONTRACT_ID_PLACEHOLDER>"
  echo "[dry-run] soroban contract deploy --wasm $OPTIMIZED_WASM_PATH --source $ADMIN_IDENTITY --network $NETWORK"
else
  CANARY_CONTRACT_ID=$(soroban contract deploy \
    --wasm "$OPTIMIZED_WASM_PATH" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK")
fi

log "Canary contract deployed: $CANARY_CONTRACT_ID"
if [ "$DRY_RUN" != true ]; then
  echo "$CANARY_CONTRACT_ID" > "$CONTRACT_ID_FILE"
fi

# ── Initialize canary ─────────────────────────────────────────────────
if [ "$DRY_RUN" = true ]; then
  ADMIN_ADDRESS="<ADMIN_ADDRESS_PLACEHOLDER>"
  echo "[dry-run] soroban keys address $ADMIN_IDENTITY"
else
  ADMIN_ADDRESS=$(soroban keys address "$ADMIN_IDENTITY" 2>/dev/null || echo "<ADMIN_ADDRESS>")
fi

log "Initializing canary contract (admin=$ADMIN_ADDRESS, service=$SERVICE_ADDRESS)"
run soroban contract invoke \
  --id "$CANARY_CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  initialize \
  --admin "$ADMIN_ADDRESS" \
  --service "$SERVICE_ADDRESS"

# ── Canary verification ──────────────────────────────────────────────
log "Running canary verification suite"

# Verify basic state
if [ "$DRY_RUN" = true ]; then
  echo "[dry-run] soroban contract invoke --id $CANARY_CONTRACT_ID --source $ADMIN_IDENTITY --network $NETWORK -- get_admin"
  CANARY_ADMIN="<ADMIN_ADDRESS_PLACEHOLDER>"
else
  CANARY_ADMIN=$(soroban contract invoke \
    --id "$CANARY_CONTRACT_ID" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK" \
    -- \
    get_admin 2>/dev/null || echo "VERIFICATION_FAILED")
fi

if [ "$CANARY_ADMIN" = "VERIFICATION_FAILED" ]; then
  log "ERROR: Canary verification failed — get_admin returned an error."
  exit 1
fi
log "Canary admin verified: $CANARY_ADMIN"

if [ "$DRY_RUN" = true ]; then
  echo "[dry-run] soroban contract invoke --id $CANARY_CONTRACT_ID --source $ADMIN_IDENTITY --network $NETWORK -- get_version"
  CANARY_VERSION="1"
else
  CANARY_VERSION=$(soroban contract invoke \
    --id "$CANARY_CONTRACT_ID" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK" \
    -- \
    get_version 2>/dev/null || echo "0")
fi
log "Canary contract version: $CANARY_VERSION"

# Submit a canary test score
log "Submitting canary test score"
if [ "$DRY_RUN" = true ]; then
  echo "[dry-run] soroban contract invoke --id $CANARY_CONTRACT_ID --source $ADMIN_IDENTITY --network $NETWORK -- submit_score --signers [] --wallet $ADMIN_ADDRESS --asset-pair CAN_TEST --score 50 --benford-flag false --ml-flag false --timestamp $(date +%s) --confidence 80 --model-version 1 --attestation-input null"
  CANARY_TEST_RESULT="success"
else
  CANARY_TEST_RESULT=$(soroban contract invoke \
    --id "$CANARY_CONTRACT_ID" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK" \
    -- \
    submit_score \
    --signers "[]" \
    --wallet "$ADMIN_ADDRESS" \
    --asset-pair CAN_TEST \
    --score 50 \
    --benford-flag false \
    --ml-flag false \
    --timestamp "$(date +%s)" \
    --confidence 80 \
    --model-version 1 \
    --attestation-input null 2>&1 || echo "CANARY_SUBMIT_FAILED")
fi

if echo "$CANARY_TEST_RESULT" | grep -q "CANARY_SUBMIT_FAILED\|Error"; then
  log "ERROR: Canary test score submission failed: $CANARY_TEST_RESULT"
  exit 1
else
  log "Canary test score submitted successfully"
fi

# Verify the canary score
log "Verifying canary score"
if [ "$DRY_RUN" = true ]; then
  echo "[dry-run] soroban contract invoke --id $CANARY_CONTRACT_ID --source $ADMIN_IDENTITY --network $NETWORK -- get_score --wallet $ADMIN_ADDRESS --asset-pair CAN_TEST"
  CANARY_SCORE="50"
else
  CANARY_SCORE=$(soroban contract invoke \
    --id "$CANARY_CONTRACT_ID" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK" \
    -- \
    get_score \
    --wallet "$ADMIN_ADDRESS" \
    --asset-pair CAN_TEST 2>/dev/null || echo "SCORE_NOT_FOUND")
fi

log "Canary test score result: $CANARY_SCORE"
if [ "$CANARY_SCORE" = "SCORE_NOT_FOUND" ]; then
  log "ERROR: Canary score retrieval failed: SCORE_NOT_FOUND"
  exit 1
fi
if ! echo "$CANARY_SCORE" | grep -q "50"; then
  log "ERROR: Canary score verification failed: expected score 50, got $CANARY_SCORE"
  exit 1
fi

# ── Canary summary ────────────────────────────────────────────────────
echo ""
log "── Canary deployment complete ──"
log "Network:    $NETWORK"
log "Canary ID:  $CANARY_CONTRACT_ID"
log "Admin:      $ADMIN_ADDRESS"
log "Service:    $SERVICE_ADDRESS"
log "WASM Hash:  $CANARY_WASM_HASH"
log "Log file:   $LOG_FILE"
log "────────────────────────────────"
echo ""
if [ "$DRY_RUN" != true ]; then
  log "Canary deployment ID saved to: $CONTRACT_ID_FILE"
fi
log "Review the canary results before proceeding to full production deployment."
log "To promote: ./deploy.sh $NETWORK $ADMIN_IDENTITY $SERVICE_ADDRESS"
echo ""