import hashlib
import importlib.util
import json
import os
import sys
from pathlib import Path

import pytest


SCRIPT_PATH = Path(__file__).resolve().parents[1] / "fetch_testnet_snapshot.py"


def _load_module():
    spec = importlib.util.spec_from_file_location(
        "fetch_testnet_snapshot", SCRIPT_PATH
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


@pytest.fixture(scope="module")
def snapshot_module():
    return _load_module()


def _hash_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _write_existing_export(path: Path) -> str:
    path.write_text(json.dumps({"records": ["previous-valid-export"]}) + "\n")
    return _hash_file(path)


def _sibling_temp_files(output: Path):
    return [
        p
        for p in output.parent.iterdir()
        if p != output and p.name.startswith(output.name)
    ]


def test_conversion_failure_preserves_existing_export(
    snapshot_module, tmp_path, monkeypatch
):
    output = tmp_path / "snapshot.json"
    original_hash = _write_existing_export(output)

    def boom(*args, **kwargs):
        raise RuntimeError("injected conversion failure")

    monkeypatch.setattr(snapshot_module, "convert_record", boom, raising=False)

    with pytest.raises(RuntimeError):
        snapshot_module.main(["--output", str(output)])

    assert output.exists()
    assert _hash_file(output) == original_hash
    assert _sibling_temp_files(output) == []


def test_write_failure_after_one_record_preserves_existing_export(
    snapshot_module, tmp_path, monkeypatch
):
    output = tmp_path / "snapshot.json"
    original_hash = _write_existing_export(output)

    real_convert = getattr(snapshot_module, "convert_record", None)
    calls = {"count": 0}

    def flaky_convert(record, *args, **kwargs):
        calls["count"] += 1
        if calls["count"] > 1:
            raise OSError("injected write failure after one record")
        if real_convert is not None:
            return real_convert(record, *args, **kwargs)
        return record

    monkeypatch.setattr(
        snapshot_module, "convert_record", flaky_convert, raising=False
    )

    with pytest.raises(OSError):
        snapshot_module.main(["--output", str(output)])

    assert output.exists()
    assert _hash_file(output) == original_hash
    assert _sibling_temp_files(output) == []


def test_successful_export_replaces_existing_export(snapshot_module, tmp_path):
    output = tmp_path / "snapshot.json"
    original_hash = _write_existing_export(output)

    snapshot_module.main(["--output", str(output)])

    assert output.exists()
    assert _hash_file(output) != original_hash
    assert _sibling_temp_files(output) == []
    json.loads(output.read_text())
