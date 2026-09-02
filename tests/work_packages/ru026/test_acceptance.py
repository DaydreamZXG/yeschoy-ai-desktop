"""Final read-only verification using Vite's non-bundling config loader."""

import importlib.util
import json
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[3]
PREDECESSOR = ROOT / "tests/work_packages/ru025/test_acceptance.py"
SPEC = importlib.util.spec_from_file_location("ru025_acceptance", PREDECESSOR)
assert SPEC is not None and SPEC.loader is not None
RU025 = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = RU025
SPEC.loader.exec_module(RU025)


def test_native_session_price_and_activation(tmp_path: Path) -> None:
    RU025.test_native_session_price_and_activation(tmp_path)


def test_shared_account_and_setup_ui(tmp_path: Path) -> None:
    RU025.test_shared_account_and_setup_ui(tmp_path)


def test_internal_candidate_build(tmp_path: Path) -> None:
    package = (ROOT / "package.json").read_text(encoding="utf-8")
    tauri = (ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8")
    candidate = (ROOT / "src-tauri/tauri.candidate.conf.json").read_text(
        encoding="utf-8"
    )
    renderer_activation = (ROOT / "src/configuration/activation.ts").read_text(
        encoding="utf-8"
    )

    assert '"version": "0.3.0"' in package
    assert '"version": "0.3.0"' in tauri
    assert '"createUpdaterArtifacts": false' in tauri
    assert '"hardenedRuntime": true' in candidate
    for forbidden in ("apiKey", "refreshToken", "configPath", "backupPath"):
        assert forbidden not in renderer_activation

    RU025.run_checked(["pnpm", "typecheck"], timeout=180)
    resolved = subprocess.run(
        [
            "node",
            "--input-type=module",
            "-e",
            "console.log(import.meta.resolve('@vitejs/plugin-react'))",
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
        timeout=20,
    )
    config = tmp_path / "vite.config.mjs"
    config.write_text(
        f"import react from {json.dumps(resolved.stdout.strip())};\n"
        "export default "
        + json.dumps(
            {
                "root": str(ROOT / "src"),
                "base": "./",
                "build": {
                    "outDir": str(tmp_path / "dist"),
                    "emptyOutDir": True,
                },
                "resolve": {"alias": {"@": str(ROOT / "src")}},
                "clearScreen": False,
                "envPrefix": ["VITE_", "TAURI_"],
            }
        ).removesuffix("}")
        + ", plugins: [react()]};\n",
        encoding="utf-8",
    )
    RU025.run_checked(
        [
            "pnpm",
            "exec",
            "vite",
            "build",
            "--config",
            str(config),
            "--configLoader",
            "runner",
        ],
        timeout=300,
    )
