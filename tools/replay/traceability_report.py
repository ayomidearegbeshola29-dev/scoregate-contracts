#!/usr/bin/env python3
import sys
import os
import json
import argparse
import time
import tempfile
import hashlib

EXIT_SUCCESS = 0
EXIT_INVALID_ARGS = 1
EXIT_CORRUPT_INPUT = 2
EXIT_TRACEABILITY_GAP = 3
EXIT_CHECKPOINT_ERROR = 4
EXIT_VERSION_MISMATCH = 5

MAX_INPUT_BYTES = 10 * 1024 * 1024
MAX_ITEMS = 5000
MAX_INVARIANTS_PER_ISSUE = 200
MAX_TESTS_PER_INVARIANT = 200
SUPPORTED_INPUT_VERSIONS = {"1.0.0"}
CHECKPOINT_FORMAT_VERSION = 2

STATUS_PASSED = "PASSED"
STATUS_FAILED = "FAILED"
STATUS_NO_EVIDENCE = "NO_EVIDENCE"

# This tool performs mapping-only traceability: it links invariants to
# user-provided test identifiers. It does not execute tests nor ingest test
# results, so reports describe references, never executions or passes.
EVIDENCE_LEVEL = "MAPPING_ONLY"
COVERAGE_METRIC_DESCRIPTION = (
    "coverage_percentage is the share of invariants that reference at least "
    "one test id; it measures mapping coverage, not test execution or passing."
)


def parse_args():
    parser = argparse.ArgumentParser(description="ScoreGate Traceability Generator")
    parser.add_argument("--input", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--format", choices=["json", "human"], default="json")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--resume-checkpoint")
    parser.add_argument("--strict", action="store_true")
    return parser.parse_args()


def fail(exit_code, message):
    sys.stderr.write(message + "\n")
    sys.exit(exit_code)


def normalize_test_id(t_id):
    """Return a normalized test identifier, or None if it is not meaningful.

    Accepts nonblank strings and explicitly supported integers. Booleans are
    rejected (bool is a subclass of int but is not a meaningful test id).
    """
    if isinstance(t_id, bool):
        return None
    if isinstance(t_id, str):
        normalized = t_id.strip()
        return normalized if normalized else None
    if isinstance(t_id, int):
        return str(t_id)
    return None


def validate_issue(item, index):
    if not isinstance(item, dict):
        fail(EXIT_CORRUPT_INPUT, f"Error: item at index {index} is not an object")

    issue_id = item.get("issue_id")
    if not isinstance(issue_id, str) or not issue_id.strip():
        fail(EXIT_CORRUPT_INPUT, f"Error: item at index {index} missing valid 'issue_id'")

    invariants = item.get("invariants")
    if not isinstance(invariants, list):
        fail(EXIT_CORRUPT_INPUT, f"Error: issue '{issue_id}' has invalid 'invariants' field")

    if len(invariants) > MAX_INVARIANTS_PER_ISSUE:
        fail(EXIT_CORRUPT_INPUT, f"Error: issue '{issue_id}' exceeds max invariants of {MAX_INVARIANTS_PER_ISSUE}")

    seen_invariant_ids = set()
    for inv_index, inv in enumerate(invariants):
        if not isinstance(inv, dict):
            fail(EXIT_CORRUPT_INPUT, f"Error: issue '{issue_id}' invariant at index {inv_index} is not an object")

        inv_id = inv.get("invariant_id")
        if not isinstance(inv_id, str) or not inv_id.strip():
            fail(EXIT_CORRUPT_INPUT, f"Error: issue '{issue_id}' invariant at index {inv_index} missing valid 'invariant_id'")

        if inv_id in seen_invariant_ids:
            fail(EXIT_CORRUPT_INPUT, f"Error: issue '{issue_id}' has duplicate invariant_id '{inv_id}'")
        seen_invariant_ids.add(inv_id)

        test_ids = inv.get("test_ids", [])
        if not isinstance(test_ids, list):
            fail(EXIT_CORRUPT_INPUT, f"Error: invariant '{inv_id}' in issue '{issue_id}' has invalid 'test_ids' field")

        if len(test_ids) > MAX_TESTS_PER_INVARIANT:
            fail(EXIT_CORRUPT_INPUT, f"Error: invariant '{inv_id}' in issue '{issue_id}' exceeds max test_ids of {MAX_TESTS_PER_INVARIANT}")

        for t_index, t_id in enumerate(test_ids):
            if normalize_test_id(t_id) is None:
                fail(EXIT_CORRUPT_INPUT, f"Error: invariant '{inv_id}' in issue '{issue_id}' has invalid test_id at index {t_index}")

    return issue_id


def validate_input_file(path):
    if not os.path.exists(path):
        fail(EXIT_CORRUPT_INPUT, f"Error: File not found: {path}")

    file_size = os.path.getsize(path)
    if file_size > MAX_INPUT_BYTES:
        fail(EXIT_CORRUPT_INPUT, f"Error: Input file size {file_size} exceeds limit of {MAX_INPUT_BYTES} bytes")

    try:
        with open(path, "r", encoding="utf-8") as f:
            raw = json.load(f)
    except Exception as e:
        fail(EXIT_CORRUPT_INPUT, f"Error parsing JSON input: {str(e)}")

    if isinstance(raw, dict):
        version = raw.get("schema_version")
        if version not in SUPPORTED_INPUT_VERSIONS:
            fail(EXIT_VERSION_MISMATCH, f"Error: unsupported input schema_version '{version}'")
        data = raw.get("issues")
        if not isinstance(data, list):
            fail(EXIT_CORRUPT_INPUT, "Error: 'issues' field must be a list")
    elif isinstance(raw, list):
        data = raw
    else:
        fail(EXIT_CORRUPT_INPUT, "Error: root input JSON structure must be a list or a versioned object")
        return

    if len(data) > MAX_ITEMS:
        fail(EXIT_CORRUPT_INPUT, f"Error: Exceeded maximum item limit of {MAX_ITEMS}")

    seen_issue_ids = set()
    for index, item in enumerate(data):
        issue_id = validate_issue(item, index)
        if issue_id in seen_issue_ids:
            fail(EXIT_CORRUPT_INPUT, f"Error: duplicate issue_id '{issue_id}' at index {index}")
        seen_issue_ids.add(issue_id)

    return data


def compute_input_fingerprint(data):
    """Deterministic fingerprint of the normalized input.

    Normalizes ordering (issues by issue_id, invariants by invariant_id,
    test_ids sorted) so semantically identical input yields the same
    fingerprint, while any change to invariant IDs or test references
    produces a different fingerprint.
    """
    normalized = []
    for item in sorted(data, key=lambda x: x["issue_id"]):
        invariants = []
        for inv in sorted(item.get("invariants", []), key=lambda x: x["invariant_id"]):
            test_ids = sorted(str(t) for t in inv.get("test_ids", []))
            invariants.append({
                "invariant_id": inv["invariant_id"],
                "test_ids": test_ids,
            })
        normalized.append({
            "issue_id": item["issue_id"],
            "invariants": invariants,
        })
    payload = json.dumps(normalized, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(payload.encode("utf-8")).hexdigest()


def build_report(processed_items, total_invariants, covered_invariants, all_test_ids):
    total_issues = len(processed_items)
    total_tests = len(all_test_ids)
    coverage = (covered_invariants / total_invariants * 100.0) if total_invariants > 0 else 0.0
    if total_invariants == 0:
        status = STATUS_NO_EVIDENCE
    elif covered_invariants == total_invariants:
        status = STATUS_PASSED
    else:
        status = STATUS_FAILED
    return {
        "version": "1.0.0",
        "timestamp": int(time.time()),
        "evidence_level": EVIDENCE_LEVEL,
        "summary": {
            "total_issues": total_issues,
            "total_invariants": total_invariants,
            "total_tests": total_tests,
            "total_tests_referenced": total_tests,
            "coverage_percentage": round(coverage, 2),
            "coverage_metric": COVERAGE_METRIC_DESCRIPTION,
            "status": status
        },
        "items": processed_items
    }


def atomic_write(file_path, content):
    dir_name = os.path.dirname(os.path.abspath(file_path))
    os.makedirs(dir_name, exist_ok=True)
    fd, temp_name = tempfile.mkstemp(dir=dir_name)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as tf:
            tf.write(content)
        os.replace(temp_name, file_path)
    except Exception:
        if os.path.exists(temp_name):
            os.remove(temp_name)
        raise


def generate_human_readable(report):
    lines = []
    lines.append("=== ScoreGate Traceability Report ===")
    lines.append(f"Evidence Level: {report.get('evidence_level', EVIDENCE_LEVEL)} (mapping only; tests are not executed)")
    lines.append(f"Status: {report['summary']['status']}")
    lines.append(f"Coverage: {report['summary']['coverage_percentage']}%")
    lines.append(f"Coverage Metric: {report['summary'].get('coverage_metric', COVERAGE_METRIC_DESCRIPTION)}")
    lines.append(f"Total Issues: {report['summary']['total_issues']}")
    lines.append(f"Total Invariants: {report['summary']['total_invariants']}")
    lines.append(f"Total Tests Referenced: {report['summary']['total_tests']}")
    lines.append("=" * 38)
    for item in report["items"]:
        lines.append(f"\nIssue [{item['issue_id']}]: {item['title']}")
        for inv in item["invariants"]:
            v_str = "LINKED" if inv["verified"] else "NO TEST REFERENCED"
            lines.append(f"  - Invariant [{inv['invariant_id']}]: {inv['description']} ({v_str})")
            for t_id in inv["test_ids"]:
                lines.append(f"      * Referenced Test: {t_id}")
    return "\n".join(lines) + "\n"


def _is_valid_index(value):
    """A checkpoint index must be a non-negative, non-bool integer."""
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


def _validate_checkpoint_item(item, index):
    """Validate a single processed item stored in a checkpoint."""
    if not isinstance(item, dict):
        fail(EXIT_CHECKPOINT_ERROR, f"Error: checkpoint processed_items[{index}] is not an object")

    issue_id = item.get("issue_id")
    if not isinstance(issue_id, str) or not issue_id.strip():
        fail(EXIT_CHECKPOINT_ERROR, f"Error: checkpoint processed_items[{index}] missing valid 'issue_id'")

    invariants = item.get("invariants")
    if not isinstance(invariants, list):
        fail(EXIT_CHECKPOINT_ERROR, f"Error: checkpoint item '{issue_id}' has invalid 'invariants' field")

    for inv_index, inv in enumerate(invariants):
        if not isinstance(inv, dict):
            fail(EXIT_CHECKPOINT_ERROR, f"Error: checkpoint item '{issue_id}' invariant at index {inv_index} is not an object")

        inv_id = inv.get("invariant_id")
        if not isinstance(inv_id, str) or not inv_id.strip():
            fail(EXIT_CHECKPOINT_ERROR, f"Error: checkpoint item '{issue_id}' invariant at index {inv_index} missing valid 'invariant_id'")

        test_ids = inv.get("test_ids", [])
        if not isinstance(test_ids, list):
            fail(EXIT_CHECKPOINT_ERROR, f"Error: checkpoint invariant '{inv_id}' in item '{issue_id}' has invalid 'test_ids' field")

        for t_index, t_id in enumerate(test_ids):
            if not isinstance(t_id, (str, int)) or isinstance(t_id, bool):
                fail(EXIT_CHECKPOINT_ERROR, f"Error: checkpoint invariant '{inv_id}' in item '{issue_id}' has invalid test_id at index {t_index}")


def load_checkpoint(path):
    if not path or not os.path.exists(path):
        return None
    try:
        with open(path, "r", encoding="utf-8") as f:
            data = json.load(f)
    except Exception as e:
        fail(EXIT_CHECKPOINT_ERROR, f"Error reading checkpoint: {str(e)}")
        return None
    if not isinstance(data, dict) or "last_index" not in data or "processed_items" not in data:
        fail(EXIT_CHECKPOINT_ERROR, "Error: checkpoint missing required fields")
        return None

    last_index = data["last_index"]
    if not _is_valid_index(last_index):
        fail(EXIT_CHECKPOINT_ERROR, f"Error: checkpoint 'last_index' must be a non-negative integer, got {last_index!r}")

    processed_items = data["processed_items"]
    if not isinstance(processed_items, list):
        fail(EXIT_CHECKPOINT_ERROR, "Error: checkpoint 'processed_items' must be a list")

    if last_index > len(processed_items):
        fail(EXIT_CHECKPOINT_ERROR, f"Error: checkpoint 'last_index' {last_index} exceeds processed_items length {len(processed_items)}")

    for index, item in enumerate(processed_items):
        _validate_checkpoint_item(item, index)

    return data


def main():
    args = parse_args()

    data = validate_input_file(args.input)
    fingerprint = compute_input_fingerprint(data)

    checkpoint = load_checkpoint(args.resume_checkpoint)
    start_index = 0
    processed_items = []
    total_invariants = 0
    covered_invariants = 0
    all_test_ids = set()

    if checkpoint is not None:
        if checkpoint.get("format_version") != CHECKPOINT_FORMAT_VERSION:
            fail(EXIT_CHECKPOINT_ERROR, f"Error: unsupported checkpoint format_version '{checkpoint.get('format_version')}'")
        if checkpoint.get("input_fingerprint") != fingerprint:
            fail(EXIT_CHECKPOINT_ERROR, "Error: checkpoint input fingerprint does not match current input")

        start_index = checkpoint["last_index"]
        processed_items = list(checkpoint["processed_items"])
        for item in processed_items:
            for inv in item["invariants"]:
                total_invariants += 1
                if inv.get("verified"):
                    covered_invariants += 1
                for t_id in inv.get("test_ids", []):
                    all_test_ids.add(t_id)

    for index in range(start_index, len(data)):
        item = data[index]
        issue_id = item["issue_id"]
        title = item.get("title", "")
        processed_invariants = []
        for inv in item.get("invariants", []):
            inv_id = inv["invariant_id"]
            description = inv.get("description", "")
            test_ids = inv.get("test_ids", [])
            verified = len(test_ids) > 0
            total_invariants += 1
            if verified:
                covered_invariants += 1
            for t in test_ids:
                all_test_ids.add(t)
            invariants_out.append({
                "invariant_id": inv_id,
                "description": description,
                "verified": verified,
                "test_ids": list(test_ids),
            })
        processed_items.append({
            "issue_id": issue_id,
            "title": title,
            "invariants": processed_invariants,
        })

    report = build_report(processed_items, total_invariants, covered_invariants, all_test_ids)

    if args.format == "human":
        output = generate_human_readable(report)
    else:
        output = json.dumps(report, indent=2) + "\n"

    if args.dry_run:
        sys.stdout.write(output)
    else:
        atomic_write(args.output, output)

    if args.strict and report["summary"]["status"] != "PASSED":
        fail(EXIT_TRACEABILITY_GAP, "Error: traceability gap detected in strict mode")

    sys.exit(EXIT_SUCCESS)


if __name__ == "__main__":
    sys.exit(main())
