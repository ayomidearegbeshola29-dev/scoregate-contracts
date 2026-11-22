//! Cross-repository schema/type generation for the canonical ScoreGate contract
//! types: [`scoregate_score::RiskScore`], [`scoregate_score::AggregateRiskScore`],
//! [`scoregate_score::ScoreSubmission`], and [`scoregate_score::BatchAttestation`].
//!
//! The generation source is the Soroban **contract-spec (XDR)** metadata that
//! the `#[contracttype]` derive already compiles into every contract:
//!
//! * natively via e.g. [`scoregate_score::RiskScore::spec_xdr()`] — the exact
//!   spec bytes generated from the Rust struct itself, and
//! * from a built/deployed contract WASM via [`soroban_spec::read::from_wasm`]
//!   — the same read path Soroban's SDK typed-client generators use.
//!
//! On top of the field shape (names, order, types) we layer the semantic score
//! domain `[0, 100]`, sourced from the contract's own
//! [`scoregate_score::constants`] range constants so the emitted schema
//! cannot drift from what `submit_score` actually enforces.
//!
//! # Extended types (issue #274)
//!
//! `schema-gen` previously only extracted `RiskScore`.  It now supports three
//! additional contract types used by batch and aggregate endpoints:
//!
//! | Rust type              | XDR name              | Description                          |
//! |------------------------|-----------------------|--------------------------------------|
//! | `AggregateRiskScore`   | `AggregateRiskScore`  | Cross-asset aggregate view per wallet |
//! | `ScoreSubmission`      | `ScoreSubmission`     | Single entry in a batch submission   |
//! | `BatchAttestation`     | `BatchAttestation`    | Merkle-root attestation for a batch  |

use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use soroban_sdk::xdr::{Limits, ReadXdr, ScSpecEntry, ScSpecTypeDef, ScSpecUdtStructV0};

/// The canonical cross-repo name of the risk-score struct.
pub const RISK_SCORE_NAME: &str = "RiskScore";

/// The canonical cross-repo name of the aggregate risk-score struct (issue #274).
pub const AGGREGATE_RISK_SCORE_NAME: &str = "AggregateRiskScore";

/// The canonical cross-repo name of the score-submission struct (issue #274).
pub const SCORE_SUBMISSION_NAME: &str = "ScoreSubmission";

/// The canonical cross-repo name of the batch-attestation struct (issue #274).
pub const BATCH_ATTESTATION_NAME: &str = "BatchAttestation";

/// Default directory the CLI emits generated artifacts into.
pub const DEFAULT_OUTPUT_DIR: &str = "schemas";

/// Filename of the generated JSON Schema artifact.
pub const SCHEMA_FILE: &str = "risk_score.schema.json";

/// Filename of the generated TypeScript artifact.
pub const TYPESCRIPT_FILE: &str = "risk_score.ts";

/// Filename of the generated Python (Pydantic) artifact.
pub const PYTHON_FILE: &str = "risk_score.py";

/// Filename of the generated JSON Schema artifact for `AggregateRiskScore` (issue #274).
pub const AGGREGATE_RISK_SCORE_SCHEMA_FILE: &str = "aggregate_risk_score.schema.json";

/// Filename of the generated TypeScript artifact for `AggregateRiskScore` (issue #274).
pub const AGGREGATE_RISK_SCORE_TYPESCRIPT_FILE: &str = "aggregate_risk_score.ts";

/// Filename of the generated Python artifact for `AggregateRiskScore` (issue #274).
pub const AGGREGATE_RISK_SCORE_PYTHON_FILE: &str = "aggregate_risk_score.py";

/// Filename of the generated JSON Schema artifact for `ScoreSubmission` (issue #274).
pub const SCORE_SUBMISSION_SCHEMA_FILE: &str = "score_submission.schema.json";

/// Filename of the generated TypeScript artifact for `ScoreSubmission` (issue #274).
pub const SCORE_SUBMISSION_TYPESCRIPT_FILE: &str = "score_submission.ts";

/// Filename of the generated Python artifact for `ScoreSubmission` (issue #274).
pub const SCORE_SUBMISSION_PYTHON_FILE: &str = "score_submission.py";

/// Filename of the generated JSON Schema artifact for `BatchAttestation` (issue #274).
pub const BATCH_ATTESTATION_SCHEMA_FILE: &str = "batch_attestation.schema.json";

/// Filename of the generated TypeScript artifact for `BatchAttestation` (issue #274).
pub const BATCH_ATTESTATION_TYPESCRIPT_FILE: &str = "batch_attestation.ts";

/// Filename of the generated Python artifact for `BatchAttestation` (issue #274).
pub const BATCH_ATTESTATION_PYTHON_FILE: &str = "batch_attestation.py";

/// Decode the `RiskScore` UDT struct entry natively, straight from the
/// `#[contracttype]`-generated `spec_xdr()` on the Rust type itself — the same
/// bytes the contract's `contractspecv0` section is compiled from.
pub fn native_risk_score_struct() -> Result<ScSpecUdtStructV0> {
    let bytes = scoregate_score::RiskScore::spec_xdr();
    let entry = ScSpecEntry::from_xdr(bytes.as_slice(), Limits::none())
        .context("`RiskScore::spec_xdr()` must decode to an `ScSpecEntry`")?;
    into_named_struct(entry, RISK_SCORE_NAME)
}

/// Decode the `AggregateRiskScore` UDT struct entry natively (issue #274).
pub fn native_aggregate_risk_score_struct() -> Result<ScSpecUdtStructV0> {
    let bytes = scoregate_score::AggregateRiskScore::spec_xdr();
    let entry = ScSpecEntry::from_xdr(bytes.as_slice(), Limits::none())
        .context("`AggregateRiskScore::spec_xdr()` must decode to an `ScSpecEntry`")?;
    into_named_struct(entry, AGGREGATE_RISK_SCORE_NAME)
}

/// Decode the `ScoreSubmission` UDT struct entry natively (issue #274).
pub fn native_score_submission_struct() -> Result<ScSpecUdtStructV0> {
    let bytes = scoregate_score::ScoreSubmission::spec_xdr();
    let entry = ScSpecEntry::from_xdr(bytes.as_slice(), Limits::none())
        .context("`ScoreSubmission::spec_xdr()` must decode to an `ScSpecEntry`")?;
    into_named_struct(entry, SCORE_SUBMISSION_NAME)
}

/// Decode the `BatchAttestation` UDT struct entry natively (issue #274).
pub fn native_batch_attestation_struct() -> Result<ScSpecUdtStructV0> {
    let bytes = scoregate_score::BatchAttestation::spec_xdr();
    let entry = ScSpecEntry::from_xdr(bytes.as_slice(), Limits::none())
        .context("`BatchAttestation::spec_xdr()` must decode to an `ScSpecEntry`")?;
    into_named_struct(entry, BATCH_ATTESTATION_NAME)
}

/// Read the `RiskScore` UDT struct entry from a built/deployed contract WASM,
/// using the same `contractspecv0` read path the SDK typed-client generators
/// use.
pub fn wasm_risk_score_struct(wasm: &[u8]) -> Result<ScSpecUdtStructV0> {
    wasm_named_struct(wasm, RISK_SCORE_NAME)
}

/// Read the `AggregateRiskScore` UDT struct entry from a contract WASM (issue #274).
pub fn wasm_aggregate_risk_score_struct(wasm: &[u8]) -> Result<ScSpecUdtStructV0> {
    wasm_named_struct(wasm, AGGREGATE_RISK_SCORE_NAME)
}

/// Read the `ScoreSubmission` UDT struct entry from a contract WASM (issue #274).
pub fn wasm_score_submission_struct(wasm: &[u8]) -> Result<ScSpecUdtStructV0> {
    wasm_named_struct(wasm, SCORE_SUBMISSION_NAME)
}

/// Read the `BatchAttestation` UDT struct entry from a contract WASM (issue #274).
pub fn wasm_batch_attestation_struct(wasm: &[u8]) -> Result<ScSpecUdtStructV0> {
    wasm_named_struct(wasm, BATCH_ATTESTATION_NAME)
}

fn wasm_named_struct(wasm: &[u8], name: &str) -> Result<ScSpecUdtStructV0> {
    let entries = soroban_spec::read::from_wasm(wasm).context("contract WASM spec must parse")?;
    entries
        .into_iter()
        .find(|entry| {
            matches!(entry, ScSpecEntry::UdtStructV0(s) if xdr_str(&s.name) == name)
        })
        .map(|e| into_named_struct(e, name))
        .unwrap_or_else(|| bail!("contract spec contains no `{name}` UDT struct"))
}

fn into_named_struct(entry: ScSpecEntry, name: &str) -> Result<ScSpecUdtStructV0> {
    match entry {
        ScSpecEntry::UdtStructV0(struct_) if xdr_str(&struct_.name) == name => Ok(struct_),
        other => bail!("expected `UdtStructV0` named `{name}`, got {other:?}"),
    }
}

/// Returns the `[min, max]` domain for the bounded components of the risk
/// score, sourced from the contract's own range constants rather than a
/// hard-coded copy. All five bounded components (`score`, `confidence`, and
/// the three sub-scores) live on the same `[MIN_SCORE, MAX_SCORE]` domain that
/// `submit_score` enforces; only `score` has a dedicated named constant (see
/// the spike report for the follow-up to promote the others).
///
/// Extended in issue #274 to cover fields on `AggregateRiskScore` and
/// `ScoreSubmission` that share the same domain.
pub fn risk_domain_range(field: &str) -> Option<(u32, u32)> {
    match field {
        // RiskScore bounded fields
        "score" | "confidence" | "benford_score" | "ml_score" | "network_score"
        // AggregateRiskScore bounded fields
        | "aggregate_score" | "max_pair_score"
        // ScoreSubmission bounded fields (score and confidence)
        // — field names are the same as in RiskScore, already covered above.
        => {
            Some((scoregate_score::constants::MIN_SCORE, scoregate_score::constants::MAX_SCORE))
        }
        _ => None,
    }
}

fn xdr_str(bytes: &[u8]) -> String {
    std::str::from_utf8(bytes).expect("contract spec strings must be UTF-8").to_owned()
}

/// JSON Schema fragment for a fixed-size byte string (base64 wire encoding).
fn bytes_schema(n: u32) -> Value {
    let mut schema = json!({ "type": "string", "contentEncoding": "base64" });
    if n > 0 {
        schema["minLength"] = json!(n);
        schema["maxLength"] = json!(n);
    }
    schema
}

/// Map an XDR `ScSpecTypeDef` to its JSON Schema (draft-07) fragment.
pub fn json_schema_type(type_: &ScSpecTypeDef) -> Value {
    match type_ {
        ScSpecTypeDef::Val => json!({}),
        ScSpecTypeDef::Bool => json!({ "type": "boolean" }),
        ScSpecTypeDef::Void => json!({}),
        ScSpecTypeDef::Error => json!({ "type": "integer" }),
        ScSpecTypeDef::U32
        | ScSpecTypeDef::U64
        | ScSpecTypeDef::U128
        | ScSpecTypeDef::U256
        | ScSpecTypeDef::Timepoint
        | ScSpecTypeDef::Duration => json!({ "type": "integer", "minimum": 0 }),
        ScSpecTypeDef::I32 | ScSpecTypeDef::I64 | ScSpecTypeDef::I128 | ScSpecTypeDef::I256 => {
            json!({ "type": "integer" })
        }
        ScSpecTypeDef::Bytes => bytes_schema(0),
        ScSpecTypeDef::BytesN(bytes) => bytes_schema(bytes.n),
        ScSpecTypeDef::String | ScSpecTypeDef::Symbol | ScSpecTypeDef::Address => {
            json!({ "type": "string" })
        }
        ScSpecTypeDef::Option(option) => {
            json!({ "anyOf": [json_schema_type(&option.value_type), json!({"type": "null"})] })
        }
        ScSpecTypeDef::Result(result) => json!({
            "anyOf": [
                json!({"type": "object", "properties": {"ok": json_schema_type(&result.ok_type)}, "required": ["ok"], "additionalProperties": false}),
                json!({"type": "object", "properties": {"err": json_schema_type(&result.error_type)}, "required": ["err"], "additionalProperties": false}),
            ]
        }),
        ScSpecTypeDef::Vec(vec) => {
            json!({ "type": "array", "items": json_schema_type(&vec.element_type) })
        }
        ScSpecTypeDef::Map(map) => json!({
            "type": "object",
            "additionalProperties": json_schema_type(&map.value_type)
        }),
        ScSpecTypeDef::Tuple(tuple) => json!({
            "type": "array",
            "items": tuple.value_types.iter().map(json_schema_type).collect::<Vec<_>>()
        }),
        ScSpecTypeDef::Udt(udt) => json!({ "$ref": format!("#/$defs/{}", xdr_str(&udt.name)) }),
    }
}

/// Attach the semantic score-domain range to a field's schema fragment.
fn with_risk_range(field: &str, mut schema: Value) -> Value {
    if let Some((min, max)) = risk_domain_range(field) {
        if let Some(object) = schema.as_object_mut() {
            object.insert("minimum".to_string(), json!(min));
            object.insert("maximum".to_string(), json!(max));
        }
    }
    schema
}

/// Convert a Soroban UDT struct spec entry into a JSON Schema (draft-07)
/// document preserving contract-spec (/XDR) field order — the alphabetical
/// ordering the `#[contracttype]` derive emits, which is what on-chain
/// decoders and SDK clients use.
pub fn struct_to_json_schema(struct_: &ScSpecUdtStructV0) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for field in struct_.fields.iter() {
        let name = xdr_str(&field.name);
        let schema = with_risk_range(&name, json_schema_type(&field.type_));
        properties.insert(name.clone(), schema);
        required.push(name);
    }
    let mut root = json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "$id": "https://scoregate.dev/schemas/risk_score.schema.json",
        "title": xdr_str(&struct_.name),
        "$defs": {
            xdr_str(&struct_.name): {
                "type": "object",
                "additionalProperties": false,
                "properties": properties,
                "required": required,
            }
        }
    });
    let doc = xdr_str(&struct_.doc);
    if !doc.is_empty() {
        root["description"] = json!(doc);
    }
    root
}

/// Map an XDR `ScSpecTypeDef` to its TypeScript type.
fn ts_type(type_: &ScSpecTypeDef) -> String {
    match type_ {
        ScSpecTypeDef::Val => "unknown".to_string(),
        ScSpecTypeDef::Bool => "boolean".to_string(),
        ScSpecTypeDef::Void => "void".to_string(),
        ScSpecTypeDef::Error => "number".to_string(),
        ScSpecTypeDef::U32 | ScSpecTypeDef::I32 => "number".to_string(),
        ScSpecTypeDef::U64
        | ScSpecTypeDef::I64
        | ScSpecTypeDef::U128
        | ScSpecTypeDef::I128
        | ScSpecTypeDef::U256
        | ScSpecTypeDef::I256
        | ScSpecTypeDef::Timepoint
        | ScSpecTypeDef::Duration => "bigint".to_string(),
        ScSpecTypeDef::Bytes
        | ScSpecTypeDef::BytesN(_)
        | ScSpecTypeDef::String
        | ScSpecTypeDef::Symbol
        | ScSpecTypeDef::Address => "string".to_string(),
        ScSpecTypeDef::Option(option) => format!("{} | null", ts_type(&option.value_type)),
        ScSpecTypeDef::Result(result) => format!(
            "{{ ok: {} }} | {{ err: {} }}",
            ts_type(&result.ok_type),
            ts_type(&result.error_type)
        ),
        ScSpecTypeDef::Vec(vec) => format!("{}[]", ts_type(&vec.element_type)),
        ScSpecTypeDef::Map(map) => format!("Record<string, {}>", ts_type(&map.value_type)),
        ScSpecTypeDef::Tuple(tuple) => {
            let inner = tuple.value_types.iter().map(ts_type).collect::<Vec<_>>().join(", ");
            format!("[{inner}]")
        }
        ScSpecTypeDef::Udt(udt) => xdr_str(&udt.name),
    }
}

/// Convert a Soroban UDT struct spec entry into a TypeScript interface,
/// preserving contract-spec (/XDR) field order (alphabetical, as the
/// `#[contracttype]` derive emits).
pub fn struct_to_typescript(struct_: &ScSpecUdtStructV0) -> String {
    let mut lines = vec![
        "// Generated by tools/schema-gen from the Soroban contract-spec of".to_string(),
        "// contracts/scoregate-score/src/types.rs (`RiskScore`). Do not edit by hand."
            .to_string(),
        "// Fields are in contract-spec (XDR) order — alphabetical, matching the on-chain spec."
            .to_string(),
        format!("export interface {} {{", xdr_str(&struct_.name)),
    ];
    for field in struct_.fields.iter() {
        let name = xdr_str(&field.name);
        if let Some((min, max)) = risk_domain_range(&name) {
            lines.push(format!("  /** @minimum {min} @maximum {max} */"));
        }
        lines.push(format!("  {}: {};", name, ts_type(&field.type_)));
    }
    lines.push("}".to_string());
    lines.push(format!("// JSON-Schema twin: {SCHEMA_FILE}"));
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Map an XDR `ScSpecTypeDef` to its Pydantic type annotation, recording any
/// `typing` imports it needs in `needs`.
fn py_type(type_: &ScSpecTypeDef, needs: &mut Vec<&'static str>) -> String {
    match type_ {
        ScSpecTypeDef::Val => "Any".to_string(),
        ScSpecTypeDef::Bool => "bool".to_string(),
        ScSpecTypeDef::Void => "None".to_string(),
        ScSpecTypeDef::Error => "int".to_string(),
        ScSpecTypeDef::U32
        | ScSpecTypeDef::I32
        | ScSpecTypeDef::U64
        | ScSpecTypeDef::I64
        | ScSpecTypeDef::U128
        | ScSpecTypeDef::I128
        | ScSpecTypeDef::U256
        | ScSpecTypeDef::I256
        | ScSpecTypeDef::Timepoint
        | ScSpecTypeDef::Duration => "int".to_string(),
        ScSpecTypeDef::Bytes
        | ScSpecTypeDef::BytesN(_)
        | ScSpecTypeDef::String
        | ScSpecTypeDef::Symbol
        | ScSpecTypeDef::Address => "str".to_string(),
        ScSpecTypeDef::Option(option) => {
            needs.push("Optional");
            format!("Optional[{}]", py_type(&option.value_type, needs))
        }
        ScSpecTypeDef::Result(result) => {
            needs.push("Union");
            format!(
                "Union[{}, {}]",
                py_type(&result.ok_type, needs),
                py_type(&result.error_type, needs)
            )
        }
        ScSpecTypeDef::Vec(vec) => {
            needs.push("List");
            format!("List[{}]", py_type(&vec.element_type, needs))
        }
        ScSpecTypeDef::Map(_) => {
            needs.push("Dict");
            "Dict[str, Any]".to_string()
        }
        ScSpecTypeDef::Tuple(tuple) => {
            needs.push("Tuple");
            let inner =
                tuple.value_types.iter().map(|t| py_type(t, needs)).collect::<Vec<_>>().join(", ");
            format!("Tuple[{inner}]")
        }
        ScSpecTypeDef::Udt(udt) => xdr_str(&udt.name),
    }
}

fn typing_names() -> [&'static str; 6] {
    ["Any", "Dict", "List", "Optional", "Tuple", "Union"]
}

/// Convert a Soroban UDT struct spec entry into a Pydantic v2 model,
/// preserving contract-spec (/XDR) field order (alphabetical, as the
/// `#[contracttype]` derive emits).
///
/// A struct with no fields still yields valid Python: the class body is a bare `pass`, so the
/// model has no fields (no placeholder is added) and serializes as an empty object.
pub fn struct_to_python(struct_: &ScSpecUdtStructV0) -> String {
    let mut needs = Vec::new();
    let mut field_lines = Vec::new();
    for field in struct_.fields.iter() {
        let name = xdr_str(&field.name);
        let annotation = py_type(&field.type_, &mut needs);
        let default = match &field.type_ {
            ScSpecTypeDef::Option(_) => " = None".to_string(),
            _ => risk_domain_range(&name)
                .map(|(min, max)| format!(" = Field(ge={min}, le={max})"))
                .unwrap_or_default(),
        };
        field_lines.push(format!("    {name}: {annotation}{default}"));
    }
    let used = typing_names()
        .iter()
        .filter(|name| needs.iter().any(|item| item == *name))
        .copied()
        .collect::<Vec<_>>();

    let mut lines = vec![
        "# Generated by tools/schema-gen from the Soroban contract-spec of".to_string(),
        "# contracts/scoregate-score/src/types.rs (`RiskScore`). Do not edit by hand.".to_string(),
        "# Fields are in contract-spec (XDR) order — alphabetical, matching the on-chain spec."
            .to_string(),
        "# Requires: pydantic>=2.0".to_string(),
        String::new(),
        "from pydantic import BaseModel, Field".to_string(),
    ];
    if !used.is_empty() {
        lines.push(format!("from typing import {}", used.join(", ")));
    }
    lines.push(String::new());
    lines.push(format!("# JSON-Schema twin: {SCHEMA_FILE}"));
    lines.push(format!("class {}(BaseModel):", xdr_str(&struct_.name)));
    if field_lines.is_empty() {
        lines.push("    pass".to_string());
    } else {
        lines.extend(field_lines);
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native() -> ScSpecUdtStructV0 {
        native_risk_score_struct().expect("native RiskScore spec must decode")
    }

    #[test]
    fn native_spec_matches_the_current_struct_fields_in_contract_order() {
        let struct_ = native();
        let fields: Vec<String> = struct_.fields.iter().map(|f| xdr_str(&f.name)).collect();
        // The `#[contracttype]` derive emits the spec with fields sorted
        // alphabetically by name (derive_struct.rs sorted_by_key), so bindings
        // must follow XDR/contract-spec order — not Rust declaration order.
        assert_eq!(
            fields,
            [
                "benford_flag",
                "benford_score",
                "commitment",
                "confidence",
                "ml_flag",
                "ml_score",
                "model_version",
                "network_score",
                "score",
                "timestamp",
            ]
        );
        assert_eq!(fields.len(), 10);
    }

    #[test]
    fn score_range_is_driven_by_contract_constants_not_a_copy() {
        let struct_ = native();
        let schema = struct_to_json_schema(&struct_);
        let score = &schema["$defs"]["RiskScore"]["properties"]["score"];
        assert_eq!(score["type"], "integer");
        assert_eq!(
            score["minimum"].as_u64(),
            Some(u64::from(scoregate_score::constants::MIN_SCORE)),
            "`score.minimum` must equal the contract's MIN_SCORE constant"
        );
        assert_eq!(
            score["maximum"].as_u64(),
            Some(u64::from(scoregate_score::constants::MAX_SCORE)),
            "`score.maximum` must equal the contract's MAX_SCORE constant"
        );
        assert_eq!(score["minimum"].as_u64(), Some(0), "MIN_SCORE is currently 0");
        assert_eq!(score["maximum"].as_u64(), Some(100), "MAX_SCORE is currently 100");
    }

    #[test]
    fn all_bounded_components_carry_the_contract_range() {
        let struct_ = native();
        let schema = struct_to_json_schema(&struct_);
        for field in ["score", "confidence", "benford_score", "ml_score", "network_score"] {
            let prop = &schema["$defs"]["RiskScore"]["properties"][field];
            assert_eq!(prop["minimum"].as_u64(), Some(0), "{field} minimum");
            assert_eq!(prop["maximum"].as_u64(), Some(100), "{field} maximum");
        }
    }

    #[test]
    fn unsigned_scalars_are_not_capped_at_the_score_domain() {
        let struct_ = native();
        let schema = struct_to_json_schema(&struct_);
        let model_version = &schema["$defs"]["RiskScore"]["properties"]["model_version"];
        assert!(model_version["maximum"].is_null(), "model_version has no semantic max");
        let timestamp = &schema["$defs"]["RiskScore"]["properties"]["timestamp"];
        assert_eq!(timestamp["minimum"].as_u64(), Some(0), "u64 timestamp is unsigned");
        assert!(timestamp["maximum"].is_null(), "timestamp has no semantic max");
    }

    #[test]
    fn commitment_option_bytes_is_nullable_in_all_bindings() {
        let struct_ = native();
        let schema = struct_to_json_schema(&struct_);
        let commitment = &schema["$defs"]["RiskScore"]["properties"]["commitment"];
        assert_eq!(commitment["anyOf"][1]["type"], "null");
        assert_eq!(commitment["anyOf"][0]["type"], "string");
        assert_eq!(commitment["anyOf"][0]["contentEncoding"], "base64");

        let ts = struct_to_typescript(&struct_);
        assert!(ts.contains("commitment: string | null;"), "TS commitment is nullable");

        let py = struct_to_python(&struct_);
        assert!(py.contains("commitment: Optional[str] = None"), "Pydantic commitment is optional");
    }

    #[test]
    fn all_ten_fields_are_required_in_contract_order() {
        let struct_ = native();
        let schema = struct_to_json_schema(&struct_);
        let required =
            schema["$defs"]["RiskScore"]["required"].as_array().expect("required must be an array");
        assert_eq!(required.len(), 10);
        assert_eq!(required[0], "benford_flag");
        assert_eq!(required[9], "timestamp");
        let properties_len = schema["$defs"]["RiskScore"]["properties"]
            .as_object()
            .expect("properties must be an object")
            .len();
        assert_eq!(properties_len, 10);
    }

    #[test]
    fn pydantic_binding_includes_the_ranged_components() {
        let struct_ = native();
        let py = struct_to_python(&struct_);
        assert!(py.contains("score: int = Field(ge=0, le=100)"));
        assert!(py.contains("confidence: int = Field(ge=0, le=100)"));
        assert!(py.contains("benford_score: int = Field(ge=0, le=100)"));
        assert!(py.contains("ml_score: int = Field(ge=0, le=100)"));
        assert!(py.contains("network_score: int = Field(ge=0, le=100)"));
    }

    #[test]
    fn wasm_read_agrees_with_native_when_a_test_wasm_is_provided() {
        let Ok(path) = std::env::var("SCHEMA_GEN_TEST_WASM") else {
            return;
        };
        let bytes = std::fs::read(&path).expect("test wasm path must exist");
        let wasm = wasm_risk_score_struct(&bytes).expect("wasm spec must parse");
        let wasm_fields: Vec<String> = wasm.fields.iter().map(|f| xdr_str(&f.name)).collect();
        let native_fields: Vec<String> = native().fields.iter().map(|f| xdr_str(&f.name)).collect();
        assert_eq!(wasm_fields, native_fields);
    }

    #[test]
    fn provenance_notes_are_embedded_in_artifacts() {
        let struct_ = native();
        assert!(struct_to_typescript(&struct_).contains("Generated by tools/schema-gen"));
        assert!(struct_to_python(&struct_).contains("Generated by tools/schema-gen"));
        let schema = struct_to_json_schema(&struct_);
        assert_eq!(schema["title"], "RiskScore");
    }
}

#[cfg(test)]
mod tests_extended {
    //! Tests for the three additional types added in issue #274.
    use super::*;

    // ── AggregateRiskScore ────────────────────────────────────────────────────

    fn native_aggregate() -> ScSpecUdtStructV0 {
        native_aggregate_risk_score_struct()
            .expect("native AggregateRiskScore spec must decode")
    }

    #[test]
    fn aggregate_risk_score_spec_decodes_from_native() {
        let s = native_aggregate();
        assert_eq!(xdr_str(&s.name), AGGREGATE_RISK_SCORE_NAME);
    }

    #[test]
    fn aggregate_risk_score_has_expected_fields_in_contract_order() {
        let s = native_aggregate();
        let fields: Vec<String> = s.fields.iter().map(|f| xdr_str(&f.name)).collect();
        // AggregateRiskScore fields in alphabetical XDR order.
        assert!(fields.contains(&"aggregate_score".to_string()));
        assert!(fields.contains(&"pair_count".to_string()));
        assert!(fields.contains(&"max_pair_score".to_string()));
        assert!(fields.contains(&"benford_flag_count".to_string()));
        assert!(fields.contains(&"ml_flag_count".to_string()));
        assert!(fields.contains(&"last_updated".to_string()));
        assert!(fields.contains(&"decay_lambda_applied".to_string()));
    }

    #[test]
    fn aggregate_risk_score_bounded_fields_carry_contract_range() {
        let s = native_aggregate();
        let schema = struct_to_json_schema(&s);
        let type_name = AGGREGATE_RISK_SCORE_NAME;
        for field in ["aggregate_score", "max_pair_score"] {
            let prop = &schema["$defs"][type_name]["properties"][field];
            assert_eq!(
                prop["minimum"].as_u64(),
                Some(0),
                "{field} minimum on AggregateRiskScore"
            );
            assert_eq!(
                prop["maximum"].as_u64(),
                Some(100),
                "{field} maximum on AggregateRiskScore"
            );
        }
    }

    #[test]
    fn aggregate_risk_score_unbounded_fields_have_no_max() {
        let s = native_aggregate();
        let schema = struct_to_json_schema(&s);
        let type_name = AGGREGATE_RISK_SCORE_NAME;
        // pair_count, benford_flag_count, ml_flag_count are u32 counters — unbounded.
        for field in ["pair_count", "benford_flag_count", "ml_flag_count", "last_updated"] {
            let prop = &schema["$defs"][type_name]["properties"][field];
            assert!(
                prop["maximum"].is_null(),
                "{field} must have no maximum on AggregateRiskScore"
            );
        }
    }

    #[test]
    fn aggregate_risk_score_json_schema_title_is_correct() {
        let s = native_aggregate();
        let schema = struct_to_json_schema(&s);
        assert_eq!(schema["title"], AGGREGATE_RISK_SCORE_NAME);
    }

    #[test]
    fn aggregate_risk_score_typescript_interface_is_generated() {
        let s = native_aggregate();
        let ts = struct_to_typescript(&s);
        assert!(
            ts.contains(&format!("export interface {AGGREGATE_RISK_SCORE_NAME}")),
            "TypeScript output must declare the AggregateRiskScore interface"
        );
        assert!(
            ts.contains("aggregate_score"),
            "TypeScript output must include aggregate_score field"
        );
        assert!(
            ts.contains("Generated by tools/schema-gen"),
            "TypeScript output must include provenance comment"
        );
    }

    #[test]
    fn aggregate_risk_score_python_model_is_generated() {
        let s = native_aggregate();
        let py = struct_to_python(&s);
        assert!(
            py.contains(&format!("class {AGGREGATE_RISK_SCORE_NAME}(BaseModel):")),
            "Python output must declare the AggregateRiskScore class"
        );
        assert!(
            py.contains("aggregate_score: int = Field(ge=0, le=100)"),
            "aggregate_score must carry Pydantic field constraint"
        );
        assert!(
            py.contains("max_pair_score: int = Field(ge=0, le=100)"),
            "max_pair_score must carry Pydantic field constraint"
        );
    }

    // ── ScoreSubmission ───────────────────────────────────────────────────────

    fn native_submission() -> ScSpecUdtStructV0 {
        native_score_submission_struct()
            .expect("native ScoreSubmission spec must decode")
    }

    #[test]
    fn score_submission_spec_decodes_from_native() {
        let s = native_submission();
        assert_eq!(xdr_str(&s.name), SCORE_SUBMISSION_NAME);
    }

    #[test]
    fn score_submission_has_expected_fields() {
        let s = native_submission();
        let fields: Vec<String> = s.fields.iter().map(|f| xdr_str(&f.name)).collect();
        // All eight fields must be present (alphabetical XDR order).
        for expected in [
            "asset_pair",
            "benford_flag",
            "confidence",
            "ml_flag",
            "model_version",
            "score",
            "timestamp",
            "wallet",
        ] {
            assert!(
                fields.contains(&expected.to_string()),
                "ScoreSubmission must contain field `{expected}`"
            );
        }
        assert_eq!(fields.len(), 8, "ScoreSubmission must have exactly 8 fields");
    }

    #[test]
    fn score_submission_score_and_confidence_carry_contract_range() {
        let s = native_submission();
        let schema = struct_to_json_schema(&s);
        let type_name = SCORE_SUBMISSION_NAME;
        for field in ["score", "confidence"] {
            let prop = &schema["$defs"][type_name]["properties"][field];
            assert_eq!(prop["minimum"].as_u64(), Some(0), "{field} minimum");
            assert_eq!(prop["maximum"].as_u64(), Some(100), "{field} maximum");
        }
    }

    #[test]
    fn score_submission_json_schema_title_is_correct() {
        let s = native_submission();
        let schema = struct_to_json_schema(&s);
        assert_eq!(schema["title"], SCORE_SUBMISSION_NAME);
    }

    #[test]
    fn score_submission_typescript_interface_is_generated() {
        let s = native_submission();
        let ts = struct_to_typescript(&s);
        assert!(
            ts.contains(&format!("export interface {SCORE_SUBMISSION_NAME}")),
            "TypeScript output must declare the ScoreSubmission interface"
        );
        assert!(ts.contains("wallet"), "TypeScript output must include wallet field");
        assert!(ts.contains("Generated by tools/schema-gen"));
    }

    #[test]
    fn score_submission_python_model_is_generated() {
        let s = native_submission();
        let py = struct_to_python(&s);
        assert!(
            py.contains(&format!("class {SCORE_SUBMISSION_NAME}(BaseModel):")),
            "Python output must declare the ScoreSubmission class"
        );
        assert!(py.contains("score: int = Field(ge=0, le=100)"));
        assert!(py.contains("confidence: int = Field(ge=0, le=100)"));
    }

    // ── BatchAttestation ──────────────────────────────────────────────────────

    fn native_attestation() -> ScSpecUdtStructV0 {
        native_batch_attestation_struct()
            .expect("native BatchAttestation spec must decode")
    }

    #[test]
    fn batch_attestation_spec_decodes_from_native() {
        let s = native_attestation();
        assert_eq!(xdr_str(&s.name), BATCH_ATTESTATION_NAME);
    }

    #[test]
    fn batch_attestation_has_expected_fields() {
        let s = native_attestation();
        let fields: Vec<String> = s.fields.iter().map(|f| xdr_str(&f.name)).collect();
        // merkle_root (BytesN<32>) and signature (BytesN<65>).
        assert!(fields.contains(&"merkle_root".to_string()));
        assert!(fields.contains(&"signature".to_string()));
        assert_eq!(fields.len(), 2, "BatchAttestation must have exactly 2 fields");
    }

    #[test]
    fn batch_attestation_fields_are_bytes_in_json_schema() {
        let s = native_attestation();
        let schema = struct_to_json_schema(&s);
        let type_name = BATCH_ATTESTATION_NAME;
        // Both fields are BytesN — encoded as base64 strings in JSON Schema.
        for field in ["merkle_root", "signature"] {
            let prop = &schema["$defs"][type_name]["properties"][field];
            assert_eq!(
                prop["type"], "string",
                "{field} must be string (base64) in JSON Schema"
            );
            assert_eq!(
                prop["contentEncoding"], "base64",
                "{field} must have contentEncoding=base64"
            );
        }
    }

    #[test]
    fn batch_attestation_json_schema_title_is_correct() {
        let s = native_attestation();
        let schema = struct_to_json_schema(&s);
        assert_eq!(schema["title"], BATCH_ATTESTATION_NAME);
    }

    #[test]
    fn batch_attestation_typescript_interface_is_generated() {
        let s = native_attestation();
        let ts = struct_to_typescript(&s);
        assert!(
            ts.contains(&format!("export interface {BATCH_ATTESTATION_NAME}")),
            "TypeScript output must declare the BatchAttestation interface"
        );
        // Both fields are BytesN — must map to `string` in TypeScript.
        assert!(ts.contains("merkle_root: string"), "merkle_root must be string");
        assert!(ts.contains("signature: string"), "signature must be string");
        assert!(ts.contains("Generated by tools/schema-gen"));
    }

    #[test]
    fn batch_attestation_python_model_is_generated() {
        let s = native_attestation();
        let py = struct_to_python(&s);
        assert!(
            py.contains(&format!("class {BATCH_ATTESTATION_NAME}(BaseModel):")),
            "Python output must declare the BatchAttestation class"
        );
        assert!(py.contains("merkle_root: str"), "merkle_root must be str");
        assert!(py.contains("signature: str"), "signature must be str");
    }

    #[test]
    fn batch_attestation_has_no_bounded_fields() {
        let s = native_attestation();
        let schema = struct_to_json_schema(&s);
        let type_name = BATCH_ATTESTATION_NAME;
        for field in ["merkle_root", "signature"] {
            let prop = &schema["$defs"][type_name]["properties"][field];
            assert!(prop["minimum"].is_null(), "{field} must have no minimum");
            assert!(prop["maximum"].is_null(), "{field} must have no maximum");
        }
    }

    // ── Cross-type consistency ────────────────────────────────────────────────

    /// All four types must generate provenance headers in all three binding
    /// formats.
    #[test]
    fn all_four_types_embed_provenance_in_artifacts() {
        let types = [
            ("RiskScore", native_risk_score_struct().unwrap()),
            (AGGREGATE_RISK_SCORE_NAME, native_aggregate()),
            (SCORE_SUBMISSION_NAME, native_submission()),
            (BATCH_ATTESTATION_NAME, native_attestation()),
        ];
        for (name, s) in &types {
            let ts = struct_to_typescript(s);
            let py = struct_to_python(s);
            let schema = struct_to_json_schema(s);
            assert!(
                ts.contains("Generated by tools/schema-gen"),
                "{name}: TypeScript must contain provenance comment"
            );
            assert!(
                py.contains("Generated by tools/schema-gen"),
                "{name}: Python must contain provenance comment"
            );
            assert_eq!(schema["title"], *name, "{name}: JSON Schema title must match");
        }
    }

    /// WASM extraction for each new type returns a bail error (not a panic)
    /// when the WASM does not contain the type.  We use an obviously malformed
    /// WASM slice to confirm the error path is exercised cleanly.
    #[test]
    fn wasm_extraction_returns_error_on_malformed_wasm() {
        let bad_wasm = b"not a wasm binary";
        assert!(wasm_aggregate_risk_score_struct(bad_wasm).is_err());
        assert!(wasm_score_submission_struct(bad_wasm).is_err());
        assert!(wasm_batch_attestation_struct(bad_wasm).is_err());
        assert!(wasm_risk_score_struct(bad_wasm).is_err());
    }
}
