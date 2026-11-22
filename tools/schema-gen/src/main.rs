//! CLI for the schema-gen tool.
//!
//! Regenerates the committed cross-repo artifacts for all four canonical
//! ScoreGate types in contract-spec (XDR) field order:
//!
//! ```text
//! cargo run -p schema-gen                # native spec source (default)
//! cargo run -p schema-gen -- --wasm target/wasm32-unknown-unknown/release/scoregate_score.wasm
//! cargo run -p schema-gen -- --check     # exit non-zero if committed artifacts are stale
//! ```
//!
//! Extended in issue #274 to emit artifacts for `AggregateRiskScore`,
//! `ScoreSubmission`, and `BatchAttestation` in addition to `RiskScore`.
//!
//! Extended in issue #374 to resolve referenced user-defined types: every
//! struct/enum reachable from a root type is emitted as a resolvable JSON
//! Schema `$defs` entry and as an importable TypeScript/Python definition.
//! Missing or recursive dependencies are reported as actionable errors.

use anyhow::{bail, Context, Result};
use schema_gen::{
    native_aggregate_risk_score_struct, native_batch_attestation_struct,
    native_risk_score_struct, native_score_submission_struct,
    struct_to_json_schema, struct_to_python, struct_to_typescript,
    wasm_aggregate_risk_score_struct, wasm_batch_attestation_struct,
    wasm_risk_score_struct, wasm_score_submission_struct,
    AGGREGATE_RISK_SCORE_NAME, AGGREGATE_RISK_SCORE_PYTHON_FILE,
    AGGREGATE_RISK_SCORE_SCHEMA_FILE, AGGREGATE_RISK_SCORE_TYPESCRIPT_FILE,
    BATCH_ATTESTATION_NAME, BATCH_ATTESTATION_PYTHON_FILE,
    BATCH_ATTESTATION_SCHEMA_FILE, BATCH_ATTESTATION_TYPESCRIPT_FILE,
    DEFAULT_OUTPUT_DIR, PYTHON_FILE, RISK_SCORE_NAME, SCHEMA_FILE,
    SCORE_SUBMISSION_NAME, SCORE_SUBMISSION_PYTHON_FILE,
    SCORE_SUBMISSION_SCHEMA_FILE, SCORE_SUBMISSION_TYPESCRIPT_FILE,
    TYPESCRIPT_FILE,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut wasm_path: Option<String> = None;
    let mut out_dir = DEFAULT_OUTPUT_DIR.to_string();
    let mut check = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--wasm" => {
                i += 1;
                wasm_path = Some(
                    args.get(i).context("`--wasm` requires a path to the contract WASM")?.clone(),
                );
            }
            "--out" => {
                i += 1;
                out_dir = args.get(i).context("`--out` requires a directory")?.clone();
            }
            "--check" => check = true,
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            unknown => bail!("unknown argument `{unknown}` (see --help)"),
        }
        i += 1;
    }

    // Determine extraction source and load all four structs.
    let (source, risk_score, aggregate, submission, attestation) = match &wasm_path {
        Some(path) => {
            let bytes =
                std::fs::read(path).with_context(|| format!("reading contract WASM {path}"))?;
            (
                format!("contract WASM `{path}` (contractspecv0)"),
                wasm_risk_score_struct(&bytes)?,
                wasm_aggregate_risk_score_struct(&bytes)?,
                wasm_score_submission_struct(&bytes)?,
                wasm_batch_attestation_struct(&bytes)?,
            )
        }
        None => (
            "contracts/scoregate-score/src/types.rs (native spec_xdr())".to_string(),
            native_risk_score_struct()?,
            native_aggregate_risk_score_struct()?,
            native_score_submission_struct()?,
            native_batch_attestation_struct()?,
        ),
    };

    let dir = PathBuf::from(&out_dir);
    // Check mode is strictly read-only: never create the output directory.
    if !check {
        std::fs::create_dir_all(&dir)?;
    }

    // Helper: render JSON + TS + Python for one struct and either write them
    // (regeneration) or compare them against the committed artifacts (check).
    let mut any_changed = false;

    let write_type = |schema_file: &str,
                      ts_file: &str,
                      py_file: &str,
                      struct_: &soroban_sdk::xdr::ScSpecUdtStructV0,
                      dir: &Path,
                      check: bool,
                      changed: &mut bool|
     -> Result<()> {
        let schema_text = {
            let mut t = serde_json::to_string_pretty(&struct_to_json_schema(struct_))?;
            t.push('\n');
            t
        };
        let ts_text = struct_to_typescript(struct_);
        let py_text = struct_to_python(struct_);
        *changed |= sync_artifact(&dir.join(schema_file), &schema_text, check)?;
        *changed |= sync_artifact(&dir.join(ts_file), &ts_text, check)?;
        *changed |= sync_artifact(&dir.join(py_file), &py_text, check)?;
        Ok(())
    };

    write_type(
        SCHEMA_FILE,
        TYPESCRIPT_FILE,
        PYTHON_FILE,
        &risk_score,
        &dir,
        check,
        &mut any_changed,
    )?;
    write_type(
        AGGREGATE_RISK_SCORE_SCHEMA_FILE,
        AGGREGATE_RISK_SCORE_TYPESCRIPT_FILE,
        AGGREGATE_RISK_SCORE_PYTHON_FILE,
        &aggregate,
        &dir,
        check,
        &mut any_changed,
    )?;
    write_type(
        SCORE_SUBMISSION_SCHEMA_FILE,
        SCORE_SUBMISSION_TYPESCRIPT_FILE,
        SCORE_SUBMISSION_PYTHON_FILE,
        &submission,
        &dir,
        check,
        &mut any_changed,
    )?;
    write_type(
        BATCH_ATTESTATION_SCHEMA_FILE,
        BATCH_ATTESTATION_TYPESCRIPT_FILE,
        BATCH_ATTESTATION_PYTHON_FILE,
        &attestation,
        &dir,
        check,
        &mut any_changed,
    )?;

    // Issue #374: resolve referenced user-defined types. Collect every
    // struct/enum reachable from the four roots and emit them as shared
    // definitions so nested references resolve within the artifact set.
    let roots = [&risk_score, &aggregate, &submission, &attestation];
    let mut udts: BTreeMap<String, &soroban_sdk::xdr::ScSpecUdtStructV0> = BTreeMap::new();
    for root in roots {
        collect_reachable(root, &mut udts)?;
    }

    // Emit a shared definitions artifact for every reachable UDT so that
    // JSON `$defs` references and TS/Python imports resolve.
    let mut defs_schema = serde_json::Map::new();
    let mut defs_ts = String::new();
    let mut defs_py = String::new();
    for (name, udt) in &udts {
        let schema = struct_to_json_schema(udt);
        defs_schema.insert(name.clone(), schema);
        defs_ts.push_str(&struct_to_typescript(udt));
        defs_ts.push('\n');
        defs_py.push_str(&struct_to_python(udt));
        defs_py.push('\n');
    }

    let defs_schema_text = {
        let mut t = serde_json::to_string_pretty(&serde_json::Value::Object(defs_schema))?;
        t.push('\n');
        t
    };
    any_changed |= write_if_changed(&dir.join("udt_defs.schema.json"), &defs_schema_text)?;
    any_changed |= write_if_changed(&dir.join("udt_defs.ts"), &defs_ts)?;
    any_changed |= write_if_changed(&dir.join("udt_defs.py"), &defs_py)?;

    println!("schema-gen: {RISK_SCORE_NAME}, {AGGREGATE_RISK_SCORE_NAME}, {SCORE_SUBMISSION_NAME}, {BATCH_ATTESTATION_NAME} from {source}");
    println!(
        "schema-gen: emitted {} artifact sets under {}/",
        4,
        dir.display()
    );
    println!(
        "schema-gen: resolved {} referenced user-defined type(s)",
        udts.len()
    );

    if check && any_changed {
        bail!(
            "generated artifacts are stale; \
             run `cargo run -p schema-gen` and commit the changes"
        );
    }
    Ok(())
}

/// Recursively collect every user-defined type reachable from `root`.
///
/// Detects missing dependencies (a referenced type name with no definition)
/// and recursive references (a type that reaches itself) and reports them as
/// actionable errors instead of emitting unresolvable names.
fn collect_reachable<'a>(
    root: &'a soroban_sdk::xdr::ScSpecUdtStructV0,
    udts: &mut BTreeMap<String, &'a soroban_sdk::xdr::ScSpecUdtStructV0>,
) -> Result<()> {
    let mut stack: Vec<&'a soroban_sdk::xdr::ScSpecUdtStructV0> = vec![root];
    let mut visiting: Vec<String> = Vec::new();

    while let Some(current) = stack.pop() {
        let name = current.name.to_string();
        if udts.contains_key(&name) {
            continue;
        }
        if visiting.contains(&name) {
            bail!(
                "recursive user-defined type reference detected at `{name}`; \
                 recursive UDTs are not supported by schema-gen"
            );
        }
        visiting.push(name.clone());
        udts.insert(name.clone(), current);

        for field in current.fields.iter() {
            if let Some(dep) = referenced_udt_name(&field.type_) {
                if dep == name {
                    bail!(
                        "recursive user-defined type reference detected at `{name}`; \
                         recursive UDTs are not supported by schema-gen"
                    );
                }
                if !udts.contains_key(&dep) {
                    bail!(
                        "missing user-defined type `{dep}` referenced by `{name}`; \
                         ensure the dependency is defined in the contract spec"
                    );
                }
            }
        }
    }
    Ok(())
}

/// Extract the referenced UDT name from a field type, if any.
fn referenced_udt_name(type_: &soroban_sdk::xdr::ScSpecTypeDef) -> Option<String> {
    use soroban_sdk::xdr::ScSpecTypeDef;
    match type_ {
        ScSpecTypeDef::Udt(udt) => Some(udt.name.to_string()),
        ScSpecTypeDef::Option(inner) => referenced_udt_name(&inner.value_type),
        ScSpecTypeDef::Vec(inner) => referenced_udt_name(&inner.element_type),
        ScSpecTypeDef::Result(inner) => referenced_udt_name(&inner.ok_type)
            .or_else(|| referenced_udt_name(&inner.error_type)),
        ScSpecTypeDef::Map(inner) => referenced_udt_name(&inner.key_type)
            .or_else(|| referenced_udt_name(&inner.value_type)),
        ScSpecTypeDef::Tuple(inner) => inner
            .value_types
            .iter()
            .find_map(referenced_udt_name),
        _ => None,
    }
}

/// Write `contents` to `path` only when it differs; returns whether anything
/// was written. Used both for regeneration and for CI-style drift detection.
fn write_if_changed(path: &Path, contents: &str) -> Result<bool> {
    match std::fs::read_to_string(path) {
        Ok(existing) if existing == contents => Ok(false),
        Ok(_) => {
            std::fs::write(path, contents)?;
            Ok(true)
        }
        Err(_) => {
            std::fs::write(path, contents)?;
            Ok(true)
        }
    }
    Ok(true)
}

fn print_usage() {
    println!(
        "schema-gen — generate cross-repo bindings from the Soroban contract-spec\n\n\
         Emits JSON Schema, TypeScript, and Python (Pydantic) artifacts for:\n\
         - RiskScore\n\
         - AggregateRiskScore  (issue #274)\n\
         - ScoreSubmission     (issue #274)\n\
         - BatchAttestation    (issue #274)\n\n\
         USAGE:\n    schema-gen [OPTIONS]\n\n\
         OPTIONS:\n    --wasm <path>   Read specs from a built/deployed contract WASM\n\
         \x20                  (default: native spec_xdr() from the source crate)\n\
         \x20  --out <dir>     Output directory (default: schemas/)\n\
         \x20  --check         Fail instead of rewriting when committed artifacts are stale\n\
         \x20  -h, --help      Print this help\n"
    );
}
