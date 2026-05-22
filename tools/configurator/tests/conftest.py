"""Shared fixtures: spin up the FastAPI app against a temp settings/profiles dir
so each test runs in isolation without touching the developer's real settings."""
import os
import sys
from pathlib import Path

import pytest
import yaml

HERE = Path(__file__).parent.parent
sys.path.insert(0, str(HERE))


def _reset_app_modules():
    """Force re-import of app and its deps so MOBILEOS_CONF_DIR is re-read.
    NOTE: safety is intentionally not reset — its UnsafeInputError class must
    stay identical so test-side `pytest.raises(UnsafeInputError)` matches."""
    for mod in ("app", "auth", "build_runner", "flash_runner"):
        sys.modules.pop(mod, None)


def _make_sandbox(tmp_path: Path, auth_section: dict) -> dict:
    """Create a sandboxed configurator dir; returns paths."""
    profiles_dir  = tmp_path / "profiles"
    artifacts_dir = tmp_path / "artifacts"
    profiles_dir.mkdir()
    artifacts_dir.mkdir()

    settings_file = tmp_path / "settings.yaml"
    settings = {
        "build": {
            "server":       "localhost",
            "base_dir":     str(tmp_path),
            "mobileos_dir": str(tmp_path / "mobileos"),
            "buildroot_dir":str(tmp_path / "buildroot"),
            "output": {"qemu-aarch64": "output-qemu", "zero2w-phone": "output-zero2w"},
        },
        "artifacts": {"dir": str(artifacts_dir)},
        "auth":      auth_section,
    }
    settings_file.write_text(yaml.dump(settings))
    return {
        "settings_file": settings_file,
        "profiles_dir":  profiles_dir,
        "artifacts_dir": artifacts_dir,
        "tmp":           tmp_path,
    }


@pytest.fixture
def tmp_configurator(tmp_path, monkeypatch):
    """Auth disabled — easiest for testing app behaviour."""
    paths = _make_sandbox(tmp_path, {"disabled": True})
    monkeypatch.setenv("MOBILEOS_CONF_DIR", str(tmp_path))
    _reset_app_modules()
    from fastapi.testclient import TestClient
    import app as app_module
    return {
        "client": TestClient(app_module.app),
        "module": app_module,
        **paths,
    }


@pytest.fixture
def tmp_configurator_with_auth(tmp_path, monkeypatch):
    """Auth enabled — admin/secret123."""
    paths = _make_sandbox(tmp_path, {"username": "admin", "password": "secret123"})
    monkeypatch.setenv("MOBILEOS_CONF_DIR", str(tmp_path))
    _reset_app_modules()
    from fastapi.testclient import TestClient
    import app as app_module
    return {
        "client": TestClient(app_module.app),
        "module": app_module,
        **paths,
    }
