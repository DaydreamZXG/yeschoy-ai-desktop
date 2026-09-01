"""pnpm-compatible scenarios; pytest and child Vitest stay in source-readonly isolation."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[3]


def run_selected_vitest(selector: str, tmp_path: Path) -> None:
    resolved = subprocess.run(
        ["node", "--input-type=module", "-e", "import {createRequire} from 'node:module'; const require=createRequire(import.meta.url); console.log(JSON.stringify({manifest:require.resolve('vitest/package.json'),react:import.meta.resolve('@vitejs/plugin-react')}))"],
        cwd=ROOT, capture_output=True, text=True, check=True, timeout=20,
    )
    modules = json.loads(resolved.stdout)
    manifest = Path(modules["manifest"])
    version = json.loads(manifest.read_text())["version"]
    # The existing pnpm lock and installation must agree; no install/update is performed.
    assert f"vitest@{version}" in (ROOT / "pnpm-lock.yaml").read_text()
    config = tmp_path / "vitest.config.mjs"
    config.write_text(
        f"import react from {json.dumps(modules['react'])};\n"
        "export default " + json.dumps({
            "root": str(ROOT), "cacheDir": str(tmp_path / "vite-cache"),
            "resolve": {"alias": {"@": str(ROOT / "src")}},
            "test": {
                "environment": "jsdom", "globals": True,
                "setupFiles": [str(ROOT / "tests/setupGlobals.ts"), str(ROOT / "tests/setupTests.ts")],
                "include": [selector], "exclude": ["release/**", "work/**", "node_modules/**"],
                "cache": {"dir": str(tmp_path / "vitest-cache")},
            },
        }).removesuffix("}") + ", plugins: [react()]};\n"
    )
    report_path = tmp_path / "vitest.json"
    run = subprocess.run(
        ["node", str(manifest.parent / "vitest.mjs"), "run", selector,
         "--config", str(config), "--reporter=json", "--outputFile", str(report_path)],
        cwd=ROOT, capture_output=True, text=True, check=False, timeout=120,
    )
    assert run.returncode == 0, run.stdout + run.stderr
    report = json.loads(report_path.read_text())
    assert report["success"] is True
    assert report["numFailedTests"] == 0 and report["numPendingTests"] == 0
    assert report.get("numTodoTests", 0) == 0
    results = report["testResults"]
    assert len(results) == 1 and Path(results[0]["name"]).resolve() == ROOT / selector
    assertions = results[0]["assertionResults"]
    assert assertions and all(item["status"] == "passed" for item in assertions)
    assert report["numTotalTests"] == report["numPassedTests"] == len(assertions)


def test_workbench_ui(tmp_path: Path) -> None:
    run_selected_vitest("src/workbench/Workbench.test.tsx", tmp_path)


def test_catalog_preservation(tmp_path: Path) -> None:
    run_selected_vitest("src/service-catalog/ServiceCatalogPanel.test.tsx", tmp_path)


def test_preview_preservation(tmp_path: Path) -> None:
    run_selected_vitest("src/configuration/preview.test.ts", tmp_path)
