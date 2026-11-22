use std::collections::BTreeSet;
use std::fs;

/// The variant names declared in `pub enum Error { Name = 1, ... }`, in lower case.
fn error_variants(errors_rs: &str) -> Vec<String> {
    let mut variants = Vec::new();
    let mut in_enum = false;
    for line in errors_rs.lines() {
        let line = line.trim();
        if line.starts_with("pub enum Error {") {
            in_enum = true;
            continue;
        }
        if !in_enum {
            continue;
        }
        if line.starts_with('}') {
            break;
        }
        if let Some((name, discriminant)) = line.split_once('=') {
            if discriminant.trim().trim_end_matches(',').parse::<u32>().is_ok() {
                variants.push(name.trim().to_lowercase());
            }
        }
    }
    variants
}

/// The `<variant>` part of every `fn test_error_<variant>(` in `error_coverage.rs`.
fn covered_variants(error_coverage_rs: &str) -> Vec<String> {
    error_coverage_rs
        .lines()
        .filter_map(|line| line.strip_prefix("fn test_error_"))
        .filter_map(|rest| rest.split('(').next())
        .map(str::to_string)
        .collect()
}

// Build-time guard: fails the build when the set of `test_error_*` functions in error_coverage.rs
// is not the set of Error variants. It compares variant names, not counts, so a duplicated or
// misnamed test cannot hide a missing one. It only reads the sources: the tests themselves, and the
// check that each one asserts its variant, run under `cargo test`.
fn main() {
    println!("cargo:rerun-if-changed=src/errors.rs");
    println!("cargo:rerun-if-changed=src/error_coverage.rs");

    let errors_rs = fs::read_to_string("src/errors.rs").expect("Failed to read errors.rs");
    let coverage_rs =
        fs::read_to_string("src/error_coverage.rs").expect("Failed to read error_coverage.rs");

    let variants = error_variants(&errors_rs);
    let covered = covered_variants(&coverage_rs);

    let variant_set: BTreeSet<&String> = variants.iter().collect();
    let covered_set: BTreeSet<&String> = covered.iter().collect();

    let missing: Vec<_> = variant_set.difference(&covered_set).collect();
    let unknown: Vec<_> = covered_set.difference(&variant_set).collect();
    let duplicated: Vec<_> = covered_set
        .iter()
        .filter(|name| covered.iter().filter(|other| other == *name).count() > 1)
        .collect();

    if !missing.is_empty() || !unknown.is_empty() || !duplicated.is_empty() {
        panic!(
            "error_coverage.rs does not match the Error enum. \
            Variants without a test_error_* function: {:?}. \
            Tests that match no variant: {:?}. \
            Variants with more than one test: {:?}.",
            missing, unknown, duplicated
        );
    }

    println!(
        "cargo:warning=Error coverage: {} variants, each with one test_error_* function (run `cargo test` to execute them)",
        variants.len()
    );
}
