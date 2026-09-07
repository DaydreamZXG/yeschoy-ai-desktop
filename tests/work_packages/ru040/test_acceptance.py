"""Current hotfix build acceptance; historical candidate evidence stays intact."""
from __future__ import annotations

import importlib.util
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("ru034_build_helpers", ROOT / "tests/work_packages/ru034/test_acceptance.py")
assert SPEC and SPEC.loader
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)


def test_hotfix_version_and_production_build(tmp_path: Path) -> None:
    package = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))
    tauri = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
    candidate = json.loads((ROOT / "src-tauri/tauri.candidate.conf.json").read_text(encoding="utf-8"))
    cargo = (ROOT / "src-tauri/Cargo.toml").read_text(encoding="utf-8")
    lock = (ROOT / "src-tauri/Cargo.lock").read_text(encoding="utf-8")
    cargo_package = re.search(r"(?ms)^\[package\]\s*\n(.*?)(?=^\[|\Z)", cargo)
    assert cargo_package
    cargo_version = re.search(r'^version\s*=\s*"([^"]+)"', cargo_package[1], re.M)
    lock_version = re.search(r'(?m)^name = "yeschoy-desktop"\nversion = "([^"]+)"', lock)
    assert cargo_version and lock_version
    assert package["version"] == tauri["version"] == cargo_version[1] == lock_version[1] == "0.4.7"
    assert candidate["bundle"]["createUpdaterArtifacts"] is False
    assert candidate["bundle"]["macOS"]["hardenedRuntime"] is True
    BASE.run("node", str(ROOT / "node_modules/typescript/bin/tsc"), "--noEmit", timeout=300)

    resolved = BASE.modules()
    config = tmp_path / "vite.config.mjs"
    out = tmp_path / "renderer"
    BASE.write_config(config, resolved["react"], {
        "root": str(ROOT / "src"), "base": "./", "cacheDir": str(tmp_path / "vite-cache"),
        "build": {"outDir": str(out), "emptyOutDir": True},
        "resolve": {"alias": {"@": str(ROOT / "src")}},
        "clearScreen": False, "envPrefix": ["VITE_", "TAURI_"],
    })
    BASE.run("node", str(Path(resolved["vite"]).parent / "bin/vite.js"), "build",
        "--config", str(config), "--configLoader", "runner", timeout=600)
    html = (out / "index.html").read_text(encoding="utf-8")
    assert "野菜API 桌面助手" in html
    assets = list((out / "assets").glob("*.js"))
    assert assets
    contents = [asset.read_text(encoding="utf-8") for asset in assets]
    assert any('"0.4.7"' in content for content in contents)
    for content in contents:
        assert "本地设计预览" not in content
        assert "Not available in visual fixture" not in content
