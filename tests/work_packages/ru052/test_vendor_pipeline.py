"""Isolated official-feed/archive/publication tests; no real network or installer."""
from __future__ import annotations

from contextlib import contextmanager
import importlib.util
import io
import json
import os
from pathlib import Path
import plistlib
import stat
import sys
import zipfile

import pytest

DIRECTORY = Path(__file__).resolve().parents[3] / "deploy/vendor-sync"
sys.path.insert(0, str(DIRECTORY))
try:
    import sync as S
    import verify_native as V
    import publish as P
finally:
    sys.path.pop(0)
SOURCE = next(source for source in S.sources() if source["id"] == "claude-macos-universal")
URL = "https://downloads.claude.ai/releases/darwin/universal/1.2.3/Claude-" + "a" * 40 + ".zip"


def test_origin_serves_installers_with_client_accepted_media_types():
    config = (DIRECTORY.parent / "download-origin/Caddyfile.origin").read_text()
    assert "@msix path *.msix" in config
    assert "header @msix Content-Type application/vnd.ms-appx" in config
    assert "@dmg path *.dmg" in config
    assert "header @dmg Content-Type application/x-apple-diskimage" in config


def bundle(version="1.2.3", extra=()):
    output = io.BytesIO()
    entries = [
        ("Claude.app/Contents/Info.plist", plistlib.dumps({"CFBundleIdentifier": SOURCE["identity"], "CFBundleShortVersionString": version}), 0o100644),
        ("Claude.app/Contents/MacOS/Claude", b"fixture never executed", 0o100755),
        ("Claude.app/Contents/Frameworks/Example.framework/Versions/A/Example", b"framework", 0o100755),
        ("Claude.app/Contents/Frameworks/Example.framework/Versions/Current", b"A", 0o120755),
        ("Claude.app/Contents/Frameworks/Example.framework/Example", b"Versions/Current/Example", 0o120755),
    ]
    with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as package:
        for name, body, mode in entries + list(extra):
            info = zipfile.ZipInfo(name)
            info.create_system = 3
            info.external_attr = mode << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            package.writestr(info, body)
    return output.getvalue()


class Response:
    def __init__(self, body=b"", status=200, **headers):
        self.status = status
        self.headers = {"content-type": "application/octet-stream", "content-length": str(len(body)), "etag": '"fixture-v1"', **headers}
        self.body = io.BytesIO(body)

    def getheaders(self):
        return list(self.headers.items())

    def read(self, size):
        return self.body.read(size)


class Transport:
    def __init__(self, responses):
        self.responses = iter(responses)
        self.calls = []

    @contextmanager
    def request(self, source, method, url, headers=None):
        S.validate_url(source, url)
        self.calls.append((method, url, headers))
        yield next(self.responses)


def feed(version="1.2.3", url=URL):
    return {"currentRelease": version, "releases": [{"version": version, "updateTo": {"version": version, "url": url}}]}


def json_response(value):
    return Response(json.dumps(value).encode(), **{"content-type": "application/json"})


def test_official_feed_exact_version_and_bounded_failure(tmp_path):
    body = bundle()
    transport = Transport([json_response(feed()), Response(body)])
    metadata = S.discover(SOURCE, transport)
    assert metadata["url"] == URL and metadata["versionHint"] == "1.2.3"
    assert [call[0] for call in transport.calls] == ["GET", "HEAD"]
    for bad in [feed(url=URL.replace("downloads.claude.ai", "evil.test")), feed(url=URL.replace("/1.2.3/", "/1.2.4/")), feed(url=URL + "?token=x"), feed(version="latest"), {"currentRelease": "1.2.3", "releases": []}, {"currentRelease": "1.2.3", "releases": feed()["releases"] * 2}]:
        with pytest.raises(S.SyncError):
            S.discover(SOURCE, Transport([json_response(bad)]))
    for bad in [Response(status=403, **{"cf-mitigated": "challenge"}), Response(b"{}", **{"content-type": "text/html"}), Response(b"{}", **{"content-type": "application/json", "content-length": str(S.MAX_JSON_BYTES + 1)}), Response(b'{"currentRelease":"1.2.3","currentRelease":"1.2.4"}', **{"content-type": "application/json"})]:
        with pytest.raises(S.SyncError):
            S.discover(SOURCE, Transport([bad]))
    root = S.private_root(tmp_path / "stage")
    result, _ = S.scan(root, [SOURCE], Transport([json_response(feed()), Response(body), Response(body)]), True, S.MAX_BYTES)
    assert result["sources"][0]["version"] == "1.2.3"
    before = json.loads((root / "catalog.json").read_text())["sources"][SOURCE["id"]]["candidate"]
    old_feed = feed("1.2.2", URL.replace("/1.2.3/", "/1.2.2/"))
    result, _ = S.scan(root, [SOURCE], Transport([json_response(old_feed), Response(body)]), True, S.MAX_BYTES)
    assert result["sources"][0]["error"] == "version_regression"
    assert json.loads((root / "catalog.json").read_text())["sources"][SOURCE["id"]]["candidate"] == before
    assert (root / before["path"]).read_bytes() == body


def test_zip_framework_links_are_safe_and_native_checks_unchanged(tmp_path, monkeypatch):
    path = tmp_path / "Claude.zip"
    path.write_bytes(bundle())
    assert S.inspect_package(path, SOURCE)["version"] == "1.2.3"
    destination = tmp_path / "extract"
    destination.mkdir(mode=0o700)
    app = S.extract_macos_zip(path, destination)
    framework = app / "Contents/Frameworks/Example.framework"
    assert (framework / "Example").is_symlink()
    assert (framework / "Example").read_bytes() == b"framework"
    assert (app / "Contents/MacOS/Claude").stat().st_mode & stat.S_IXUSR
    calls = []
    def native(argv):
        calls.append(argv)
        if argv[0].endswith("codesign") and "-d" in argv:
            return f"TeamIdentifier={SOURCE['teamId']}\nIdentifier={SOURCE['identity']}\n".encode()
        return b""
    monkeypatch.setattr(V.sys, "platform", "darwin")
    monkeypatch.setattr(V, "run", native)
    result = V.verify(path, SOURCE)
    assert result["sha256"] == S.digest(path) and result["published"] is False
    assert result["installation"] == "not_tested" and result["package"]["signature"] == "native_verified"
    assert [call[0] for call in calls] == ["/usr/bin/codesign", "/usr/bin/codesign", "/usr/sbin/spctl"]
    assert not any("hdiutil" in call[0] for call in calls)
    monkeypatch.setattr(V, "run", lambda argv: (_ for _ in ()).throw(S.SyncError("native_verifier_failed_codesign")))
    with pytest.raises(S.SyncError):
        V.verify(path, SOURCE)


@pytest.mark.parametrize("extra", [
    [("../outside", b"bad", 0o100644)],
    [("Claude.app/../outside", b"bad", 0o100644)],
    [("Claude.app/Contents/Link", b"/tmp/outside", 0o120777)],
    [("Claude.app/Contents/Link", b"../../outside", 0o120777)],
    [("Claude.app/Contents/Link", b"Link", 0o120777)],
    [("Claude.app/Contents/Link", b"MacOS", 0o120777), ("Claude.app/Contents/Link/evil", b"bad", 0o100644)],
    [("Claude.app/Contents/Info.PLIST", b"duplicate", 0o100644)],
    [("Claude.app/Contents/pipe", b"bad", 0o010644)],
    [("Claude.app/Contents/x\\outside", b"bad", 0o100644)],
    [("Claude.app/Contents/Back", b".", 0o120777)],
    [("Claude.app/Contents/Sub/Back", b"..", 0o120777)],
    [("Claude.app/A/file", b"a", 0o100644), ("Claude.app/B/file", b"b", 0o100644), ("Claude.app/A/link", b"../B", 0o120777), ("Claude.app/B/link", b"../A", 0o120777)],
    [("Claude.app/contents/New", b"implicit parent alias", 0o100644)],
    [("Claude.app/contents/", b"", 0o040755)],
    [("Claude.app/contents", b"Contents/Frameworks", 0o120777)],
    [("Claude.app/Café/One", b"a", 0o100644), ("Claude.app/Cafe\u0301/Two", b"b", 0o100644)],
    [("Claude.app/Straße/One", b"a", 0o100644), ("Claude.app/Strasse/Two", b"b", 0o100644)],
    [("Claude.app/Contents/Back", b"MacOS/Claude/../..", 0o120777)],
    [("Claude.app/Contents/setuid", b"not executed", 0o104755)],
])
def test_zip_rejects_traversal_cycles_parent_links_duplicates_and_specials(tmp_path, extra):
    path = tmp_path / "bad.zip"
    path.write_bytes(bundle(extra=extra))
    destination = tmp_path / "extract"
    destination.mkdir(mode=0o700)
    with pytest.raises(S.SyncError):
        S.extract_macos_zip(path, destination)
    assert not list(destination.iterdir())
    assert not (tmp_path / "outside").exists()


def test_zip_expansion_and_owned_destination(tmp_path, monkeypatch):
    path = tmp_path / "Claude.zip"
    path.write_bytes(bundle(extra=[("Claude.app/Contents/Compressed", b"a" * 5000, 0o100644)]))
    monkeypatch.setattr(S, "MAX_BYTES", path.stat().st_size + 10)
    with pytest.raises(S.SyncError, match="zip_expansion_limit"):
        S.inspect_package(path, SOURCE)
    outside = tmp_path / "outside"
    outside.mkdir(mode=0o700)
    link = tmp_path / "link"
    link.symlink_to(outside)
    with pytest.raises(S.SyncError, match="unsafe_extraction_directory"):
        S.extract_macos_zip(path, link)


@pytest.mark.parametrize("method", ["inspect", "extract"])
@pytest.mark.parametrize("failure", ["zip64_count", "wrong_count", "multidisk", "huge_directory", "fake_eocd_comment", "long_path", "long_link", "long_header_name"])
def test_zip_preflight_rejects_unbounded_metadata_before_library_constructor(tmp_path, monkeypatch, method, failure):
    body = bytearray(bundle())
    footer_at = body.rfind(b"PK\x05\x06")
    footer = bytearray(body[footer_at:])
    path = tmp_path / "forged.zip"
    if failure == "huge_directory":
        # Sparse file: a structurally placed EOCD advertises a directory larger
        # than the metadata budget. Preflight reads only the bounded tail and
        # must reject before ZipFile tries to allocate/read the advertised size.
        directory_size = S.MAX_DIRECTORY_BYTES + 1
        footer[8:12] = (1).to_bytes(2, "little") * 2
        footer[12:16] = directory_size.to_bytes(4, "little")
        footer[16:20] = (0).to_bytes(4, "little")
        with path.open("wb") as stream:
            stream.write(b"PK\x03\x04")
            stream.seek(directory_size)
            stream.write(footer)
    elif failure == "fake_eocd_comment":
        # The real EOCD has a valid EOF-reaching comment, but the comment
        # contains a later fake EOCD plus one trailing byte. Python's parser
        # selects the fake footer, whereas an EOF-matching preflight used to
        # select the real one. Its fake directory size exceeds the budget.
        original_start = int.from_bytes(footer[16:20], "little")
        central = body[original_start:footer_at]
        relocated_start = S.MAX_DIRECTORY_BYTES + 4096
        footer[16:20] = relocated_start.to_bytes(4, "little")
        fake = bytearray(footer)
        fake[12:16] = (S.MAX_DIRECTORY_BYTES + 1).to_bytes(4, "little")
        fake[16:20] = (0).to_bytes(4, "little")
        footer[20:22] = (len(fake) + 1).to_bytes(2, "little")
        with path.open("wb") as stream:
            stream.write(body[:original_start])
            stream.seek(relocated_start)
            stream.write(central)
            stream.write(footer)
            stream.write(fake)
            stream.write(b"x")
    else:
        if failure in {"zip64_count", "wrong_count"}:
            count = 65535 if failure == "zip64_count" else 1
            body[footer_at + 8:footer_at + 12] = count.to_bytes(2, "little") * 2
        elif failure == "multidisk":
            body[footer_at + 4:footer_at + 6] = (1).to_bytes(2, "little")
        elif failure == "long_path":
            body = bundle(extra=[("Claude.app/" + "a/" * 600 + "file", b"x", 0o100644)])
        elif failure == "long_link":
            body = bundle(extra=[("Claude.app/Contents/Link", b"a" * 1024, 0o120777)])
        else:
            first = body.find(b"PK\x01\x02")
            body[first + 28:first + 30] = (1025).to_bytes(2, "little")
        path.write_bytes(body)
    entered = []
    def forbidden_constructor(*args, **kwargs):
        entered.append(True)
        raise AssertionError("unbounded metadata reached ZipFile constructor")
    monkeypatch.setattr(S.zipfile, "ZipFile", forbidden_constructor)
    with pytest.raises(S.SyncError):
        if method == "inspect":
            S.inspect_package(path, SOURCE)
        else:
            destination = tmp_path / "extract"
            destination.mkdir(mode=0o700)
            S.extract_macos_zip(path, destination)
    assert not entered


def test_zip_rejects_utf8_byte_limit_before_namespace(tmp_path):
    # Fewer than 1024 Unicode characters may still exceed Darwin PATH_MAX.
    path = tmp_path / "unicode-path.zip"
    path.write_bytes(bundle(extra=[("Claude.app/" + "é" * 510, b"x", 0o100644)]))
    with pytest.raises(S.SyncError):
        S.inspect_package(path, SOURCE)


def prepared(tmp_path, version="1.2.3"):
    state = S.private_root(tmp_path / "stage")
    body = bundle(version)
    url = URL.replace("/1.2.3/", f"/{version}/")
    S.scan(state, [SOURCE], Transport([json_response(feed(version, url)), Response(body), Response(body)]), True, S.MAX_BYTES)
    candidate = json.loads((state / "catalog.json").read_text())["sources"][SOURCE["id"]]["candidate"]
    receipt = {"schemaVersion": 1, "source": SOURCE["id"], "sha256": candidate["sha256"], "checkedAt": "2026-01-01T00:00:00Z",
               "package": {"version": version, "identity": SOURCE["identity"], "publisher": SOURCE["teamId"], "signature": "native_verified", "compatibility": "not_tested"},
               "verifier": "macos_codesign_gatekeeper", "installation": "not_tested", "published": False}
    receipt_path = tmp_path / "verification.json"
    receipt_path.write_text(json.dumps(receipt))
    output = tmp_path / "public"
    output.mkdir(mode=0o755)
    (output / "health.json").write_text(json.dumps(P.HEALTH))
    return state, output, receipt_path, candidate


def publish_fixture(paths):
    state, output, receipt, _ = paths
    return P.publish(state, output, SOURCE["id"], receipt)


def assert_schema(value, schema):
    types = {"object": dict, "array": list, "string": str, "integer": int, "boolean": bool}
    assert type(value) is types[schema["type"]]
    if "enum" in schema:
        assert value in schema["enum"]
    if "pattern" in schema:
        import re
        assert re.fullmatch(schema["pattern"], value)
    if type(value) is int:
        assert value >= schema.get("minimum", value) and value <= schema.get("maximum", value)
    if isinstance(value, dict):
        assert set(schema.get("required", [])) <= set(value)
        if schema.get("additionalProperties") is False:
            assert set(value) <= set(schema["properties"])
        for key, item in value.items():
            assert_schema(item, schema["properties"][key])
    if isinstance(value, list):
        assert len(value) <= schema.get("maxItems", len(value))
        for item in value:
            assert_schema(item, schema["items"])


def test_publisher_exact_contract_and_idempotent_original_bytes(tmp_path):
    paths = prepared(tmp_path)
    state, root, _, candidate = paths
    result = publish_fixture(paths)
    assert result["published"] and not result["reused"]
    catalog_path = root / "apps/catalog.json"
    before = catalog_path.read_bytes()
    value = json.loads(before)
    frozen = json.loads((DIRECTORY.parents[1] / ".product-governance/contracts.json").read_text())["contracts"]
    schema = next(row["outputSchema"] for row in frozen if row["id"] == "vendor-download-catalog@v2")
    assert_schema(value, schema)
    artifact = value["artifacts"][0]
    target = P.artifact_path(root, artifact)
    assert target.read_bytes() == (state / candidate["path"]).read_bytes()
    assert target.stat().st_mode & 0o777 == 0o644
    assert catalog_path.stat().st_mode & 0o777 == 0o644
    assert json.loads((root / "health.json").read_text()) == {**P.HEALTH, "thirdPartyInstallersPublished": True}
    assert publish_fixture(paths)["reused"]
    assert catalog_path.read_bytes() == before
    assert not list((root / "apps").rglob(".object-*"))
    assert not list((root / "apps").rglob(".publish-*"))
    assert "InaccessiblePaths=-/srv/yeschoy-download/public" in (DIRECTORY / "yeschoy-vendor-sync.service").read_text()
    assert "publish.py" not in (DIRECTORY / "yeschoy-vendor-sync.service").read_text()


def test_publisher_needs_no_approval_file_and_marks_client_native_requirement(tmp_path):
    state, root, _, candidate = prepared(tmp_path)
    result = P.publish(state, root, SOURCE["id"])
    assert result == {
        "sourceId": SOURCE["id"],
        "sha256": candidate["sha256"],
        "published": True,
        "reused": False,
        "verification": "client_native_required",
    }
    catalog = json.loads((root / "apps/catalog.json").read_text())
    assert catalog["schemaVersion"] == 2
    assert catalog["artifacts"][0]["verification"] == "client_native_required"
    public_text = json.dumps(catalog)
    assert "approval" not in public_text and "basis" not in public_text and "operator" not in public_text


def test_publisher_disable_and_reenable_are_atomic_and_keep_objects(tmp_path, monkeypatch):
    paths = prepared(tmp_path)
    publish_fixture(paths)
    state, root, receipt, _ = paths
    public_object = next((root / "apps").rglob("*.zip"))
    before = public_object.read_bytes()
    result = P.disable(root)
    assert result == {"disabled": True, "previousArtifactCount": 1, "objectsDeleted": False}
    assert json.loads((root / "apps/catalog.json").read_text()) == {
        "schemaVersion": 2,
        "generatedAt": json.loads((root / "apps/catalog.json").read_text())["generatedAt"],
        "artifacts": [],
    }
    assert json.loads((root / "health.json").read_text())["thirdPartyInstallersPublished"] is False
    assert public_object.read_bytes() == before
    assert P.publish(state, root, SOURCE["id"], receipt)["published"]
    assert json.loads((root / "health.json").read_text())["thirdPartyInstallersPublished"] is True
    assert public_object.read_bytes() == before
    original = P.atomic_public_json
    monkeypatch.setattr(P, "atomic_public_json", lambda path, value: (_ for _ in ()).throw(OSError("fixture disable interruption")) if path.name == "catalog.json" else original(path, value))
    catalog_before = (root / "apps/catalog.json").read_bytes()
    with pytest.raises(OSError):
        P.disable(root)
    assert (root / "apps/catalog.json").read_bytes() == catalog_before
    assert json.loads((root / "health.json").read_text())["thirdPartyInstallersPublished"] is True


@pytest.mark.parametrize("target,changes", [
    ("receipt", {"sha256": "0" * 64}),
    ("receipt", {"source": "codex-macos-arm64"}),
    ("receipt", {"verifier": "self_asserted"}),
    ("receipt", {"checkedAt": "2999-01-01T00:00:00Z"}),
    ("native", {"signature": "pending_native_verification"}),
    ("native", {"publisher": "ATTACKER"}),
    ("native", {"identity": "Other.app"}),
    ("native", {"version": "1.2.4"}),
])
def test_publisher_rejects_mismatched_native_receipt_before_copy(tmp_path, target, changes):
    paths = prepared(tmp_path)
    file = paths[2]
    value = json.loads(file.read_text())
    (value["package"] if target == "native" else value).update(changes)
    file.write_text(json.dumps(value))
    with pytest.raises(S.SyncError):
        publish_fixture(paths)
    assert not (paths[1] / "apps/catalog.json").exists()
    assert not list((paths[1] / "apps").rglob("*.zip"))
    assert json.loads((paths[1] / "health.json").read_text())["thirdPartyInstallersPublished"] is False


def test_publisher_preserves_previous_on_corruption_downgrade_and_failed_update(tmp_path):
    paths = prepared(tmp_path / "first")
    publish_fixture(paths)
    root = paths[1]
    before = (root / "apps/catalog.json").read_bytes()
    public_object = next((root / "apps").rglob("*.zip"))
    public_bytes = public_object.read_bytes()
    older = prepared(tmp_path / "older", "1.2.2")
    with pytest.raises(S.SyncError, match="version_regression"):
        P.publish(older[0], root, SOURCE["id"], older[2])
    newer = prepared(tmp_path / "newer", "1.2.4")
    (newer[0] / newer[3]["path"]).write_bytes(b"corrupt")
    with pytest.raises(S.SyncError, match="staged_object_corrupt"):
        P.publish(newer[0], root, SOURCE["id"], newer[2])
    value = json.loads((paths[0] / "catalog.json").read_text())
    value["sources"][SOURCE["id"]]["status"] = "failed"
    (paths[0] / "catalog.json").write_text(json.dumps(value))
    # Replaying the identical already-published intent is allowed despite a
    # later private source failure; it cannot publish any new candidate.
    assert publish_fixture(paths)["reused"]
    assert (root / "apps/catalog.json").read_bytes() == before
    assert public_object.read_bytes() == public_bytes


def test_publisher_atomic_catalog_failure_retains_previous_and_replays(tmp_path, monkeypatch):
    paths = prepared(tmp_path / "first")
    publish_fixture(paths)
    root = paths[1]
    before = (root / "apps/catalog.json").read_bytes()
    older_object = next((root / "apps").rglob("*.zip"))
    newer = prepared(tmp_path / "newer", "1.2.4")
    real_replace = P.os.replace
    monkeypatch.setattr(P.os, "replace", lambda *args: (_ for _ in ()).throw(OSError("fixture atomic publication interruption")))
    with pytest.raises(OSError):
        P.publish(newer[0], root, SOURCE["id"], newer[2])
    assert (root / "apps/catalog.json").read_bytes() == before
    assert older_object.is_file()
    monkeypatch.setattr(P.os, "replace", real_replace)
    P.publish(newer[0], root, SOURCE["id"], newer[2])
    assert json.loads((root / "apps/catalog.json").read_text())["artifacts"][0]["version"] == "1.2.4"
    assert older_object.is_file() and len(list((root / "apps").rglob("*.zip"))) == 2


def test_publisher_lock_symlinks_and_corrupt_catalog_fail_closed(tmp_path):
    paths = prepared(tmp_path)
    root = paths[1]
    with P.locked(root):
        with pytest.raises(S.SyncError, match="publisher_already_running"):
            publish_fixture(paths)
    outside = tmp_path / "outside"
    outside.mkdir(mode=0o755)
    (root / "apps" / SOURCE["id"]).symlink_to(outside)
    with pytest.raises(S.SyncError, match="unsafe_symlink"):
        publish_fixture(paths)
    assert not list(outside.iterdir())
    (root / "apps" / SOURCE["id"]).unlink()
    publish_fixture(paths)
    path = root / "apps/catalog.json"
    path.write_bytes(b'{"schemaVersion":1,"schemaVersion":1}')
    before = path.read_bytes()
    with pytest.raises(S.SyncError):
        publish_fixture(paths)
    assert path.read_bytes() == before


def test_publisher_health_recovery_and_permission_boundary(tmp_path, monkeypatch):
    paths = prepared(tmp_path)
    original = P.atomic_public_json
    def fail_health(path, value):
        if path.name == "health.json":
            raise OSError("fixture health interruption")
        return original(path, value)
    monkeypatch.setattr(P, "atomic_public_json", fail_health)
    with pytest.raises(OSError):
        publish_fixture(paths)
    catalog = (paths[1] / "apps/catalog.json").read_bytes()
    assert json.loads(catalog)["artifacts"]
    assert json.loads((paths[1] / "health.json").read_text())["thirdPartyInstallersPublished"] is False
    # Real regression: a timer may fail after the catalog commit and before
    # operator replay. That status must not strand the public health projection.
    S.scan(paths[0], [SOURCE], Transport([Response(status=403, **{"cf-mitigated": "challenge"})]), True, S.MAX_BYTES)
    state_before = (paths[0] / "catalog.json").read_bytes()
    assert json.loads(state_before)["sources"][SOURCE["id"]]["status"] == "failed"
    public_before = next((paths[1] / "apps").rglob("*.zip")).read_bytes()
    monkeypatch.setattr(P, "atomic_public_json", original)
    assert publish_fixture(paths)["reused"]
    assert (paths[1] / "apps/catalog.json").read_bytes() == catalog
    assert (paths[0] / "catalog.json").read_bytes() == state_before
    assert next((paths[1] / "apps").rglob("*.zip")).read_bytes() == public_before
    assert json.loads((paths[1] / "health.json").read_text())["thirdPartyInstallersPublished"] is True
    paths[2].chmod(0o666)
    with pytest.raises(S.SyncError, match="operator_owned_path_required"):
        publish_fixture(paths)


def test_health_replay_never_promotes_a_new_failed_candidate(tmp_path):
    paths = prepared(tmp_path / "first")
    publish_fixture(paths)
    (paths[1] / "health.json").write_text(json.dumps(P.HEALTH))
    catalog_before = (paths[1] / "apps/catalog.json").read_bytes()
    new = prepared(tmp_path / "new", "1.2.4")
    S.scan(new[0], [SOURCE], Transport([Response(status=403, **{"cf-mitigated": "challenge"})]), True, S.MAX_BYTES)
    with pytest.raises(S.SyncError, match="stale_or_unavailable_candidate"):
        P.publish(new[0], paths[1], SOURCE["id"], new[2])
    assert (paths[1] / "apps/catalog.json").read_bytes() == catalog_before
    assert json.loads((paths[1] / "health.json").read_text())["thirdPartyInstallersPublished"] is False
    with pytest.raises(S.SyncError, match="stale_or_unavailable_candidate"):
        P.publish(new[0], paths[1], SOURCE["id"])
    assert json.loads((paths[1] / "health.json").read_text())["thirdPartyInstallersPublished"] is False


def test_publisher_preserves_other_slots_and_rejects_update_during_copy(tmp_path, monkeypatch):
    paths = prepared(tmp_path / "first")
    publish_fixture(paths)
    codex = next(source for source in S.sources() if source["id"] == "codex-macos-arm64")
    body = b"fixture" + b"koly" + b"\0" * 508
    S.scan(paths[0], [codex], Transport([Response(body), Response(body)]), True, S.MAX_BYTES)
    candidate = json.loads((paths[0] / "catalog.json").read_text())["sources"][codex["id"]]["candidate"]
    receipt = json.loads(paths[2].read_text())
    receipt.update({"source": codex["id"], "sha256": candidate["sha256"]})
    receipt["package"].update({"identity": codex["identity"], "publisher": codex["teamId"], "version": "9.8.7"})
    codex_receipt = tmp_path / "codex-verification.json"
    codex_receipt.write_text(json.dumps(receipt))
    P.publish(paths[0], paths[1], codex["id"], codex_receipt)
    public_catalog = paths[1] / "apps/catalog.json"
    before = public_catalog.read_bytes()
    rows = json.loads(before)["artifacts"]
    assert {row["sourceId"] for row in rows} == {SOURCE["id"], codex["id"]}
    newer = prepared(tmp_path / "newer", "1.2.4")
    real_copy = P.copy_immutable
    def changed(*args):
        real_copy(*args)
        value = json.loads((newer[0] / "catalog.json").read_text())
        value["sources"][SOURCE["id"]]["status"] = "failed"
        (newer[0] / "catalog.json").write_text(json.dumps(value))
    monkeypatch.setattr(P, "copy_immutable", changed)
    with pytest.raises(S.SyncError, match="staged_candidate_changed"):
        P.publish(newer[0], paths[1], SOURCE["id"], newer[2])
    assert public_catalog.read_bytes() == before
    assert all(P.artifact_path(paths[1], row).is_file() for row in rows)


@pytest.mark.parametrize("mutation", ["duplicate", "foreign_url", "wrong_publisher", "boolean_size", "unknown_key", "bad_object"])
def test_publisher_rejects_corrupt_existing_catalog_without_replacing_it(tmp_path, mutation):
    paths = prepared(tmp_path)
    publish_fixture(paths)
    path = paths[1] / "apps/catalog.json"
    value = json.loads(path.read_text())
    artifact = value["artifacts"][0]
    if mutation == "duplicate":
        value["artifacts"].append(dict(artifact))
    elif mutation == "foreign_url":
        artifact["url"] = artifact["url"].replace("ergou.qzz.io", "evil.test")
    elif mutation == "wrong_publisher":
        artifact["publisher"] = "UNTRUSTED"
    elif mutation == "boolean_size":
        artifact["size"] = True
    elif mutation == "unknown_key":
        artifact["installCommand"] = "do not execute"
    else:
        P.artifact_path(paths[1], artifact).write_bytes(b"corrupt")
    path.write_text(json.dumps(value))
    before = path.read_bytes()
    with pytest.raises(S.SyncError):
        publish_fixture(paths)
    assert path.read_bytes() == before


def test_msix_publication_requires_exact_compiled_publisher_for_both_verification_states(tmp_path):
    source = next(s for s in S.sources() if s["id"] == "codex-windows-x64")
    publisher = P.publisher(source)
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as package:
        package.writestr("AppxManifest.xml", f'<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="OpenAI.Codex" Version="1.2.3.4" ProcessorArchitecture="x64" Publisher="{publisher}"/></Package>')
        package.writestr("AppxSignature.p7x", b"fixture is not a real signature")
        package.writestr("AppxBlockMap.xml", "<BlockMap/>")
    state = S.private_root(tmp_path / "stage")
    body = buffer.getvalue()
    S.scan(state, [source], Transport([Response(body), Response(body)]), True, S.MAX_BYTES)
    candidate = json.loads((state / "catalog.json").read_text())["sources"][source["id"]]["candidate"]
    receipt = {"schemaVersion": 1, "source": source["id"], "sha256": candidate["sha256"], "checkedAt": "2026-01-01T00:00:00Z",
               "package": {"version": "1.2.3.4", "identity": source["identity"], "publisher": publisher, "signature": "native_verified", "compatibility": "not_tested"},
               "verifier": "windows_signtool_authenticode", "installation": "not_tested", "published": False}
    verification = tmp_path / "native.json"
    verification.write_text(json.dumps(receipt))
    root = tmp_path / "public"
    root.mkdir(mode=0o755)
    (root / "health.json").write_text(json.dumps(P.HEALTH))
    # A Windows receipt is operator evidence; merely finding the signature file
    # in the ZIP is not sufficient for this separate authorization boundary.
    wrong = {**receipt, "verifier": "macos_codesign_gatekeeper"}
    verification.write_text(json.dumps(wrong))
    with pytest.raises(S.SyncError, match="native_verification_required"):
        P.publish(state, root, source["id"], verification)
    verification.write_text(json.dumps(receipt))
    assert P.publish(state, root, source["id"], verification)["published"]
    artifact = json.loads((root / "apps/catalog.json").read_text())["artifacts"][0]
    assert artifact["publisher"] == publisher and artifact["format"] == "msix" and artifact["architecture"] == "x64"
    assert artifact["verification"] == "native_verified"
    client_root = tmp_path / "client-native-public"
    client_root.mkdir(mode=0o755)
    (client_root / "health.json").write_text(json.dumps(P.HEALTH))
    assert P.publish(state, client_root, source["id"])["verification"] == "client_native_required"
    client_artifact = json.loads((client_root / "apps/catalog.json").read_text())["artifacts"][0]
    assert client_artifact["publisher"] == publisher
    assert client_artifact["verification"] == "client_native_required"
