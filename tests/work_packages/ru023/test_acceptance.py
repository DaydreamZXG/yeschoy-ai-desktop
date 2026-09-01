import json
import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
NATIVE = ROOT / "src-tauri/src/account_v2.rs"
LINES = ROOT / "src-tauri/src/connectivity_core.rs"
SESSION = ROOT / "src/account/session.ts"
HANDOFF = ROOT / "outputs/野菜API-客户端v2接入与上线阻塞清单.md"


def run(*command: str, timeout: int = 600) -> None:
    environment = os.environ.copy()
    environment["RUSTUP_TOOLCHAIN"] = "stable"
    subprocess.run(
        command,
        cwd=ROOT,
        check=True,
        timeout=timeout,
        env=environment,
    )


def test_native_account_security_boundary():
    native = NATIVE.read_text(encoding="utf-8")
    lines = LINES.read_text(encoding="utf-8")
    assert "https://yeschoy.com" in lines
    assert "https://api.yeschoy.com" in lines
    assert "CONNECTIVITY_LINES" in native
    assert "Policy::none" in native
    assert ".redirect(Policy::none())" in native
    assert "com.yeschoy.desktop.account" in native
    assert "keyring::v1::{Entry" in native
    projection = native[
        native.index("struct AccountProjection") : native.index(
            "impl AccountProjection"
        )
    ]
    assert "device_code" not in projection
    assert "access_token" not in projection
    assert "access_token" not in SESSION.read_text(encoding="utf-8")
    for native_test in (
        "request_and_url_allowlists_reject_caller_controlled_network_targets",
        "price_projection_is_ratio_based_and_never_prices_unsupported_rows",
        "projection_serialization_contains_no_secret_fields",
    ):
        assert native_test in native


def test_renderer_account_and_pricing_states(tmp_path: Path):
    config = tmp_path / "vitest.config.mjs"
    config.write_text(
        "export default {\n"
        f"  resolve: {{ alias: {{ '@': {json.dumps(str(ROOT / 'src'))} }} }},\n"
        "  test: {\n"
        "    environment: 'jsdom',\n"
        "    globals: true,\n"
        "    setupFiles: [\n"
        f"      {json.dumps(str(ROOT / 'tests/setupGlobals.ts'))},\n"
        f"      {json.dumps(str(ROOT / 'tests/setupTests.ts'))}\n"
        "    ]\n"
        "  }\n"
        "};\n",
        encoding="utf-8",
    )
    run(
        str(ROOT / "node_modules/.bin/vitest"),
        "run",
        "--no-cache",
        "--config",
        str(config),
        "--dir",
        "src",
        "account/session.test.ts",
        "workbench/Workbench.test.tsx",
        "candidate/readiness.test.ts",
    )


def test_candidate_build_and_server_handoff(tmp_path: Path):
    build_output = tmp_path / "dist"
    config = tmp_path / "vite.config.mjs"
    config.write_text(
        "export default {\n"
        f"  root: {json.dumps(str(ROOT / 'src'))},\n"
        "  base: './',\n"
        f"  build: {{ outDir: {json.dumps(str(build_output))}, emptyOutDir: true }},\n"
        f"  resolve: {{ alias: {{ '@': {json.dumps(str(ROOT / 'src'))} }} }}\n"
        "};\n",
        encoding="utf-8",
    )
    run(str(ROOT / "node_modules/.bin/tsc"), "--noEmit")
    run(
        str(ROOT / "node_modules/.bin/vite"),
        "build",
        "--config",
        str(config),
        "--configLoader",
        "runner",
    )
    assert (build_output / "index.html").is_file()
    assert '"version": "0.2.0"' in (ROOT / "package.json").read_text()
    assert '"version": "0.2.0"' in (ROOT / "src-tauri/tauri.conf.json").read_text()
    handoff = HANDOFF.read_text(encoding="utf-8")
    for required in (
        "2e5973a3705688a81867985a7e5a899b72be674c",
        "未修改或部署 NewAPI、Nginx、DNS、Redis、数据库和线上配置",
        "不新增数据库表",
        "5 分钟 TTL",
        "session_purpose=desktop",
        "必须得到 `403`",
        "1 USD = 6.75 CNY",
        "当前单价比较",
        "不能公开上线",
    ):
        assert required in handoff
