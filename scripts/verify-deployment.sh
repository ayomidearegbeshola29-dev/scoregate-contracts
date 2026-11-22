#!/usr/bin/env bash
set -euo pipefail

VERIFY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$VERIFY_DIR/.." && pwd)"

DRY_RUN=false
POSITIONAL=()

for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=true ;;
    --help)
      sed -n '3,15p' "$0"
      exit 0
      ;;
    *) POSITIONAL+=("$arg") ;;
  esac
done
set -- "${POSITIONAL[@]+"${POSITIONAL[@]}"}"

NETWORK="${1:?ERROR: network argument is required}"
CONTRACT_ID="${2:?ERROR: contract-id argument is required}"
ADMIN_IDENTITY="${3:?ERROR: admin-identity argument is required}"

ADMIN_ADDRESS=$(soroban keys address "$ADMIN_IDENTITY" 2>/dev/null || echo "<ADMIN_ADDRESS>")

log() {
  local ts
  ts=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
  echo "[$ts] $*"
}

PASS=0
FAIL=0
WARN=0

check() {
  local label="$1"
  local result="$2"
  if [ -n "$result" ] && [ "$result" != "FAIL" ] && [ "$result" != "VERIFY_FAILED" ] && [ "$result" != "SCORE_NOT_FOUND" ]; then
    log "  PASS: $label"
    PASS=$((PASS + 1))
  else
    log "  FAIL: $label"
    FAIL=$((FAIL + 1))
  fi
}

# fail_check <label> <detail>: record a failed check with the reason.
fail_check() {
  log "  FAIL: $1 ($2)"
  FAIL=$((FAIL + 1))
}

# read_value <function> [args...]: call a contract view function, keeping its exit status apart
# from its output. Sets READ_OK (true only when the call succeeded) and READ_VALUE (stdout with one
# pair of surrounding double quotes removed). A failed call never yields a value.
read_value() {
  local out
  if out=$(soroban contract invoke \
    --id "$CONTRACT_ID" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK" \
    -- \
    "$@" 2>/dev/null); then
    READ_OK=true
    READ_VALUE="${out#\"}"
    READ_VALUE="${READ_VALUE%\"}"
  else
    READ_OK=false
    READ_VALUE=""
  fi
}

# check_number_in_range <label> <function> <min> <max>: pass only when the call succeeds and returns
# an unsigned integer within [min, max]. A failed call, malformed output or an out-of-domain value
# fails; nothing is substituted for a failed read.
check_number_in_range() {
  local label="$1" fn="$2" min="$3" max="$4"
  read_value "$fn"
  if [ "$READ_OK" != true ]; then
    fail_check "$label" "$fn call failed"
  elif ! [[ "$READ_VALUE" =~ ^[0-9]{1,18}$ ]]; then
    fail_check "$label" "$fn returned '$READ_VALUE', expected an unsigned integer"
  elif [ "$((10#$READ_VALUE))" -lt "$min" ] || [ "$((10#$READ_VALUE))" -gt "$max" ]; then
    fail_check "$label" "$fn returned $READ_VALUE, expected $min to $max"
  else
    check "$label" "$READ_VALUE"
  fi
}

log "Verifying deployment on $NETWORK (contract=$CONTRACT_ID)"
log "Admin: $ADMIN_ADDRESS"
echo ""

# ── 1. Basic state checks ──────────────────────────────
log "1. Basic state checks"

ADMIN=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_admin 2>/dev/null || echo "VERIFY_FAILED")
if [ "$ADMIN" != "VERIFY_FAILED" ]; then
  check "get_admin returns valid address" "$ADMIN"
else
  check "get_admin returns valid address" "FAIL"
fi

SERVICE=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_service 2>/dev/null || echo "VERIFY_FAILED")
if [ "$SERVICE" != "VERIFY_FAILED" ]; then
  check "get_service returns valid address" "$SERVICE"
else
  check "get_service returns valid address" "FAIL"
fi

# CONTRACT_VERSION is positive; get_version returns a u32.
check_number_in_range "get_version returns non-zero" get_version 1 4294967295

# is_paused returns a boolean. An unreadable or malformed pause state is a failure; only a
# readable true is a (non-critical) warning.
read_value is_paused
if [ "$READ_OK" != true ]; then
  fail_check "Contract pause state is readable" "is_paused call failed"
elif [ "$READ_VALUE" = "false" ]; then
  check "Contract is not paused" "true"
elif [ "$READ_VALUE" = "true" ]; then
  WARN=$((WARN + 1))
  log "  WARN: Contract is paused (is_paused=true)"
else
  fail_check "Contract pause state is readable" "is_paused returned '$READ_VALUE', expected true or false"
fi

# 0 disables the finality buffer and is valid; the setter rejects more than 24 hours.
check_number_in_range "Finality buffer is within its allowed range" get_finality_buffer 0 86400

# MIN_COOLDOWN_SECS..MAX_COOLDOWN_SECS (1 minute to 24 hours).
check_number_in_range "Cooldown is configured" get_cooldown 60 86400

# MIN_UPGRADE_DELAY_SECS..MAX_UPGRADE_DELAY_SECS (48 hours to 14 days).
check_number_in_range "Upgrade delay is configured" get_upgrade_delay 172800 1209600

echo ""

# ── 2. Functional smoke tests ──────────────────────────
log "2. Functional smoke tests"

TEST_WALLET="TEST_WALLET_SMOKE"
TEST_PAIR="SMOKE_TEST_PAIR"
TEST_TIMESTAMP=$(date +%s)

SUBMIT_RESULT=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  submit_score \
  --signers "[]" \
  --wallet "$ADMIN_ADDRESS" \
  --asset-pair "$TEST_PAIR" \
  --score 42 \
  --benford-flag false \
  --ml-flag false \
  --timestamp "$TEST_TIMESTAMP" \
  --confidence 80 \
  --model-version 1 \
  --attestation-input null 2>&1 || echo "SMOKE_SUBMIT_FAILED")

if echo "$SUBMIT_RESULT" | grep -q "SMOKE_SUBMIT_FAILED\|Error"; then
  WARN=$((WARN + 1))
  log "  WARN: Smoke test submission failed (may be expected if attestation required): $SUBMIT_RESULT"
else
  check "Smoke test score submission succeeded" "$SUBMIT_RESULT"
fi

SCORE=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_score \
  --wallet "$ADMIN_ADDRESS" \
  --asset-pair "$TEST_PAIR" 2>/dev/null || echo "SCORE_NOT_FOUND")
if [ "$SCORE" != "SCORE_NOT_FOUND" ] && [ "$SCORE" != "VERIFY_FAILED" ] && [ "$SCORE" != "FAIL" ]; then
  check "Smoke test score is retrievable" "$SCORE"
else
  check "Smoke test score is retrievable" "FAIL"
fi

echo ""

# ── 3. Pending upgrade check ───────────────────────────
log "3. Pending upgrade check"
PENDING_UPGRADE=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_pending_upgrade 2>/dev/null || echo "NO_PENDING_UPGRADE")
if [ "$PENDING_UPGRADE" = "NO_PENDING_UPGRADE" ] || [ -z "$PENDING_UPGRADE" ]; then
  check "No unexpected pending upgrade" "true"
else
  WARN=$((WARN + 1))
  log "  WARN: Pending upgrade exists — may be intentional (governance review): $PENDING_UPGRADE"
fi

echo ""

# ── 4. Service set check ──────────────────────────────
log "4. Service configuration check"
read_value get_service_signers
if [ "$READ_OK" != true ]; then
  fail_check "Service signers are configured" "get_service_signers call failed"
elif [ -z "$READ_VALUE" ] || [ "$READ_VALUE" = "[]" ]; then
  fail_check "Service signers are configured" "get_service_signers returned no signers"
else
  check "Service signers are configured" "$READ_VALUE"
fi

read_value get_service_threshold
if [ "$READ_OK" != true ]; then
  fail_check "Service threshold is set (>0)" "get_service_threshold call failed"
elif ! [[ "$READ_VALUE" =~ ^[0-9]{1,18}$ ]]; then
  fail_check "Service threshold is set (>0)" "get_service_threshold returned '$READ_VALUE', expected an unsigned integer"
elif [ "$((10#$READ_VALUE))" -gt 0 ]; then
  check "Service threshold is set (>0)" "$READ_VALUE"
else
  WARN=$((WARN + 1))
  log "  WARN: Service threshold is 0 — single service key auth is in effect"
fi

echo ""

# ── Summary ────────────────────────────────────────────
echo ""
log "── Verification Summary ──"
log "PASS: $PASS"
log "FAIL: $FAIL"
log "WARN: $WARN"

if [ "$FAIL" -gt 0 ]; then
  log "STATUS: FAILED — $FAIL check(s) failed"
  exit 1
else
  log "STATUS: PASSED — All critical checks passed"
  if [ "$WARN" -gt 0 ]; then
    log "NOTICE: $WARN non-critical warning(s) — review recommended"
  fi
fi
echo ""