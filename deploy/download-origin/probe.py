"""Read-only public-origin HTTP/TLS checks; never accesses account/model APIs."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

HOST = "ergou.qzz.io"
ROOT = Path(__file__).resolve().parent


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--origin-ip", help="Optional direct-origin IP; TLS still verifies ergou.qzz.io")
    parser.add_argument(
        "--expect-updates",
        choices=("auto", "published", "unpublished"),
        default="auto",
        help="Expected stable-channel state; auto trusts public health.",
    )
    args = parser.parse_args()
    fixture = (ROOT / "public/checks/download-test.txt").read_bytes()
    tests = [
        ("health", "/health.json", "GET", 200, None),
        ("stable-channel", "/updates/stable.json", "GET", None, None),
        ("beta-not-published", "/updates/beta.json", "GET", 204, None),
        ("download", "/checks/download-test.txt", "GET", 200, None),
        ("resume", "/checks/download-test.txt", "GET", 206, "bytes=8-39"),
        ("missing", "/not-published.exe", "GET", 404, None),
        ("hidden", "/releases/.env", "GET", 404, None),
        ("traversal", "/releases/../../health.json", "GET", 404, None),
        ("encoded-traversal", "/apps/%2e%2e/.env", "GET", 404, None),
        ("directory", "/apps/", "GET", 404, None),
        ("read-only", "/health.json", "POST", 405, None),
    ]
    results = []
    remote_health = None
    with tempfile.TemporaryDirectory(prefix="yeschoy-origin-probe-") as directory:
        output = Path(directory) / "body"
        headers = Path(directory) / "headers"
        for name, path, method, expected, byte_range in tests:
            argv = [
                "curl", "--silent", "--show-error", "--path-as-is", "--noproxy", "*",
                "--connect-timeout", "8", "--max-time", "20",
                "--request", method, "--output", str(output), "--dump-header", str(headers),
                "--write-out", "%{http_code}",
            ]
            if args.origin_ip:
                argv.extend(["--resolve", f"{HOST}:443:{args.origin_ip}"])
            if byte_range:
                argv.extend(["--header", f"Range: {byte_range}"])
            argv.append(f"https://{HOST}{path}")
            response = subprocess.run(argv, capture_output=True, text=True, timeout=25)
            if response.returncode:
                raise RuntimeError(f"{name}: curl failed ({response.returncode}): {response.stderr.strip()}")
            actual = int(response.stdout)
            body = output.read_bytes()
            header_text = headers.read_text().lower()
            # Cloudflare rejects malformed dot-segment URLs before forwarding
            # them. Direct-origin checks remain strict 404; public 400 must
            # come from Cloudflare and is accepted only for traversal probes.
            edge_rejected = (
                not args.origin_ip and name in ("traversal", "encoded-traversal")
                and actual == 400 and "server: cloudflare" in header_text
            )
            if name == "stable-channel":
                assert remote_health is not None, "health must be checked before stable channel"
                expected = 200 if remote_health["updatesPublished"] else 204
            assert actual == expected or edge_rejected, f"{name}: HTTP {actual}, expected {expected}"
            if name == "health":
                remote_health = json.loads(body)
                assert remote_health.get("schemaVersion") == 1
                assert remote_health.get("service") == "yeschoy-download-origin"
                assert remote_health.get("status") == "origin_ready"
                assert isinstance(remote_health.get("updatesPublished"), bool)
                assert isinstance(remote_health.get("thirdPartyInstallersPublished"), bool)
                if args.expect_updates != "auto":
                    assert remote_health["updatesPublished"] is (args.expect_updates == "published")
            if name == "stable-channel" and actual == 200:
                manifest = json.loads(body)
                assert isinstance(manifest.get("version"), str)
                platforms = manifest.get("platforms")
                assert isinstance(platforms, dict)
                assert set(platforms) == {
                    "darwin-aarch64", "darwin-x86_64", "windows-x86_64"
                }
                for platform in platforms.values():
                    assert platform["url"].startswith(
                        "https://ergou.qzz.io/updates/releases/"
                    )
                    assert platform["signature"].strip()
            if actual == 204:
                assert body == b"" and "cache-control: no-store" in header_text
            if name == "download":
                assert body == fixture
            if name == "resume":
                assert body == fixture[8:40]
                assert f"content-range: bytes 8-39/{len(fixture)}" in header_text
            results.append({"check": name, "status": actual, "bytes": len(body)})
    print(json.dumps({
        "host": HOST,
        "mode": "direct-origin" if args.origin_ip else "public-dns",
        "tlsVerification": True,
        "proxyEnvironmentIgnored": True,
        "updatesPublished": remote_health["updatesPublished"] if remote_health else None,
        "fixtureSha256": hashlib.sha256(fixture).hexdigest(),
        "checks": results,
    }, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
