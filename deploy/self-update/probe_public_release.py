"""Fail-closed public checks for a newly promoted Yeschoy release."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile
from urllib.parse import urlsplit

HOST = "ergou.qzz.io"
DOWNLOAD_ORIGIN_ROOT = Path(__file__).resolve().parents[1] / "download-origin"


def curl_request(
    *,
    url: str,
    output: Path,
    headers: Path,
    method: str = "GET",
    byte_range: str | None = None,
    origin_ip: str | None = None,
) -> tuple[int, bytes, str]:
    argv = [
        "curl",
        "--silent",
        "--show-error",
        "--path-as-is",
        "--noproxy",
        "*",
        "--connect-timeout",
        "8",
        "--max-time",
        "20",
        "--request",
        method,
        "--output",
        str(output),
        "--dump-header",
        str(headers),
        "--write-out",
        "%{http_code}",
    ]
    if origin_ip:
        argv.extend(["--resolve", f"{HOST}:443:{origin_ip}"])
    if byte_range:
        argv.extend(["--header", f"Range: {byte_range}"])
    argv.append(url)
    response = subprocess.run(argv, capture_output=True, text=True, timeout=25)
    if response.returncode:
        raise RuntimeError(
            f"curl failed ({response.returncode}) for {url}: "
            f"{response.stderr.strip()}"
        )
    return int(response.stdout), output.read_bytes(), headers.read_text().lower()


def validated_artifact_url(url: str) -> str:
    parsed = urlsplit(url)
    assert (
        parsed.scheme == "https"
        and parsed.hostname == HOST
        and parsed.port is None
        and not parsed.username
        and not parsed.password
        and not parsed.query
        and not parsed.fragment
        and parsed.path.startswith(("/updates/releases/", "/releases/yeschoy/"))
    ), "unsafe artifact URL"
    return url


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--origin-ip",
        help="Optional direct-origin IP; TLS still verifies ergou.qzz.io",
    )
    parser.add_argument(
        "--expect-updates",
        choices=("published",),
        default="published",
        help="This promotion probe only accepts a published stable channel.",
    )
    args = parser.parse_args()
    fixture = (DOWNLOAD_ORIGIN_ROOT / "public/checks/download-test.txt").read_bytes()
    tests = [
        ("health", "/health.json", "GET", 200, None),
        ("stable-channel", "/updates/stable.json", "GET", 200, None),
        ("download-manifest", "/releases/yeschoy.json", "GET", 200, None),
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
    results: list[dict[str, int | str]] = []
    published_artifacts: dict[str, int | None] = {}
    with tempfile.TemporaryDirectory(prefix="yeschoy-release-probe-") as directory:
        output = Path(directory) / "body"
        headers = Path(directory) / "headers"
        for name, path, method, expected, byte_range in tests:
            actual, body, header_text = curl_request(
                url=f"https://{HOST}{path}",
                output=output,
                headers=headers,
                method=method,
                byte_range=byte_range,
                origin_ip=args.origin_ip,
            )
            edge_rejected = (
                not args.origin_ip
                and name in ("traversal", "encoded-traversal")
                and actual == 400
                and "server: cloudflare" in header_text
            )
            assert actual == expected or edge_rejected, (
                f"{name}: HTTP {actual}, expected {expected}"
            )
            if name == "health":
                health = json.loads(body)
                assert health.get("schemaVersion") == 1
                assert health.get("service") == "yeschoy-download-origin"
                assert health.get("status") == "origin_ready"
                assert health.get("updatesPublished") is True
                assert isinstance(health.get("thirdPartyInstallersPublished"), bool)
            if name == "stable-channel":
                manifest = json.loads(body)
                assert isinstance(manifest.get("version"), str)
                platforms = manifest.get("platforms")
                assert isinstance(platforms, dict)
                assert set(platforms) == {
                    "darwin-aarch64",
                    "darwin-x86_64",
                    "windows-x86_64",
                }
                for platform in platforms.values():
                    url = validated_artifact_url(platform["url"])
                    assert url.startswith(f"https://{HOST}/updates/releases/")
                    assert platform["signature"].strip()
                    published_artifacts[url] = None
            if name == "download-manifest":
                manifest = json.loads(body)
                assert manifest.get("schemaVersion") == 1
                assert isinstance(manifest.get("version"), str)
                platforms = manifest.get("platforms")
                assert isinstance(platforms, dict)
                assert set(platforms) == {"macos-universal", "windows-x86_64"}
                for platform in platforms.values():
                    url = validated_artifact_url(platform["url"])
                    assert url.startswith(f"https://{HOST}/releases/yeschoy/")
                    assert len(platform["sha256"]) == 64
                    int(platform["sha256"], 16)
                    assert isinstance(platform["size"], int) and platform["size"] > 0
                    published_artifacts[url] = platform["size"]
            if actual == 204:
                assert body == b"" and "cache-control: no-store" in header_text
            if name == "download":
                assert body == fixture
            if name == "resume":
                assert body == fixture[8:40]
                assert f"content-range: bytes 8-39/{len(fixture)}" in header_text
            results.append({"check": name, "status": actual, "bytes": len(body)})

        # A manifest is not healthy while any referenced immutable object is
        # missing. A one-byte range checks routing and total size without
        # downloading every installer again during promotion.
        for index, (url, expected_size) in enumerate(
            sorted(published_artifacts.items()), 1
        ):
            actual, body, header_text = curl_request(
                url=url,
                output=output,
                headers=headers,
                byte_range="bytes=0-0",
                origin_ip=args.origin_ip,
            )
            assert actual == 206 and len(body) == 1, (
                f"artifact-{index}: HTTP {actual}, bytes {len(body)}"
            )
            match = re.search(
                r"^content-range:\s*bytes 0-0/([1-9][0-9]*)\s*$",
                header_text,
                re.MULTILINE,
            )
            assert match, f"artifact-{index}: missing bounded Content-Range"
            actual_size = int(match.group(1))
            if expected_size is not None:
                assert actual_size == expected_size, (
                    f"artifact-{index}: size {actual_size}, expected {expected_size}"
                )
            results.append(
                {"check": f"artifact-{index}", "status": actual, "bytes": len(body)}
            )

    print(
        json.dumps(
            {
                "host": HOST,
                "mode": "direct-origin" if args.origin_ip else "public-dns",
                "tlsVerification": True,
                "proxyEnvironmentIgnored": True,
                "updatesPublished": True,
                "fixtureSha256": hashlib.sha256(fixture).hexdigest(),
                "checks": results,
            },
            ensure_ascii=False,
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
