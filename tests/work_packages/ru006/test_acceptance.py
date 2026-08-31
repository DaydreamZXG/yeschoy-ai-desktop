import importlib.util
import json
from pathlib import Path
import re
import subprocess

import pytest
import yaml


ROOT = Path(__file__).resolve().parents[3]
CI = ROOT / ".github/workflows/ci.yml"
NIGHTLY = ROOT / ".github/workflows/wsl2-nightly.yml"
GUIDE = ROOT / "outputs/野菜API-CI验证说明.md"
SPEC = importlib.util.spec_from_file_location(
    "candidate_native_ci", ROOT / "scripts/ci/run-rust-tests.py"
)
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)


def workflow(path):
    # BaseLoader preserves GitHub's YAML key `on` (YAML 1.1 treats it as true).
    return yaml.load(path.read_text(encoding="utf-8"), Loader=yaml.BaseLoader)


def steps(job):
    return job.get("steps", [])


def commands(job):
    return [step["run"] for step in steps(job) if "run" in step]


def artifact(name=RUNNER.LIBRARY, executable="candidate-tests.exe", test=True):
    return json.dumps(
        {
            "reason": "compiler-artifact",
            "target": {"name": name},
            "profile": {"test": test},
            "executable": executable,
        }
    )


def discovery(names=None):
    return "\n".join(f"{name}: test" for name in (names or RUNNER.REQUIRED_TESTS))


def summary(passed=6, failed=0, ignored=0, filtered=0):
    return (
        f"test result: ok. {passed} passed; {failed} failed; {ignored} ignored; "
        f"0 measured; {filtered} filtered out; finished in 0.00s\n"
    )


def completed(stdout="", code=0):
    return subprocess.CompletedProcess([], code, stdout, "")


def fake_commands(monkeypatch, outcomes):
    pending = iter(outcomes)
    calls = []

    def invoke(argv, **kwargs):
        calls.append((argv, kwargs))
        outcome = next(pending)
        if isinstance(outcome, Exception):
            raise outcome
        return outcome

    monkeypatch.setattr(RUNNER.subprocess, "run", invoke)
    return calls


def test_triggers_and_nightly_reuse():
    ci = workflow(CI)
    nightly = workflow(NIGHTLY)
    assert ci["name"] == "YesChoy CI"
    for trigger in ("push", "pull_request"):
        assert ci["on"][trigger] == {"branches": ["main", "product/**"]}
    assert {"workflow_call", "workflow_dispatch"} <= ci["on"].keys()
    assert nightly["on"]["schedule"] == [{"cron": "23 18 * * *"}]
    assert "workflow_dispatch" in nightly["on"]
    assert nightly["name"] == "YesChoy Nightly"
    assert nightly["jobs"] == {
        "candidate": {
            "name": "Current candidate checks",
            "uses": "./.github/workflows/ci.yml",
        }
    }
    assert ci["concurrency"]["cancel-in-progress"] == "true"
    assert "github.workflow" in ci["concurrency"]["group"]
    assert "github.ref" in ci["concurrency"]["group"]
    assert "concurrency" not in nightly  # No caller/callee self-cancellation.


def test_current_candidate_and_native_matrix():
    ci = workflow(CI)
    assert set(ci["jobs"]) == {"frontend", "native"}
    front = commands(ci["jobs"]["frontend"])
    assert {"pnpm install --frozen-lockfile", "pnpm typecheck", "pnpm test:candidate", "pnpm build:renderer"} <= set(front)
    assert any("--import-mode=importlib tests/work_packages/ru005 tests/work_packages/ru006" in cmd for cmd in front)
    assert not any("pnpm test:unit" in cmd for cmd in front)
    package = json.loads((ROOT / "package.json").read_text())
    for test_path in package["scripts"]["test:candidate"].split()[2:]:
        assert (ROOT / test_path).is_file()
    native = ci["jobs"]["native"]
    assert native["strategy"]["fail-fast"] == "false"
    assert {item["os"] for item in native["strategy"]["matrix"]["include"]} == {
        "windows-2025", "macos-15-intel", "macos-15"
    }
    native_commands = commands(native)
    assert "pnpm build:renderer" in native_commands
    assert "cargo fmt --check --manifest-path src-tauri/Cargo.toml" in native_commands
    assert "cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings" in native_commands
    assert "python scripts/ci/run-rust-tests.py" in native_commands
    assert "cargo build --locked --manifest-path src-tauri/Cargo.toml --bin yeschoy-desktop" in native_commands
    rust = next(s for s in steps(native) if s.get("uses", "").startswith("dtolnay/"))
    channel = re.search(r'channel = "([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    assert rust["with"]["toolchain"] == channel
    for job in ci["jobs"].values():
        node = next(s for s in steps(job) if s.get("uses", "").startswith("actions/setup-node@"))
        assert node["with"]["node-version"] == "22"


def test_workflows_have_no_release_or_secret_authority():
    for path in (CI, NIGHTLY):
        document = workflow(path)
        assert document["permissions"] == {"contents": "read"}
        assert "pull_request_target" not in document["on"]
        for job in document["jobs"].values():
            assert "secrets" not in job and "environment" not in job
            assert "continue-on-error" not in job and "if" not in job
            for step in steps(job):
                assert "continue-on-error" not in step and "if" not in step
                if "uses" in step:
                    assert re.fullmatch(r"[\w-]+/[\w-]+@[0-9a-f]{40}", step["uses"])
                if step.get("uses", "").startswith("actions/checkout@"):
                    assert step["with"]["persist-credentials"] == "false"
            source = "\n".join(commands(job))
            for forbidden in (
                "CC_SWITCH", "cc_switch_lib", "wsl.exe", "$env:TEMP", "$env:TMP",
                "gh release", "tauri build", "notarytool", "secrets.", "|| true",
            ):
                assert forbidden not in source
        assert "secrets." not in path.read_text()
    cache = next(s for s in steps(workflow(CI)["jobs"]["native"]) if s.get("uses", "").startswith("actions/cache@"))
    assert "runner.os" in cache["with"]["key"]
    assert "runner.arch" in cache["with"]["key"]
    assert "rust-toolchain.toml" in cache["with"]["key"]
    assert "restore-keys" not in cache["with"]


def test_runner_selects_current_artifact_and_runs_all_cases(monkeypatch):
    build = "not-json\n" + artifact(test=False) + "\n" + artifact()
    calls = fake_commands(monkeypatch, [completed(build), completed(discovery()), completed(summary())])
    assert RUNNER.verify(ROOT) == 6
    assert calls[0][0][:3] == ["cargo", "test", "--locked"]
    assert "--lib" in calls[0][0] and "--no-run" in calls[0][0]
    assert "--message-format=json" in calls[0][0]
    assert calls[1][0] == ["candidate-tests.exe", "--list", "--format=terse"]
    assert calls[2][0] == ["candidate-tests.exe", "--test-threads=1", "--color=never"]
    for _, kwargs in calls:
        assert "shell" not in kwargs and "env" not in kwargs
        assert kwargs["timeout"] > 0


def test_runner_rejects_stale_empty_or_ambiguous_artifacts():
    for output in ("", "not-json", artifact(name="cc_switch_lib"), artifact(test=False), artifact(executable=None), artifact() + "\n" + artifact()):
        with pytest.raises(RUNNER.VerificationError):
            RUNNER.select_test_binary(output)
    for output in ("", "0 tests, 0 benchmarks", "old::test: test", discovery() + "\n" + discovery()):
        with pytest.raises(RUNNER.VerificationError):
            RUNNER.listed_tests(output)


def test_runner_rejects_compile_test_skip_and_timeout_failures(monkeypatch):
    for outcomes in (
        [completed(code=1)],
        [completed(artifact()), completed(code=1)],
        [completed(artifact()), completed(discovery()), completed(code=1)],
        [completed(artifact()), completed(discovery()), completed(summary(passed=5, ignored=1))],
        [completed(artifact()), completed(discovery()), completed(summary(passed=5, filtered=1))],
        [completed(artifact()), completed(discovery()), completed(summary(passed=0))],
        [completed(artifact()), completed(discovery()), completed("no final summary")],
        [subprocess.TimeoutExpired("cargo", 2400)],
        [OSError("missing executable")],
    ):
        fake_commands(monkeypatch, outcomes)
        assert RUNNER.main() == 1
    for output in (summary(passed=5, failed=1), summary() + summary()):
        with pytest.raises(RUNNER.VerificationError):
            RUNNER.verify_test_summary(output, 6)
    with pytest.raises(RUNNER.VerificationError):
        RUNNER.listed_tests(discovery(sorted(RUNNER.REQUIRED_TESTS)[1:]))


def test_ci_guide_preserves_scope_and_recovery():
    guide = GUIDE.read_text(encoding="utf-8")
    for phrase in (
        "33335919230", "LNK1327", "product/**", "workflow_dispatch",
        "RU-005", "RU-006", "WSL 配置写入未验证", "不是安装包验收",
        "不修改 NewAPI", "取消不等于成功", "不自动发布", "run-rust-tests.py",
    ):
        assert phrase in guide
