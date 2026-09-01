"""RU-017 executable repair checks, plus exact signed artifact verification."""
import json
import os
from pathlib import Path
import runpy
import subprocess

ROOT = Path(__file__).resolve().parents[3]


def test_native_selection(tmp_path: Path) -> None:
    target = tmp_path / "selection-tests"
    env = dict(os.environ, RUSTUP_TOOLCHAIN="stable")
    compiled = subprocess.run(["rustc", "--edition=2021", "--test", str(ROOT / "src-tauri/src/tool_selection_core.rs"), "-o", str(target)], env=env, capture_output=True, text=True, timeout=60)
    assert compiled.returncode == 0, compiled.stdout + compiled.stderr
    tested = subprocess.run([str(target)], capture_output=True, text=True, timeout=60)
    assert tested.returncode == 0 and "9 passed; 0 failed; 0 ignored" in tested.stdout, tested.stdout + tested.stderr
    config = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())
    window = config["app"]["windows"][0]
    assert window["titleBarStyle"] == "Transparent" and window["hiddenTitle"] is True
    assert window.get("decorations", True) and window["resizable"]
    assert not window.get("transparent")
    native = (ROOT / "src-tauri/src/window_appearance.rs").read_text()
    assert ".set_theme(" not in native and "run_on_main_thread" in native
    assert "window.label() != \"main\"" in native
    assert "Color(244, 246, 250, 255)" in native and "Color(17, 28, 44, 255)" in native


def test_repair_ui(tmp_path: Path) -> None:
    helper = runpy.run_path(str(ROOT / "tests/work_packages/ru012/test_acceptance.py"))
    helper["run_selected_vitest"]("src/workbench/Repair.test.tsx", tmp_path)


def test_local_artifacts(tmp_path: Path, monkeypatch) -> None:
    # Reuse the exact signed-artifact checks without altering the historical unit.
    source = (ROOT / "tests/work_packages/ru016/test_acceptance.py").read_text()
    source = source.replace("yeschoy-ru016-artifacts.json", "yeschoy-ru017-artifacts.json").replace("release/ru016-local", "release/ru017-local")
    namespace = {"__file__": str(__file__)}
    exec(compile(source, str(__file__), "exec"), namespace)
    namespace["test_local_artifacts"](tmp_path, monkeypatch)
