#!/usr/bin/env bash
# Read-only health check for a deployed scoregate-score contract instance.
#
# Runs only view calls (get_admin, get_service, is_paused, get_version,
# is_service_alive, get_pending_upgrade) — no state-changing invocation is
# ever issued, so this is safe to run against production at any time.
#
# Usage:
#   ./scripts/health_check.sh <network> <contract-id> [source-identity]
#
# Exit codes:
#   0  all checks passed
#   1  one or more checks failed (contract likely unhealthy or unreachable)

set -euo pipefail

NETWORK="${1:?ERROR: network argument is required (e.g. testnet, mainnet)}"
CONTRACT_ID="${2:?ERROR: contract-id argument is required}"
SOURCE_IDENTITY="${3:-health-check}"

FAILURES=0

invoke() {
  soroban contract invoke \
    --id "$CONTRACT_ID" \
    --source "$SOURCE_IDENTITY" \
    --network "$NETWORK" \
    -- \
    "$@"
}

check() {
  local label="$1"
  shift
  local output
  if output=$("$@" 2>&1); then
    echo "PASS  $label: $output"
  else
    echo "FAIL  $label: $output"
    FAILURES=$((FAILURES + 1))
  fi
}

echo "== scoregate-score health check: $CONTRACT_ID on $NETWORK =="

check "admin address"    invoke get_admin
check "service address"  invoke get_service
# Pause state: contract must NOT be paused (is_paused must return false)
PAUSE_OUTPUT=$(invoke is_paused 2>&1) && PAUSE_RC=0 || PAUSE_RC=$?
if [[ "$PAUSE_RC" -ne 0 ]]; then
  echo "FAIL  pause state: $PAUSE_OUTPUT"
  FAILURES=$((FAILURES + 1))
elif [[ "$PAUSE_OUTPUT" == "true" ]]; then
  echo "FAIL  pause state: contract is PAUSED"
  FAILURES=$((FAILURES + 1))
else
  echo "PASS  pause state: $PAUSE_OUTPUT"
fi
check "schema version"   invoke get_version
# Service liveness: is_service_alive returns a boolean, and a successful call that returns false
# means the service is not alive. Only the literal true is healthy; an RPC/contract error or any
# other output is a failure too.
LIVE_OUTPUT=$(invoke is_service_alive 2>&1) && LIVE_RC=0 || LIVE_RC=$?
if [[ "$LIVE_RC" -ne 0 ]]; then
  echo "FAIL  service liveness: $LIVE_OUTPUT"
  FAILURES=$((FAILURES + 1))
elif [[ "$LIVE_OUTPUT" == "true" ]]; then
  echo "PASS  service liveness: $LIVE_OUTPUT"
elif [[ "$LIVE_OUTPUT" == "false" ]]; then
  echo "FAIL  service liveness: service is NOT alive"
  FAILURES=$((FAILURES + 1))
else
  echo "FAIL  service liveness: unexpected output: $LIVE_OUTPUT"
  FAILURES=$((FAILURES + 1))
fi

# A pending upgrade proposal is optional state: its absence (Error::NoPendingUpgrade,
# discriminant #13) is healthy, not a failure — only a genuine RPC/contract error is.
if UPGRADE_OUTPUT=$(invoke get_pending_upgrade 2>&1); then
  echo "INFO  pending upgrade: $UPGRADE_OUTPUT"
else
  case "$UPGRADE_OUTPUT" in
    *NoPendingUpgrade*|*'#13'*)
      echo "PASS  pending upgrade: none"
      ;;
    *)
      echo "FAIL  pending upgrade: $UPGRADE_OUTPUT"
      FAILURES=$((FAILURES + 1))
      ;;
  esac
fi

echo "== $FAILURES check(s) failed =="
[ "$FAILURES" -eq 0 ]
