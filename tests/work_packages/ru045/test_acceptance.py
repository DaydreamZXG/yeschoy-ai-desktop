from __future__ import annotations

from contextlib import contextmanager
import importlib.util
import io
import json
from pathlib import Path
import socket
import subprocess
import sys
import zipfile

import pytest

ROOT = Path(__file__).resolve().parents[3]
DIRECTORY = ROOT / "deploy/vendor-sync"
SPEC = importlib.util.spec_from_file_location("vendor_sync", DIRECTORY / "sync.py")
SYNC = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SYNC)
SOURCES = SYNC.sources()
CODEX = next(s for s in SOURCES if s["id"] == "codex-windows-x64")
MAC = next(s for s in SOURCES if s["id"] == "codex-macos-arm64")
CLAUDE = next(s for s in SOURCES if s["id"] == "claude-windows-x64")
BUDGET = 10 * 1024 * 1024


def msix(version="1.2.3.4", name="OpenAI.Codex", arch="x64", signature=True, xml=None):
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as archive:
        archive.writestr("AppxManifest.xml", xml or f'<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="{name}" Version="{version}" ProcessorArchitecture="{arch}" Publisher="CN=Fixture"/></Package>')
        archive.writestr("AppxBlockMap.xml", "<BlockMap/>")
        if signature:
            archive.writestr("AppxSignature.p7x", b"this is deliberately not a valid signature")
        archive.writestr("app.exe", b"not executed")
    return buffer.getvalue()


class Response:
    def __init__(self, body=b"", status=200, headers=None):
        self.status = status
        self.headers = {"content-type": "application/vnd.ms-appx", "content-length": str(len(body)), "etag": '"v1"', **(headers or {})}
        self.stream = io.BytesIO(body)

    def getheaders(self):
        return list(self.headers.items())

    def read(self, size):
        return self.stream.read(size)


class FakeTransport:
    def __init__(self, responses):
        self.responses = iter(responses)
        self.calls = []

    @contextmanager
    def request(self, source, method, url, headers=None):
        SYNC.validate_url(source, url)
        self.calls.append((method, url, headers or {}))
        response = next(self.responses)
        if isinstance(response, Exception):
            raise response
        yield response


def metadata(body, etag='"v1"'):
    return SYNC.discover(CODEX, FakeTransport([Response(headers={"content-length": str(len(body)), "etag": etag})]))


def staging(tmp_path):
    return SYNC.private_root(tmp_path / "private-staging")


def test_sources_and_contract(tmp_path, monkeypatch):
    assert len(SOURCES) == 6
    assert {s["platform"] for s in SOURCES} == {"macos", "windows"}
    assert all(s["evidence"].startswith("https://") for s in SOURCES)
    invalid = ["http://persistent.oaistatic.com/codex-app-prod/a.msix", "https://127.0.0.1/a.msix", "https://persistent.oaistatic.com.evil.test/a.msix", "https://u:p@persistent.oaistatic.com/codex-app-prod/a.msix", "https://persistent.oaistatic.com:444/codex-app-prod/a.msix", "https://persistent.oaistatic.com/codex-app-prod/%2e%2e/a.msix", "https://persistent.oaistatic.com/codex-app-prod/a.msix?secret=a", "https://persistent.oaistatic.com/other/a.msix"]
    for url in invalid:
        with pytest.raises(SYNC.SyncError):
            SYNC.validate_url(CODEX, url)
    for address in ["127.0.0.1", "10.1.2.3", "169.254.169.254", "::1", "224.0.0.1"]:
        monkeypatch.setattr(socket, "getaddrinfo", lambda *a, ip=address, **kw: [(socket.AF_INET, socket.SOCK_STREAM, 6, "", (ip, 443))])
        with pytest.raises(SYNC.SyncError):
            SYNC.public_addresses("persistent.oaistatic.com")
    monkeypatch.setattr(socket, "getaddrinfo", lambda *a, **kw: [(socket.AF_INET, socket.SOCK_STREAM, 6, "", ("1.1.1.1", 443))])
    assert SYNC.public_addresses("persistent.oaistatic.com") == ["1.1.1.1"]
    with pytest.raises(SYNC.SyncError):
        SYNC.discover(CODEX, FakeTransport([Response(status=302, headers={"location": "https://evil.test/install.msix"})]))
    redirect = "https://downloads.claude.ai/releases/win32/x64/1.2.3/Claude.msix"
    transport = FakeTransport([Response(status=307, headers={"location": redirect}), Response(b"123")])
    assert SYNC.discover(CLAUDE, transport)["url"] == redirect
    assert [c[0] for c in transport.calls] == ["GET", "HEAD"]
    with pytest.raises(SYNC.SyncError, match="source_challenge"):
        SYNC.discover(MAC, FakeTransport([Response(status=403, headers={"cf-mitigated": "challenge"})]))
    for bad_headers in [{"etag": 'W/"v1"'}, {"content-length": "0"}, {"content-type": "text/html"}, {"content-encoding": "gzip"}]:
        with pytest.raises(SYNC.SyncError):
            SYNC.discover(CODEX, FakeTransport([Response(b"123", headers=bad_headers)]))
    status = SYNC.status({}, SOURCES)
    assert set(status) == {"schemaVersion", "service", "checkedAt", "publicInstallersPublished", "sources"}
    assert status["publicInstallersPublished"] is False
    assert all(s["status"] == "never_checked" and s["version"] == "unknown" for s in status["sources"])
    service = (DIRECTORY / "yeschoy-vendor-sync.service").read_text()
    for setting in ["DynamicUser=yes", "ProtectSystem=strict", "ProtectHome=yes", "NoNewPrivileges=yes", "StateDirectoryMode=0700", "InaccessiblePaths=-/srv/yeschoy-download/public", "--download", "--budget-bytes"]:
        assert setting in service
    assert "00,06,12,18:00:00 UTC" in (DIRECTORY / "yeschoy-vendor-sync.timer").read_text()
    assert not list(DIRECTORY.glob("public/**"))


def test_download_replay_resume(tmp_path):
    root = staging(tmp_path)
    body = msix()
    meta = metadata(body)
    partial_dir = root / "partials"
    partial_dir.mkdir(mode=0o700)
    offset = 70
    partial = partial_dir / f"{meta['fingerprint']}.part"
    partial.write_bytes(body[:offset])
    SYNC.atomic_json(partial.with_suffix(".json"), meta)
    transport = FakeTransport([Response(body[offset:], status=206, headers={"content-range": f"bytes {offset}-{len(body)-1}/{len(body)}"})])
    candidate = SYNC.download(root, CODEX, meta, transport, BUDGET)
    assert (root / candidate["path"]).read_bytes() == body
    assert candidate["package"]["signature"] == "pending_native_verification"
    assert transport.calls[0][2] == {"If-Match": '"v1"', "Range": "bytes=70-", "If-Range": '"v1"'}
    # Lost catalog acknowledgement reuses the independently saved object receipt.
    replay = FakeTransport([Response(headers={"content-length": str(len(body))})])
    status, events = SYNC.scan(root, [CODEX], replay, True, BUDGET)
    assert len(replay.calls) == 1 and events[0]["action"] == "staged"
    replay2 = FakeTransport([Response(headers={"content-length": str(len(body))})])
    _, events = SYNC.scan(root, [CODEX], replay2, True, BUDGET)
    assert len(replay2.calls) == 1 and events[0]["action"] == "unchanged"
    assert status["sources"][0]["version"] == "1.2.3.4"
    assert len(list((root / "artifacts").glob("*.msix"))) == 1
    # A same-validator full response can restart a partial when Range is ignored.
    newer = metadata(body, '"v2"')
    part = root / "partials" / f"{newer['fingerprint']}.part"
    part.write_bytes(body[:70])
    SYNC.atomic_json(part.with_suffix(".json"), newer)
    restarted = SYNC.download(root, CODEX, newer, FakeTransport([Response(body, headers={"etag": '"v2"'})]), BUDGET)
    assert (root / restarted["path"]).read_bytes() == body


def test_stale_and_failure_recovery(tmp_path, monkeypatch):
    root = staging(tmp_path)
    body = msix()
    header = {"content-length": str(len(body))}
    SYNC.scan(root, [CODEX], FakeTransport([Response(headers=header), Response(body)]), True, BUDGET)
    prior = json.loads((root / "catalog.json").read_text())["sources"][CODEX["id"]]["candidate"]
    for failed in [Response(status=503), Response(status=403, headers={"cf-mitigated": "challenge"})]:
        result, _ = SYNC.scan(root, [CODEX], FakeTransport([failed]), True, BUDGET)
        assert result["sources"][0]["status"] == "failed"
        assert json.loads((root / "catalog.json").read_text())["sources"][CODEX["id"]]["candidate"] == prior
    changed = metadata(body, '"v2"')
    for bad in [Response(body, headers={"etag": '"v3"'}), Response(body, status=412), Response(b"<html>", headers={"content-type": "text/html"})]:
        with pytest.raises(SYNC.SyncError):
            SYNC.download(root, CODEX, changed, FakeTransport([bad]), BUDGET)
    with pytest.raises(SYNC.SyncError, match="incomplete_download"):
        SYNC.download(root, CODEX, changed, FakeTransport([Response(body[:70], headers={"etag": '"v2"', "content-length": str(len(body))})]), BUDGET)
    for bad in [Response(body, headers={"etag": '"v3"'}), Response(body[70:], status=206, headers={"etag": '"v2"', "content-range": "bytes 0-9/10"})]:
        with pytest.raises(SYNC.SyncError):
            SYNC.download(root, CODEX, changed, FakeTransport([bad]), BUDGET)
    # Catalog replacement failure leaves previous receipt readable.
    before = (root / "catalog.json").read_bytes()
    real_replace = SYNC.os.replace
    monkeypatch.setattr(SYNC.os, "replace", lambda *args: (_ for _ in ()).throw(OSError("interrupted")))
    with pytest.raises(OSError):
        SYNC.atomic_json(root / "catalog.json", {"bad": True})
    assert (root / "catalog.json").read_bytes() == before
    monkeypatch.setattr(SYNC.os, "replace", real_replace)
    # Corrupt cache is reported, never reused or silently overwritten.
    (root / prior["path"]).write_bytes(b"corrupt")
    result, _ = SYNC.scan(root, [CODEX], FakeTransport([Response(headers=header)]), True, BUDGET)
    assert result["sources"][0]["error"] == "cached_artifact_corrupt"
    assert json.loads((root / "catalog.json").read_text())["sources"][CODEX["id"]]["candidate"] == prior
    # An older official source must not replace the existing newest candidate.
    another = staging(tmp_path / "regression")
    new_body = msix(version="2.0.0.0")
    old_body = msix(version="1.0.0.0")
    SYNC.scan(another, [CODEX], FakeTransport([Response(headers={"content-length": str(len(new_body))}), Response(new_body)]), True, BUDGET)
    result, _ = SYNC.scan(another, [CODEX], FakeTransport([Response(headers={"content-length": str(len(old_body)), "etag": '"v2"'}), Response(old_body, headers={"etag": '"v2"'})]), True, BUDGET)
    assert result["sources"][0]["error"] == "version_regression"
    assert result["sources"][0]["version"] == "2.0.0.0"


def test_storage_and_concurrency(tmp_path):
    root = staging(tmp_path)
    body = msix()
    meta = metadata(body)
    with pytest.raises(SYNC.SyncError, match="cache_quota_or_disk_full"):
        SYNC.download(root, CODEX, meta, FakeTransport([]), 1)
    with SYNC.locked(root):
        with pytest.raises(SYNC.SyncError, match="already_running"):
            with SYNC.locked(root):
                raise AssertionError("lock incorrectly entered")
        # Read-only status remains usable while a timer holds the writer lock.
        completed = subprocess.run([sys.executable, str(DIRECTORY / "sync.py"), "status", "--state-dir", str(root)], capture_output=True, text=True)
        assert completed.returncode == 0
        assert json.loads(completed.stdout)["sources"][0]["status"] == "never_checked"
    with SYNC.locked(root):
        pass
    outside = tmp_path / "outside"
    outside.write_bytes(b"preserved")
    (root / "catalog.json").symlink_to(outside)
    with pytest.raises(SYNC.SyncError):
        SYNC.atomic_json(root / "catalog.json", {})
    assert outside.read_bytes() == b"preserved"
    for path in ["../outside", "/etc/passwd"]:
        with pytest.raises(SYNC.SyncError):
            SYNC.child(root, path)
    with pytest.raises(SYNC.SyncError):
        SYNC.candidate_path(root, {"path": "../outside"})
    with pytest.raises(SYNC.SyncError):
        SYNC.used_bytes(root)
    public = tmp_path / "public"
    with pytest.raises(SYNC.SyncError):
        SYNC.private_root(public)
    linked_root = tmp_path / "linked-root"
    linked_root.symlink_to(root)
    with pytest.raises(SYNC.SyncError):
        SYNC.private_root(linked_root)
    absent = tmp_path / "does-not-exist"
    completed = subprocess.run([sys.executable, str(DIRECTORY / "sync.py"), "status", "--state-dir", str(absent)], capture_output=True)
    assert completed.returncode == 0 and not absent.exists()


def test_package_identity_and_verification(tmp_path, monkeypatch):
    path = tmp_path / "candidate.msix"
    for body in [msix(name="Other"), msix(arch="arm64"), msix(version="weird"), msix(signature=False), msix(xml='<!DOCTYPE x [<!ENTITY y "hello">]><Package/>'), b"<html>challenge</html>"]:
        path.write_bytes(body)
        with pytest.raises(SYNC.SyncError):
            SYNC.inspect_package(path, CODEX)
    buffer = io.BytesIO(msix())
    with zipfile.ZipFile(buffer, "a") as archive:
        archive.writestr("../outside", b"no")
    path.write_bytes(buffer.getvalue())
    with pytest.raises(SYNC.SyncError, match="unsafe_archive"):
        SYNC.inspect_package(path, CODEX)
    path.write_bytes(msix())
    package = SYNC.inspect_package(path, CODEX)
    assert package["signature"] == "pending_native_verification"
    assert package["compatibility"] == "not_tested"
    assert not (tmp_path / "outside").exists()
    path.write_bytes(b"x" * 10 + b"koly" + b"\0" * 508)
    assert SYNC.inspect_package(path, MAC)["version"] == "unknown"
    sys.path.insert(0, str(DIRECTORY))
    try:
        spec = importlib.util.spec_from_file_location("native_verify", DIRECTORY / "verify_native.py")
        native = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(native)
        monkeypatch.setattr(sys, "platform", "linux")
        with pytest.raises(Exception, match="macos_verifier_required"):
            native.verify(path, MAC)
    finally:
        sys.path.pop(0)
