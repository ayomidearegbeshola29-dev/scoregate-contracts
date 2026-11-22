#!/usr/bin/env bash
# check_error_discriminants.sh
#
# Enforces append-only stability for error discriminants across pull requests.
# For every contract error enum in the workspace, verifies that no variant is
# removed, renumbered, or renamed between two git refs.
#
# Usage: check_error_discriminants.sh [base_ref] [head_ref]
#   base_ref defaults to origin/main
#   head_ref defaults to HEAD

set -euo pipefail

BASE_REF="${1:-origin/main}"
HEAD_REF="${2:-HEAD}"

# All workspace files that define #[contracterror] enums with numeric
# discriminants. Add new contract error files here as they are introduced.
ERRORS_PATHS=(
  "contracts/scoregate-score/src/errors.rs"
  "contracts/scoregate-aggregator/src/lib.rs"
  "contracts/mock-amm/src/lib.rs"
  "contracts/mock-lending/src/lib.rs"
)

# Extract "VariantName=discriminant" pairs from a file's contents on stdin.
# Matches lines like `    Foo = 1,` inside a #[contracterror] enum.
extract_discriminants() {
  grep -Eo '^[[:space:]]*[A-Za-z_][A-Za-z0-9_]*[[:space:]]*=[[:space:]]*[0-9]+' \
    | sed -E 's/^[[:space:]]*//; s/[[:space:]]*=[[:space:]]*/=/' \
    | sort
}

status=0

for path in "${ERRORS_PATHS[@]}"; do
  if ! git cat-file -e "${BASE_REF}:${path}" 2>/dev/null; then
    # File did not exist at base ref; nothing to compare against.
    continue
  fi

  if ! git cat-file -e "${HEAD_REF}:${path}" 2>/dev/null; then
    echo "ERROR: ${path} was removed between ${BASE_REF} and ${HEAD_REF}" >&2
    status=1
    continue
  fi

  base_discriminants="$(git show "${BASE_REF}:${path}" | extract_discriminants)"
  head_discriminants="$(git show "${HEAD_REF}:${path}" | extract_discriminants)"

  if [ "${base_discriminants}" = "${head_discriminants}" ]; then
    continue
  fi

  # Report removed or renumbered variants (present at base, missing/changed at head).
  while IFS= read -r line; do
    [ -z "${line}" ] && continue
    if ! grep -qxF "${line}" <<< "${head_discriminants}"; then
      echo "ERROR: ${path}: discriminant '${line}' was removed or renumbered" >&2
      status=1
    fi
  done <<< "${base_discriminants}"

done

if [ "${status}" -ne 0 ]; then
  echo "Error discriminant stability check failed." >&2
  exit 1
fi

echo "Error discriminant stability check passed."
