#!/usr/bin/env bash
# Fails when a Criterion benchmark and its Cargo.toml entry disagree.
#
# A file in contracts/scoregate-score/benches/ that calls `criterion_main!` must have a
# `[[bench]]` entry with `harness = false`. Without it Cargo discovers the file on its own and
# runs it with the default test harness, which reports an empty libtest suite instead of the
# Criterion groups. The reverse is checked too: every entry needs its source file.
set -euo pipefail

CRATE_DIR="contracts/scoregate-score"
MANIFEST="${CRATE_DIR}/Cargo.toml"
STATUS=0

# Names of the Criterion benchmarks, from the sources.
mapfile -t SOURCES < <(grep -l 'criterion_main!' "${CRATE_DIR}"/benches/*.rs | xargs -n1 basename | sed 's/\.rs$//' | sort)

# Names of the [[bench]] entries that disable the default harness.
mapfile -t DECLARED < <(
  awk '
    /^\[\[bench\]\]/ { if (name != "" && harness == "false") print name; name = ""; harness = ""; in_bench = 1; next }
    /^\[/            { if (name != "" && harness == "false") print name; name = ""; harness = ""; in_bench = 0 }
    in_bench && /^name[[:space:]]*=/    { gsub(/.*=[[:space:]]*"|"[[:space:]]*$/, ""); name = $0 }
    in_bench && /^harness[[:space:]]*=/ { gsub(/[[:space:]]/, ""); sub(/^harness=/, ""); harness = $0 }
    END { if (name != "" && harness == "false") print name }
  ' "${MANIFEST}" | tr -d '\r' | sort
)

for name in "${SOURCES[@]}"; do
  if ! printf '%s\n' "${DECLARED[@]}" | grep -Fxq "${name}"; then
    echo "benches/${name}.rs uses criterion_main! but has no [[bench]] entry with harness = false in ${MANIFEST}" >&2
    STATUS=1
  fi
done

for name in "${DECLARED[@]}"; do
  if [[ ! -f "${CRATE_DIR}/benches/${name}.rs" ]]; then
    echo "[[bench]] ${name} in ${MANIFEST} has no benches/${name}.rs" >&2
    STATUS=1
  fi
done

if [[ "${STATUS}" -eq 0 ]]; then
  echo "All ${#SOURCES[@]} Criterion benchmarks are declared with harness = false."
fi
exit "${STATUS}"
