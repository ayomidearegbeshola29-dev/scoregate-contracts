#!/usr/bin/env bash
set -euo pipefail

DOC="docs/wasm-size-budget.md"

CONTRACTS=(
  "scoregate-score:scoregate_score"
  "scoregate-aggregator:scoregate_aggregator"
  "mock-amm:mock_amm"
  "mock-lending:mock_lending"
)

FAIL=0

# Parse the documented budget table in docs/wasm-size-budget.md.
# The table maps a Target Contract (crate name) to a Total Binary Size in bytes.
# A missing or malformed budget record is a hard error; we never substitute a default.
parse_budget() {
  local crate="$1"
  local value
  value=$(awk -v crate="$crate" '
    BEGIN { FS = "|" }
    {
      # Skip separator rows like |---|---|
      if ($0 ~ /^[[:space:]]*\|?[[:space:]]*:?-+:?[[:space:]]*(\|[[:space:]]*:?-+:?[[:space:]]*)*\|?[[:space:]]*$/) next
      # Locate the row whose first cell names the crate
      name = $2
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", name)
      if (name != crate) next
      # Extract the numeric byte count from the Total Binary Size cell
      size = $3
      gsub(/,/, "", size)
      if (match(size, /[0-9]+/)) {
        print substr(size, RSTART, RLENGTH)
        exit
      }
    }
  ' "$DOC" 2>/dev/null || true)
  printf '%s' "$value"
}

for entry in "${CONTRACTS[@]}"; do
  CRATE="${entry%%:*}"
  BIN="${entry##*:}"
  WASM_PATH="target/wasm32-unknown-unknown/release/${BIN}.wasm"

  if [[ ! -f "$WASM_PATH" ]]; then
    echo "WASM binary not found at $WASM_PATH. Building..."
    cargo build --target wasm32-unknown-unknown --release -p "$CRATE" --locked
  fi

  ACTUAL=$(wc -c < "$WASM_PATH" | tr -d '[:space:]')

  # Read the explicit documented budget for this contract.
  BUDGET_STR=$(parse_budget "$CRATE")
  if [[ -z "$BUDGET_STR" ]]; then
    echo "❌ ERROR: No documented size budget found for $CRATE in $DOC."
    echo "   Add an explicit Total Binary Size entry for this contract; refusing to substitute a default."
    FAIL=1
    echo ""
    continue
  fi
  if ! [[ "$BUDGET_STR" =~ ^[0-9]+$ ]]; then
    echo "❌ ERROR: Malformed size budget for $CRATE in $DOC: '$BUDGET_STR'."
    FAIL=1
    echo ""
    continue
  fi
  BUDGET=$BUDGET_STR
  TOLERANCE=5

  MAX_ALLOWED=$(awk "BEGIN {print int($BUDGET * (1 + $TOLERANCE / 100))}")

  echo "WASM Size Check ($CRATE):"
  echo "  Actual: $ACTUAL bytes"
  echo "  Budget: $BUDGET bytes"
  echo "  Tolerance: $TOLERANCE%"
  echo "  Max Allowed: $MAX_ALLOWED bytes"

  if [[ "$ACTUAL" -gt "$MAX_ALLOWED" ]]; then
    echo "❌ ERROR: $CRATE WASM size budget exceeded!"
    FAIL=1
  else
    echo "✅ $CRATE WASM size is within budget."
  fi
  echo ""
done

if [[ "$FAIL" -ne 0 ]]; then
  exit 1
fi
