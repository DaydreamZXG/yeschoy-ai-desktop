"""Registered pytest acceptance; actual TS/React execution is in memory only."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[3]
RUNTIME = Path(__file__).with_name("runtime.cjs")


def run_runtime(scenario: str, minimum_checks: int) -> None:
    completed = subprocess.run(
        ["node", str(RUNTIME), scenario], cwd=ROOT, capture_output=True,
        text=True, check=False, timeout=120,
    )
    assert completed.returncode == 0, completed.stderr
    result = json.loads(completed.stdout)
    assert result["scenario"] == scenario
    assert result["passed"] is True
    assert result["checks"] >= minimum_checks


def test_catalog_contract_runtime_boundary() -> None:
    run_runtime("contract", 30)


def test_tool_access_runtime_protocol_matrix() -> None:
    run_runtime("access", 35)


def test_ui_read_recovery_and_untrusted_text() -> None:
    run_runtime("recovery", 7)


def test_ui_selection_races_and_preview_consistency() -> None:
    run_runtime("race", 9)


def test_bootstrap_handoff_and_transport_boundary() -> None:
    read = lambda path: (ROOT / path).read_text()
    schema = json.loads(read("contracts/desktop-bootstrap.v1.schema.json"))
    example = json.loads(read("contracts/fixtures/desktop-bootstrap/recognized.json"))
    bad = json.loads(read("contracts/fixtures/desktop-bootstrap/incompatible.json"))
    data = schema["properties"]["data"]
    assert schema["additionalProperties"] is False
    assert data["additionalProperties"] is False
    assert set(example["data"]) == set(data["required"])
    assert example["data"]["schema_version"] == data["properties"]["schema_version"]["const"]
    assert bad["data"]["schema_version"] != example["data"]["schema_version"]
    assert set(example["data"]["capabilities"]) == {
        "device_authorization", "account_read", "usage_read", "models_read",
        "pricing_read", "tool_keys_manage",
    }
    assert all(value is False for value in example["data"]["capabilities"].values())
    native = read("src-tauri/src/service_catalog.rs").split("#[cfg(test)]")[0]
    for required in ("/api/status", "/api/pricing", "/api/desktop/v1/bootstrap",
                     ".https_only(true)", ".redirect(Policy::none())", ".no_proxy()",
                     "MAX_RESPONSE_BYTES", "try_acquire()", "deny_unknown_fields"):
        assert required in native
    for forbidden in (".post(", ".put(", ".delete(", "bearer_auth(", "cookie_store(",
                      "Command::new", "File::create", "danger_accept_invalid_certs"):
        assert forbidden not in native
    handoff = read("outputs/野菜API-后端技术交接-客户端接入.md")
    for required in ("P0-A", "P0-B", "P0-C", "GroupGroupRatio", "6.75",
                     "不是本轮已经完成的客户端调用器", "本页不是生产上线批准"):
        assert required in handoff


def test_candidate_release_boundary() -> None:
    config = json.loads((ROOT / "src-tauri/tauri.candidate.conf.json").read_text())
    base = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())
    assert config["bundle"]["active"] is True
    assert config["bundle"]["createUpdaterArtifacts"] is False
    assert base["bundle"]["createUpdaterArtifacts"] is False
    assert base["identifier"] == "com.yeschoy.desktop"
    assert base["productName"] == "野菜API"
    assert "connect-src 'self' ipc: http://ipc.localhost" in base["app"]["security"]["csp"]
    assert "pnpm@" in json.loads((ROOT / "package.json").read_text())["packageManager"]
    assert not (ROOT / "package-lock.json").exists()
    assert 'ACCOUNT_READINESS_STATUS = "backend_upgrade_required"' in (ROOT / "src/account/readiness.ts").read_text()
    guide = (ROOT / "outputs/野菜API-桌面客户端-V1-候选版发布清单.md").read_text()
    for required in ("不是正式发布批准", "hardened runtime", "公证", "SHA-256", "未签名候选测试包"):
        assert required in guide
    for secret in ("BEGIN PRIVATE KEY", "BEGIN RSA PRIVATE KEY", '"password"', '"privateKey"'):
        assert secret not in json.dumps(config)
