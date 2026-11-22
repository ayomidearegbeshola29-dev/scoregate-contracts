#!/usr/bin/env bash
# Deploy-time manifest validation for network-specific parameters.
#
# Rejects a deployment whose admin, service, upgrade-delay, cooldown,
# risk-threshold, or schema settings are invalid for the target network,
# before any build or deploy command runs. Meant to be sourced by deploy.sh.
#
# Bounds below mirror the on-chain constraints in
# contracts/scoregate-score/src/constants.rs — keep them in sync.
readonly MANIFEST_MIN_COOLDOWN_SECS=60
readonly MANIFEST_MAX_COOLDOWN_SECS=86400
readonly MANIFEST_MIN_UPGRADE_DELAY_SECS=172800
readonly MANIFEST_MAX_UPGRADE_DELAY_SECS=1209600
readonly MANIFEST_CONTRACT_SCHEMA_VERSION=4

# StrKey version byte for a Stellar account (ed25519 public key).
readonly MANIFEST_STRKEY_ACCOUNT_VERSION=48

# Decodes a base32 (RFC 4648, no padding) string into a space-separated list
# of byte values on stdout. Returns 1 on any invalid character.
manifest_base32_decode() {
  local input="$1" alphabet="ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"
  local -a out=()
  local buffer=0 bits=0 i char idx
  for ((i = 0; i < ${#input}; i++)); do
    char="${input:i:1}"
    idx=$(expr index "$alphabet" "$char")
    if [ "$idx" -eq 0 ]; then
      return 1
    fi
    buffer=$(((buffer << 5) | (idx - 1)))
    bits=$((bits + 5))
    if [ "$bits" -ge 8 ]; then
      bits=$((bits - 8))
      out+=("$(((buffer >> bits) & 0xFF))")
    fi
  done
  echo "${out[*]}"
}

# Computes the CRC16-XModem checksum over the given byte values on stdout.
manifest_crc16_xmodem() {
  local crc=0 byte i
  for byte in "$@"; do
    crc=$((crc ^ (byte << 8)))
    for ((i = 0; i < 8; i++)); do
      if [ $((crc & 0x8000)) -ne 0 ]; then
        crc=$(((crc << 1) ^ 0x1021))
      else
        crc=$((crc << 1))
      fi
      crc=$((crc & 0xFFFF))
    done
  done
  echo "$crc"
}

# Validates a Stellar StrKey account public key (e.g. a service address):
# 56 characters, starting with 'G', using base32 alphabet [A-Z2-7], with a
# valid account version byte and CRC16-XModem checksum. Prints a diagnostic
# to stderr and returns 1 on failure; returns 0 on success.
manifest_is_valid_stellar_address() {
  local address="$1"
  if ! [[ "$address" =~ ^G[A-Z2-7]{55}$ ]]; then
    echo "ERROR: '$address' is not a valid Stellar public key" \
      "(expected 'G' followed by 55 base32 characters)." >&2
    return 1
  fi

  local decoded
  if ! decoded=$(manifest_base32_decode "$address"); then
    echo "ERROR: '$address' contains characters outside the base32 alphabet." >&2
    return 1
  fi

  local -a bytes
  read -r -a bytes <<<"$decoded"
  if [ "${#bytes[@]}" -ne 35 ]; then
    echo "ERROR: '$address' does not decode to a 35-byte StrKey payload." >&2
    return 1
  fi

  if [ "${bytes[0]}" -ne "$MANIFEST_STRKEY_ACCOUNT_VERSION" ]; then
    echo "ERROR: '$address' is not a Stellar account address" \
      "(unsupported StrKey version byte ${bytes[0]}; only account keys are supported)." >&2
    return 1
  fi

  local payload=("${bytes[@]:0:33}")
  local expected actual
  expected=$(manifest_crc16_xmodem "${payload[@]}")
  actual=$((bytes[33] | (bytes[34] << 8)))
  if [ "$expected" -ne "$actual" ]; then
    echo "ERROR: '$address' has an invalid StrKey checksum" \
      "(expected $(printf '0x%04X' "$expected"), got $(printf '0x%04X' "$actual"))." >&2
    return 1
  fi

  return 0
}

manifest_is_valid_rpc_url() {
  [[ "$1" =~ ^https://[^[:space:]]+$ ]]
}

manifest_is_valid_network_passphrase() {
  [ -n "$1" ] && [[ "$1" != *$'\n'* ]]
}

# validate_manifest <network> <manifest-path> <admin-identity> <service-address>
#
# Prints one actionable error per violation to stderr and returns 1 if any
# check fails. Returns 0 and prints nothing if the manifest is valid for
# the given network. Performs no network calls or transactions.
validate_manifest() {
  local network="$1" manifest_path="$2" admin_identity="$3" service_address="$4"
  local errors=0

  if ! command -v jq >/dev/null 2>&1; then
    echo "ERROR: manifest validation requires 'jq' but it is not installed." >&2
    return 1
  fi

  if [ ! -f "$manifest_path" ]; then
    echo "ERROR: deploy manifest not found at '$manifest_path'." >&2
    return 1
  fi

  if ! jq -e --arg net "$network" 'has($net)' "$manifest_path" >/dev/null 2>&1; then
    echo "ERROR: deploy manifest '$manifest_path' has no entry for network '$network'." >&2
    return 1
  fi

  local section
  section=$(jq -c --arg net "$network" '.[$net]' "$manifest_path")

  local delay cooldown threshold schema
  delay=$(jq -r '.upgrade_delay_secs' <<<"$section")
  cooldown=$(jq -r '.cooldown_secs' <<<"$section")
  threshold=$(jq -r '.risk_threshold' <<<"$section")
  schema=$(jq -r '.schema_version' <<<"$section")

  if ! [ "$delay" -ge "$MANIFEST_MIN_UPGRADE_DELAY_SECS" ] 2>/dev/null ||
    ! [ "$delay" -le "$MANIFEST_MAX_UPGRADE_DELAY_SECS" ] 2>/dev/null; then
    echo "ERROR: manifest[$network].upgrade_delay_secs=$delay is out of bounds" \
      "[$MANIFEST_MIN_UPGRADE_DELAY_SECS, $MANIFEST_MAX_UPGRADE_DELAY_SECS]." >&2
    errors=$((errors + 1))
  fi

  if ! [ "$cooldown" -ge "$MANIFEST_MIN_COOLDOWN_SECS" ] 2>/dev/null ||
    ! [ "$cooldown" -le "$MANIFEST_MAX_COOLDOWN_SECS" ] 2>/dev/null; then
    echo "ERROR: manifest[$network].cooldown_secs=$cooldown is out of bounds" \
      "[$MANIFEST_MIN_COOLDOWN_SECS, $MANIFEST_MAX_COOLDOWN_SECS]." >&2
    errors=$((errors + 1))
  fi

  if ! [ "$threshold" -ge 0 ] 2>/dev/null || ! [ "$threshold" -le 100 ] 2>/dev/null; then
    echo "ERROR: manifest[$network].risk_threshold=$threshold is out of bounds [0, 100]." >&2
    errors=$((errors + 1))
  fi

  if [ "$schema" != "$MANIFEST_CONTRACT_SCHEMA_VERSION" ]; then
    echo "ERROR: manifest[$network].schema_version=$schema does not match the" \
      " compiled contract schema version ($MANIFEST_CONTRACT_SCHEMA_VERSION)." \
      "Update the manifest or rebuild against the expected contract version." >&2
    errors=$((errors + 1))
  fi

  if [ -z "$admin_identity" ]; then
    echo "ERROR: admin identity is required." >&2
    errors=$((errors + 1))
  elif [ "$network" = "mainnet" ] && [ "$admin_identity" = "deployer" ]; then
    echo "ERROR: refusing to deploy to mainnet using the default 'deployer' identity." \
      "Pass an explicit admin identity for mainnet deployments." >&2
    errors=$((errors + 1))
  fi

  local rpc_url network_passphrase
  rpc_url=$(jq -r '.RPC_URL // empty' <<<"$section")
  network_passphrase=$(jq -r '.NETWORK_PASSPHRASE // empty' <<<"$section")
  if ! manifest_is_valid_rpc_url "$rpc_url"; then
    echo "ERROR: manifest[$network].RPC_URL must be a non-empty HTTPS URL." >&2
    errors=$((errors + 1))
  fi
  if ! manifest_is_valid_network_passphrase "$network_passphrase"; then
    echo "ERROR: manifest[$network].NETWORK_PASSPHRASE is required." >&2
    errors=$((errors + 1))
  fi

  if ! manifest_is_valid_stellar_address "$service_address"; then
    errors=$((errors + 1))
  fi

  [ "$errors" -eq 0 ]
}
