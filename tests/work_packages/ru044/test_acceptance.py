"""Local secret-free tests; live origin checks are recorded independently."""
from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import subprocess
import sys

import pytest

ROOT = Path(__file__).resolve().parents[3]
ORIGIN = ROOT / "deploy/download-origin"
SPEC = importlib.util.spec_from_file_location("download_origin_ingress", ORIGIN / "ingress.py")
assert SPEC and SPEC.loader
INGRESS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INGRESS)


def test_origin_contract() -> None:
    health = json.loads((ORIGIN / "public/health.json").read_text())
    assert health == {
        "schemaVersion": 1,
        "service": "yeschoy-download-origin",
        "status": "origin_ready",
        "updatesPublished": False,
        "thirdPartyInstallersPublished": False,
    }
    config = (ORIGIN / "Caddyfile.origin").read_text()
    for expected in (
        "admin off", "auto_https off", "persist_config off", "root * /srv/public",
        "@unsafe not method GET HEAD", 'respond @unsafe "Method not allowed" 405',
        'respond @hidden "Not found" 404', 'respond @directory "Not found" 404',
        '{http.request.orig_uri}.matches', 'respond @traversal "Not found" 404',
        "Cache-Control no-store", "handle /health.json", "handle /checks/download-test.txt",
        "handle /updates/stable.json", "handle /updates/beta.json",
    ):
        assert expected in config
    assert config.count('respond "" 204') == 2
    assert config.index("route {") < config.index("@unsafe") < config.index("handle / {")
    assert "browse" not in config and "tls_insecure_skip_verify" not in config
    compose = (ORIGIN / "compose.yaml").read_text()
    for expected in (
        "image: sha256:af32e97399febea808609119bb21544d0265c58a02836576e32a2d082c262c17",
        "pull_policy: never", 'user: "65532:65532"', "read_only: true",
        "cap_drop: [ALL]", "cap_add: [NET_BIND_SERVICE]", "no-new-privileges:true", "external: true", "name: web_default",
        "/srv/yeschoy-download/public:/srv/public:ro",
    ):
        assert expected in compose
    for forbidden in ("ports:", "env_file:", "privileged:", "/var/run/docker.sock", "network_mode: host"):
        assert forbidden not in compose
    files = sorted(str(p.relative_to(ORIGIN / "public")) for p in (ORIGIN / "public").rglob("*") if p.is_file())
    assert files == ["checks/download-test.txt", "health.json"]
    assert not any(p.is_symlink() for p in (ORIGIN / "public").rglob("*"))
    ingress = (ORIGIN / "Caddyfile.ingress").read_text()
    assert "ergou.qzz.io" in ingress
    assert "reverse_proxy yeschoy-download-origin:8080" in ingress
    assert "tls internal" not in ingress and "insecure" not in ingress
    assert "header_up -Authorization" in ingress and "header_up -Cookie" in ingress


def test_ingress_replay_conflict_and_recovery(tmp_path: Path) -> None:
    snippet = (ORIGIN / "Caddyfile.ingress").read_bytes()
    before = b"existing.example.test {\n reverse_proxy old-service:3000\n}\n"
    after = INGRESS.prepare(before, snippet)
    assert after.startswith(before)
    assert INGRESS.prepare(after, snippet) == after
    for conflict in (
        b"ergou.qzz.io {\n respond old\n}\n", after + b"# external change\n",
        after + snippet, before + INGRESS.BEGIN, before + INGRESS.END,
        after.replace(b"origin:8080", b"origin:9090"),
    ):
        with pytest.raises(ValueError):
            INGRESS.prepare(conflict, snippet)
    path = tmp_path / "Caddyfile"
    path.write_bytes(before)
    path.chmod(0o640)
    inode = path.stat().st_ino
    assert INGRESS.guarded_replace(path, before, after)
    assert path.stat().st_ino == inode
    assert path.stat().st_mode & 0o777 == 0o640
    assert not INGRESS.guarded_replace(path, before, after)
    assert INGRESS.guarded_replace(path, after, before)
    assert not INGRESS.guarded_replace(path, after, before)
    assert path.read_bytes() == before
    path.write_bytes(before + b"# another operator\n")
    with pytest.raises(ValueError):
        INGRESS.guarded_replace(path, before, after)
    with pytest.raises(ValueError):
        INGRESS.guarded_replace(path, after, before)
    assert path.read_bytes() == before + b"# another operator\n"
    link = tmp_path / "symlink"
    link.symlink_to(path)
    with pytest.raises(OSError):
        INGRESS.guarded_replace(link, before, after)
    path.write_bytes(before)
    baseline = tmp_path / "baseline"
    baseline.write_bytes(before)
    candidate = tmp_path / "candidate"
    args = [
        "--current", str(path), "--baseline", str(baseline),
        "--candidate", str(candidate), "--snippet", str(ORIGIN / "Caddyfile.ingress"),
        "--expected-sha256", INGRESS.digest(before),
    ]
    for action in ("prepare", "prepare", "promote", "promote", "restore", "restore"):
        result = subprocess.run([sys.executable, str(ORIGIN / "ingress.py"), action, *args], capture_output=True, text=True)
        assert result.returncode == 0, result.stderr
    assert path.read_bytes() == before
    candidate.write_bytes(after + b"# candidate tampered\n")
    result = subprocess.run([sys.executable, str(ORIGIN / "ingress.py"), "promote", *args], capture_output=True, text=True)
    assert result.returncode != 0
    assert path.read_bytes() == before
