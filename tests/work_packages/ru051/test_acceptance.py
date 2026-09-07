"""RU-051 recovery evidence. No real app installation or paid model requests."""
from __future__ import annotations

import importlib.util
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("ru051_prior", ROOT / "tests/work_packages/ru046/test_acceptance.py")
assert SPEC and SPEC.loader
PREVIOUS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREVIOUS)
BASE = PREVIOUS.PREVIOUS.PREVIOUS.PREVIOUS.BASE


def test_native_installation(tmp_path: Path) -> None:
    PREVIOUS.test_native_installation(tmp_path)
    output = (tmp_path / "native-tests.log").read_text(encoding="utf-8")
    assert re.search(r"test [^\n]*::installer_mac_identity_and_destination_conflicts_never_fake_presence \.\.\. ok", output)


def test_renderer_installation(tmp_path: Path) -> None:
    PREVIOUS.test_renderer_installation(tmp_path)
    result = json.loads((tmp_path / "vitest.json").read_text(encoding="utf-8"))
    new = [a for f in result["testResults"] for a in f["assertionResults"] if "ru051 installer recovery" in a["fullName"]]
    assert len(new) >= 7 and all(a["status"] == "passed" for a in new)


def test_all_repository_renderer(tmp_path: Path) -> None:
    resolved = BASE.modules()
    config = tmp_path / "full-vitest.config.mjs"
    # Match the repository configuration and its default full-suite discovery.
    # Vite bundles configuration next to the input file, so keep it and all
    # caches in the runner's writable directory, not in read-only source.
    BASE.write_config(config, resolved["react"], {
        "root": str(ROOT), "cacheDir": str(tmp_path / "vite-cache"),
        "resolve": {"alias": {"@": str(ROOT / "src")}},
        "test": {"environment": "jsdom", "globals": True,
            "setupFiles": [str(ROOT / "tests/setupGlobals.ts"), str(ROOT / "tests/setupTests.ts")],
            "coverage": {"reporter": ["text", "lcov"]},
            "cache": {"dir": str(tmp_path / "vitest-cache")}},
    })
    report = tmp_path / "full-vitest.json"
    # Bound CPU contention for long DOM interaction tests on the six-core host.
    # Keep the default 5-second per-test limit and every suite/assertion intact.
    BASE.run("node", str(Path(resolved["vitest"]).parent / "vitest.mjs"), "run",
        "--config", str(config), "--maxWorkers=2", "--minWorkers=1",
        "--reporter=json", "--outputFile", str(report), timeout=1200)
    result = json.loads(report.read_text(encoding="utf-8"))
    assert result["success"] and result["numTotalTests"] >= 1330
    assert result["numTotalTests"] == result["numPassedTests"]
    assert result["numFailedTests"] == result["numPendingTests"] == 0
    names = {Path(f["name"]).name for f in result["testResults"]}
    assert {"App.test.tsx", "PiProviderForm.test.tsx", "useProviderActions.test.tsx", "UnifiedSkillsPanel.test.tsx", "OpenClawProviderActions.test.tsx"} <= names
    journeys = [a for f in result["testResults"] for a in f["assertionResults"] if "ru051 current App integration" in a["fullName"]]
    assert len(journeys) >= 9 and all(a["status"] == "passed" for a in journeys)


def test_installation_build(tmp_path: Path) -> None:
    PREVIOUS.test_installation_build(tmp_path)
