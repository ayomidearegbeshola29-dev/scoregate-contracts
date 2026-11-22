#!/usr/bin/env bash
set -euo pipefail

INJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$INJECT_DIR/.." && pwd)"

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
CONTRACT_ID="${2:?ERROR: contract-id argument is required}"
ADMIN_IDENTITY="${3:?ERROR: admin-identity argument is required}"
SCENARIO="${4:?ERROR: scenario name is required}"

ADMIN_ADDRESS=$(soroban keys address "$ADMIN_IDENTITY" 2>/dev/null || echo "<ADMIN_ADDRESS>")

run() {
  if [ "$DRY_RUN" = true ]; then
    echo "[dry-run] $*"
  else
    "$@"
  fi
}

log() { echo "==> $*"; }

# invoke <function> [args...]: call a contract function (only printed in a dry run).
invoke() {
  run soroban contract invoke \
    --id "$CONTRACT_ID" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK" \
    -- \
    "$@"
}

# view <function>: print a view call's result with one pair of surrounding double quotes removed.
# The exit status is the call's own.
view() {
  local out
  out=$(soroban contract invoke \
    --id "$CONTRACT_ID" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK" \
    -- \
    "$@" 2>/dev/null) || return 1
  out="${out#\"}"
  out="${out%\"}"
  printf '%s\n' "$out"
}

# ── State capture and restoration ───────────────────────────────────────────
# A scenario that changes governance settings first records them, and an EXIT trap puts them back
# even when a step fails half way. Whatever could not be restored is reported, and the script then
# exits non-zero.

UNRESOLVED=()
ORIGINAL_THRESHOLD=""
THRESHOLD_CHANGED=false
ORIGINAL_EPOCH=""
ORIGINAL_EPOCH_OPEN=""
NEXT_EPOCH=""
EPOCH_REOPEN_PENDING=false

# capture_threshold: remember the service threshold, or stop before changing anything.
capture_threshold() {
  if [ "$DRY_RUN" = true ]; then
    ORIGINAL_THRESHOLD="<original threshold>"
    echo "[dry-run] soroban contract invoke --id $CONTRACT_ID -- get_service_threshold  # remembered for restoration"
    return 0
  fi
  if ! ORIGINAL_THRESHOLD=$(view get_service_threshold) || ! [[ "$ORIGINAL_THRESHOLD" =~ ^[0-9]+$ ]]; then
    echo "ERROR: could not read the current service threshold; nothing was changed." >&2
    exit 1
  fi
  log "Original service threshold: $ORIGINAL_THRESHOLD"
}

# restore_threshold: set the remembered threshold back and verify it.
restore_threshold() {
  local current
  THRESHOLD_CHANGED=false
  if ! invoke set_service_threshold --admin-signers "[$ADMIN_ADDRESS]" --threshold "$ORIGINAL_THRESHOLD"; then
    UNRESOLVED+=("service threshold: restoring $ORIGINAL_THRESHOLD failed; check get_service_threshold")
    return 0
  fi
  if [ "$DRY_RUN" = false ]; then
    current=$(view get_service_threshold) || current="<unreadable>"
    if [ "$current" != "$ORIGINAL_THRESHOLD" ]; then
      UNRESOLVED+=("service threshold: expected $ORIGINAL_THRESHOLD after the restore, found $current")
      return 0
    fi
    log "Service threshold restored and verified: $current"
  fi
}

# capture_epoch: remember the current epoch and whether it is open, or stop before changing anything.
capture_epoch() {
  if [ "$DRY_RUN" = true ]; then
    ORIGINAL_EPOCH="<current epoch>"
    ORIGINAL_EPOCH_OPEN="true"
    NEXT_EPOCH="<current epoch + 1>"
    echo "[dry-run] soroban contract invoke --id $CONTRACT_ID -- get_current_epoch / is_epoch_open  # remembered"
    return 0
  fi
  if ! ORIGINAL_EPOCH=$(view get_current_epoch) || ! [[ "$ORIGINAL_EPOCH" =~ ^[0-9]+$ ]] \
    || ! ORIGINAL_EPOCH_OPEN=$(view is_epoch_open) \
    || { [ "$ORIGINAL_EPOCH_OPEN" != true ] && [ "$ORIGINAL_EPOCH_OPEN" != false ]; }; then
    echo "ERROR: could not read the current epoch state; nothing was changed." >&2
    exit 1
  fi
  NEXT_EPOCH=$((ORIGINAL_EPOCH + 1))
  log "Current epoch: $ORIGINAL_EPOCH (open: $ORIGINAL_EPOCH_OPEN)"
}

# reopen_epoch: open the epoch after the one that was closed, and verify it. If the close never
# took effect (the original epoch is still open) there is nothing to undo.
reopen_epoch() {
  local epoch open
  EPOCH_REOPEN_PENDING=false
  if [ "$DRY_RUN" = false ]; then
    epoch=$(view get_current_epoch) || epoch="<unreadable>"
    open=$(view is_epoch_open) || open="<unreadable>"
    if [ "$epoch" = "$ORIGINAL_EPOCH" ] && [ "$open" = true ]; then
      log "Epoch $ORIGINAL_EPOCH is still open; nothing to reopen"
      return 0
    fi
  fi
  if ! invoke open_epoch --admin-signers "[$ADMIN_ADDRESS]" --epoch-id "$NEXT_EPOCH"; then
    UNRESOLVED+=("epoch: $ORIGINAL_EPOCH was closed and opening epoch $NEXT_EPOCH failed; submissions stay blocked until open_epoch succeeds")
    return 0
  fi
  if [ "$DRY_RUN" = false ]; then
    epoch=$(view get_current_epoch) || epoch="<unreadable>"
    open=$(view is_epoch_open) || open="<unreadable>"
    if [ "$epoch" != "$NEXT_EPOCH" ] || [ "$open" != true ]; then
      UNRESOLVED+=("epoch: expected epoch $NEXT_EPOCH open, found epoch $epoch (open: $open)")
      return 0
    fi
    log "Epoch $NEXT_EPOCH is open and verified"
  fi
}

# restore_state: runs when the script exits for any reason, including a failed step.
restore_state() {
  local status=$?
  trap - EXIT
  set +e
  if [ "$THRESHOLD_CHANGED" = true ]; then
    log "Cleanup: restoring the service threshold"
    restore_threshold
  fi
  if [ "$EPOCH_REOPEN_PENDING" = true ]; then
    log "Cleanup: reopening the epoch"
    reopen_epoch
  fi
  if [ "${#UNRESOLVED[@]}" -gt 0 ]; then
    echo "" >&2
    echo "UNRESOLVED STATE - these settings were NOT restored and need manual attention:" >&2
    printf '  - %s\n' "${UNRESOLVED[@]}" >&2
    [ "$status" -eq 0 ] && status=1
  fi
  exit "$status"
}
trap restore_state EXIT

echo ""
echo "  ╔══════════════════════════════════════════════════╗"
echo "  ║  Failure Injection: $SCENARIO"
echo "  ║  Network: $NETWORK"
echo "  ║  Contract: $CONTRACT_ID"
echo "  ╚══════════════════════════════════════════════════╝"
echo ""

case "$SCENARIO" in
  partial-signer-loss)
    log "Scenario: Partial signer loss (reduced below threshold)"
    log "1. Reducing service threshold to simulate signer loss"
    log "2. Attempting score submission with insufficient signers"
    log "3. Verifying submission is rejected with InsufficientSigners"
    log "4. Restoring threshold and verifying recovery"

    capture_threshold

    log "Setting threshold to 5 (simulating 4 signers lost)"
    THRESHOLD_CHANGED=true
    invoke set_service_threshold --admin-signers "[$ADMIN_ADDRESS]" --threshold 5

    log "Attempting submission with threshold not met"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJ_TEST \
      --score 50 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Restoring original threshold ($ORIGINAL_THRESHOLD)"
    restore_threshold

    log "Scenario complete: partial-signer-loss"
    ;;

  stale-data)
    log "Scenario: Stale data (expired epoch)"
    log "1. Closing the current epoch to block submissions"
    log "2. Attempting score submission (should be rejected)"
    log "3. Opening the next epoch"
    log "4. Verifying submissions resume"
    log "An epoch that is already closed is left as it is; closing a live epoch is only done on an"
    log "isolated deployment (FAILURE_INJECTION_ISOLATED=1), because open_epoch accepts only an ID"
    log "above the current one, so the live epoch cannot be restored, only followed by the next."

    capture_epoch
    if [ "$ORIGINAL_EPOCH_OPEN" = true ]; then
      if [ "$DRY_RUN" = false ] && [ "${FAILURE_INJECTION_ISOLATED:-}" != "1" ]; then
        echo "ERROR: epoch $ORIGINAL_EPOCH is open. Closing it and reopening can only leave epoch $NEXT_EPOCH open, so the live epoch would be replaced." >&2
        echo "       Run this scenario against an isolated deployment and set FAILURE_INJECTION_ISOLATED=1. Nothing was changed." >&2
        exit 1
      fi
      log "Closing current epoch $ORIGINAL_EPOCH"
      EPOCH_REOPEN_PENDING=true
      invoke close_epoch --admin-signers "[$ADMIN_ADDRESS]"
    else
      log "Epoch $ORIGINAL_EPOCH is already closed; leaving it unchanged"
    fi

    log "Attempting submission with closed epoch"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJ_TEST \
      --score 50 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    if [ "$ORIGINAL_EPOCH_OPEN" = true ]; then
      log "Opening epoch $NEXT_EPOCH"
      reopen_epoch
    fi

    log "Scenario complete: stale-data"
    ;;

  replay-attack)
    log "Scenario: Replay attack attempt (reusing old attestation)"
    log "1. Submit a score with a fresh attestation nonce"
    log "2. Attempt to replay the same score (should be rejected by cooldown)"
    log "3. Attempt to replay the same attestation nonce (should be rejected)"

    log "Submitting initial score"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJ_TEST \
      --score 50 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Immediately replaying same data (should be rejected by cooldown)"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJ_TEST \
      --score 50 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Scenario complete: replay-attack"
    ;;

  zero-value)
    log "Scenario: Zero-value submission (score=0, timestamp=0, confidence=0)"
    log "1. Submit with score=0 (should succeed — 0 is valid)"
    log "2. Submit with timestamp=0 (should be rejected with InvalidTimestamp)"
    log "3. Submit with confidence=0 (should succeed — 0 is valid)"

    log "Submitting score=0"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJ_TEST \
      --score 0 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Submitting with timestamp=0 (should be rejected)"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJ_TEST \
      --score 50 \
      --benford-flag false \
      --ml-flag false \
      --timestamp 0 \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Scenario complete: zero-value"
    ;;

  max-value)
    log "Scenario: Maximum-value submission (score=101, confidence=101)"
    log "1. Submit with score=101 (should be rejected with InvalidScore)"
    log "2. Submit with confidence=101 (should be rejected with InvalidConfidence)"

    log "Submitting score=101 (should be rejected)"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJ_TEST \
      --score 101 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Submitting confidence=101 (should be rejected)"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJ_TEST \
      --score 50 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 101 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Scenario complete: max-value"
    ;;

  unauthorized-caller)
    log "Scenario: Unauthorized caller (non-admin, non-service)"
    log "1. Attempt to set pair paused without admin auth"
    log "2. Attempt to pause contract without admin auth"

    log "Attempting unauthorized pair pause"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      set_pair_paused \
      --asset-pair INJ_TEST \
      --paused true 2>&1 || true

    log "Scenario complete: unauthorized-caller"
    ;;

  interrupted-retry)
    log "Scenario: Interrupted retry (submit, fail, retry within cooldown)"
    log "1. Submit a score"
    log "2. Attempt to submit the same score again (cooldown should block)"
    log "3. Wait and verify retry succeeds"

    log "First submission"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJ_TEST \
      --score 50 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Immediate retry (should be rate-limited)"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJ_TEST \
      --score 55 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Scenario complete: interrupted-retry"
    ;;

  *)
    echo "ERROR: Unknown scenario '$SCENARIO'" >&2
    echo "Available scenarios:"
    echo "  partial-signer-loss"
    echo "  stale-data"
    echo "  replay-attack"
    echo "  zero-value"
    echo "  max-value"
    echo "  unauthorized-caller"
    echo "  interrupted-retry"
    exit 1
    ;;
esac

echo ""
log "Failure injection scenario '$SCENARIO' completed."
echo ""