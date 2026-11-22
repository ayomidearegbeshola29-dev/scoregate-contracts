import json
import subprocess
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "tools" / "replay" / "traceability_report.py"


def _run_report(tmp_path, issues):
    issues_path = tmp_path / "issues.json"
    issues_path.write_text(json.dumps(issues), encoding="utf-8")
    return subprocess.run(
        [sys.executable, str(SCRIPT), "--issues", str(issues_path)],
        capture_output=True,
        text=True,
    )


def _issue(test_ids):
    return {
        "number": 1,
        "title": "Example",
        "state": "closed",
        "test_ids": test_ids,
    }


@pytest.mark.parametrize("bad_ids", [[""], ["   "], [False], [True], ["\t\n"]])
def test_invalid_test_identifiers_fail_validation(tmp_path, bad_ids):
    result = _run_report(tmp_path, [_issue(bad_ids)])
    assert result.returncode == 2, result.stdout + result.stderr


def test_valid_named_and_integer_identifiers_pass(tmp_path):
    result = _run_report(tmp_path, [_issue(["test_named", 42])])
    assert result.returncode == 0, result.stdout + result.stderr


def test_duplicate_identifiers_count_once(tmp_path):
    result = _run_report(
        tmp_path,
        [_issue(["test_named", "test_named", 42, 42])],
    )
    assert result.returncode == 0, result.stdout + result.stderr
    assert "1" in result.stdout
