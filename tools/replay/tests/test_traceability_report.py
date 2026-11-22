import json
import subprocess
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]
SCRIPT = REPO_ROOT / "tools" / "replay" / "traceability_report.py"

EXIT_CHECKPOINT_ERROR = 4


def _run_report(tmp_path, checkpoint):
    checkpoint_path = tmp_path / "checkpoint.json"
    checkpoint_path.write_text(json.dumps(checkpoint), encoding="utf-8")
    output_path = tmp_path / "report.md"
    result = subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--checkpoint",
            str(checkpoint_path),
            "--output",
            str(output_path),
        ],
        capture_output=True,
        text=True,
    )
    return result, output_path


def _valid_checkpoint():
    return {
        "last_index": 1,
        "processed_items": [
            {"test_id": "test_alpha", "invariants": [{"name": "inv_a"}]},
            {"test_id": "test_beta", "invariants": []},
        ],
    }


def test_well_formed_checkpoint_succeeds(tmp_path):
    result, output_path = _run_report(tmp_path, _valid_checkpoint())
    assert result.returncode == 0, result.stderr
    assert output_path.exists()


def test_null_processed_item_rejected(tmp_path):
    checkpoint = _valid_checkpoint()
    checkpoint["processed_items"].append(None)
    result, output_path = _run_report(tmp_path, checkpoint)
    assert result.returncode == EXIT_CHECKPOINT_ERROR
    assert result.stderr.strip()
    assert not output_path.exists()


def test_list_processed_item_rejected(tmp_path):
    checkpoint = _valid_checkpoint()
    checkpoint["processed_items"].append(["not", "an", "object"])
    result, output_path = _run_report(tmp_path, checkpoint)
    assert result.returncode == EXIT_CHECKPOINT_ERROR
    assert result.stderr.strip()
    assert not output_path.exists()


def test_malformed_invariants_rejected(tmp_path):
    checkpoint = _valid_checkpoint()
    checkpoint["processed_items"][0]["invariants"] = ["not-an-object"]
    result, output_path = _run_report(tmp_path, checkpoint)
    assert result.returncode == EXIT_CHECKPOINT_ERROR
    assert result.stderr.strip()
    assert not output_path.exists()


def test_non_list_invariants_rejected(tmp_path):
    checkpoint = _valid_checkpoint()
    checkpoint["processed_items"][0]["invariants"] = {"name": "inv_a"}
    result, output_path = _run_report(tmp_path, checkpoint)
    assert result.returncode == EXIT_CHECKPOINT_ERROR
    assert result.stderr.strip()
    assert not output_path.exists()


def test_object_test_id_rejected(tmp_path):
    checkpoint = _valid_checkpoint()
    checkpoint["processed_items"][0]["test_id"] = {"unhashable": True}
    result, output_path = _run_report(tmp_path, checkpoint)
    assert result.returncode == EXIT_CHECKPOINT_ERROR
    assert result.stderr.strip()
    assert not output_path.exists()


def test_negative_index_rejected(tmp_path):
    checkpoint = _valid_checkpoint()
    checkpoint["last_index"] = -1
    result, output_path = _run_report(tmp_path, checkpoint)
    assert result.returncode == EXIT_CHECKPOINT_ERROR
    assert result.stderr.strip()
    assert not output_path.exists()


def test_boolean_index_rejected(tmp_path):
    checkpoint = _valid_checkpoint()
    checkpoint["last_index"] = True
    result, output_path = _run_report(tmp_path, checkpoint)
    assert result.returncode == EXIT_CHECKPOINT_ERROR
    assert result.stderr.strip()
    assert not output_path.exists()
