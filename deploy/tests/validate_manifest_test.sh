#!/usr/bin/env bash
# Regression tests for deploy/validate_manifest.sh
#
# Covers issue #353: Stellar address checksum authentication during manifest
# validation. Valid account keys must pass; checksum-mutated keys, invalid
# version bytes, bad alphabet, and bad length must be rejected with clear
# diagnostics. No network calls are made.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VALIDATOR="${SCRIPT_DIR}/../validate_manifest.sh"

if [[ ! -f "$VALIDATOR" ]]; then
  echo "FAIL: validator not found at $VALIDATOR" >&2
  exit 1
fi

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

PASS=0
FAIL=0

# Known valid Stellar account keys (StrKey version byte 6 << 3 = 48, 'G').
VALID_KEY_1="GBRPYHIL2CI3FNQ4BXLFMNDLFJUNPU2HY3ZMFSHONUCEOASW7QC7OX2H"
VALID_KEY_2="GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"

# Checksum-mutated otherwise well-formed key: last char changed from H to J.
MUTATED_CHECKSUM_KEY="GBRPYHIL2CI3FNQ4BXLFMNDLFJUNPU2HY3ZMFSHONUCEOASW7QC7OX2J"

# Unsupported StrKey version: a valid secret seed (version byte 18 << 3 = 144, 'S').
SECRET_SEED_KEY="SBRPYHIL2CI3FNQ4BXLFMNDLFJUNPU2HY3ZMFSHONUCEOASW7QC7OX2H"

# Bad alphabet: contains '0' which is not in the base32 alphabet.
BAD_ALPHABET_KEY="GBRPYHIL2CI3FNQ4BXLFMNDLFJUNPU2HY3ZMFSHONUCEOASW7QC7OX20"

# Bad length: truncated key.
BAD_LENGTH_KEY="GBRPYHIL2CI3FNQ4BXLFMNDLFJUNPU2HY3ZMFSHONUCEOASW7QC7OX"

write_manifest() {
  local address="$1"
  local out="$2"
  cat > "$out" <<EOF
{
  "version": 1,
  "network": "testnet",
  "contracts": {
    "amm": "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM"
  },
  "admin": "${address}"
}
EOF
}

run_case() {
  local name="$1"
  local address="$2"
  local expect="$3"
  local manifest="${WORKDIR}/${name}.json"
  write_manifest "$address" "$manifest"

  local output status
  set +e
  output="$("$VALIDATOR" "$manifest" 2>&1)"
  status=$?
  set -e

  if [[ "$expect" == "pass" ]]; then
    if [[ $status -eq 0 ]]; then
      echo "PASS: $name (accepted as expected)"
      PASS=$((PASS + 1))
    else
      echo "FAIL: $name (expected accept, got status $status)" >&2
      echo "$output" >&2
      FAIL=$((FAIL + 1))
    fi
  else
    if [[ $status -ne 0 ]]; then
      echo "PASS: $name (rejected as expected)"
      PASS=$((PASS + 1))
    else
      echo "FAIL: $name (expected rejection, got status 0)" >&2
      echo "$output" >&2
      FAIL=$((FAIL + 1))
    fi
  fi
}

run_case "valid_key_1" "$VALID_KEY_1" pass
run_case "valid_key_2" "$VALID_KEY_2" pass
run_case "mutated_checksum" "$MUTATED_CHECKSUM_KEY" fail
run_case "unsupported_version" "$SECRET_SEED_KEY" fail
run_case "bad_alphabet" "$BAD_ALPHABET_KEY" fail
run_case "bad_length" "$BAD_LENGTH_KEY" fail

echo
echo "Results: ${PASS} passed, ${FAIL} failed"

if [[ $FAIL -ne 0 ]]; then
  exit 1
fi
