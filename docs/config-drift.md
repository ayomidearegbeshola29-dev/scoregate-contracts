# Configuration drift detection

ScoreGate now defines a stable operator-facing configuration manifest for
deployed `scoregate-score` instances and ships a deterministic drift checker
through the `replay` tool.

## Stable manifest fields

- `contract_version`
- `paused`
- `risk_threshold`
- `jump_threshold`
- `staleness_window`
- `upgrade_delay`
- `cooldown`
- `service_threshold`
- `admin_threshold`
- `consensus_threshold_k`
- `consensus_epsilon`
- `reveal_window`
- `finality_buffer`
- `heartbeat_alert_threshold`
- `oracle_staleness_threshold`

## Workflow

1. Capture the approved manifest in JSON.
2. Query the live deployment and materialize the same JSON object.
3. Run:

```bash
cargo run -p replay --manifest-path tools/replay/Cargo.toml -- \
  config-drift approved.json observed.json
```

   `config-drift --template` prints an empty manifest with every field, and
   `config-drift --help` prints the usage.

4. Treat any `drift`, `missing_observed_field`, `unexpected_observed_field`,
   `unknown_approved_field`, or `unknown_observed_field` entry as an operator
   review item.

## Report status and exit status

Every mandatory field has to be present in both manifests with a real value:
`paused` is a boolean and every other field is an unsigned integer (a JSON
number, or a string of digits for values too large for a JSON number). `null`,
which is what the template contains, is not an observation.

| Report `status` | Meaning | Exit status |
|---|---|---|
| `ok` | Every field is present, valid and equal in both manifests | 0 |
| `drifted` | The manifests are complete but a value differs, or a field is missing from one side | 1 |
| `incomplete` | A field is missing from both manifests, or holds a `null` or wrongly typed value, so it was not verified | 1 |

Two empty manifests are therefore `incomplete`, not `ok`: the extra entries
`missing_in_both_manifests`, `null_approved_value`, `null_observed_value`,
`invalid_approved_value` and `invalid_observed_value` say which field.

A usage error (missing or extra arguments, an unknown option), a file that
cannot be read and malformed JSON exit with status 2 and an error that names the
path. An unknown subcommand is never treated as a replay path: it prints the usage
and exits with status 2.

## Compatibility notes

- No on-chain storage layout changed.
- No contract ABI or event changed.
- The drift checker is off-chain only and reads JSON snapshots.
- The supported manifest field list is exported from the Rust crate so the
  tool and contract documentation stay aligned.
