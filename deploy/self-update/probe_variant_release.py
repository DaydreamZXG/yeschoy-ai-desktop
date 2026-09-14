#!/usr/bin/env python3
"""Verify both public channels and the complete signed installer bytes over HTTPS."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile

from publish_variant import ORIGIN, VARIANTS, channel_config, sha256, verify_signature

LOCAL_PROXY = False

def fetch(url, directory, origin_ip=None, method="GET", byte_range=None):
    body, headers = directory / "response", directory / "headers"
    args = ["curl", "--silent", "--show-error", "--noproxy", "*", "--path-as-is",
            "--proto", "=https", "--connect-timeout", "10", "--max-time", "300",
            "--output", str(body), "--dump-header", str(headers),
            "--write-out", "%{http_code}", "--request", method]
    if LOCAL_PROXY:
        args[args.index("--noproxy") + 1] = ""
        args += ["--proxy", "http://127.0.0.1:10808"]
    if origin_ip:
        args += ["--resolve", f"ergou.qzz.io:443:{origin_ip}"]
    if byte_range:
        args += ["--header", f"Range: {byte_range}"]
    result = subprocess.run(args + [url], capture_output=True, timeout=310)
    if result.returncode:
        raise RuntimeError("HTTPS probe failed")
    return int(result.stdout), body, headers.read_text().lower()


def main():
    global LOCAL_PROXY
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--channel-config", required=True)
    parser.add_argument("--candidate", required=True, help="Curated local directory containing both variants")
    parser.add_argument("--minisign", default="minisign")
    parser.add_argument("--origin-ip")
    parser.add_argument("--use-local-proxy", action="store_true", help="Use the existing localhost:10808 proxy for bulk byte verification, not a direct-network test")
    args = parser.parse_args()
    LOCAL_PROXY = args.use_local_proxy
    assert not (LOCAL_PROXY and args.origin_ip)
    results = []
    with tempfile.TemporaryDirectory(prefix="yeschoy-public-probe-") as temporary:
        directory = Path(temporary)
        for path in ("/updates/stable.json", "/updates/beta.json"):
            status, body, headers = fetch(ORIGIN + path, directory, args.origin_ip)
            assert status == 204 and body.stat().st_size == 0 and "cache-control: no-store" in headers
            results.append({"path": path, "status": status})
        status, body, _ = fetch(ORIGIN + "/health.json", directory, args.origin_ip)
        health = json.loads(body.read_bytes())
        assert status == 200 and health["updatesPublished"] is True
        assert health["updateChannels"] == {"official": True, "partner": True}
        assert health["service"] == "yeschoy-download-origin" and health["thirdPartyInstallersPublished"] is True
        for variant in VARIANTS:
            config = channel_config(Path(args.channel_config), variant)
            status, body, _ = fetch(config["endpoint"], directory, args.origin_ip)
            update = json.loads(body.read_bytes())
            assert status == 200 and set(update["platforms"]) == {"windows-x86_64", "darwin-x86_64", "darwin-aarch64"}
            status, body, _ = fetch(f"{ORIGIN}/releases/yeschoy-{variant}.json", directory, args.origin_ip)
            download = json.loads(body.read_bytes())
            assert status == 200 and set(download["platforms"]) == {"windows-x86_64", "macos-universal"}
            for manifest in (update, download):
                assert manifest["schemaVersion"] == 2 and manifest["variant"] == variant and manifest["version"] == args.version
            assert update["platforms"]["darwin-x86_64"] == update["platforms"]["darwin-aarch64"]
            assert update["platforms"]["windows-x86_64"] == download["platforms"]["windows-x86_64"]
            entries = ((update["platforms"]["windows-x86_64"], "windows-x86_64-installer.exe"),
                       (update["platforms"]["darwin-aarch64"], "macos-universal.app.tar.gz"),
                       (download["platforms"]["macos-universal"], "macos-universal-installer.dmg"))
            for entry, suffix in entries:
                filename = f"yeschoy-{args.version}-{variant}-{suffix}"
                expected_url = f"{ORIGIN}/updates/releases/{variant}/{args.version}/{filename}"
                assert entry["url"] == expected_url
                local = Path(args.candidate) / filename
                assert entry["sha256"] == sha256(local) and entry["size"] == local.stat().st_size
                status, body, headers = fetch(expected_url, directory, args.origin_ip)
                assert status == 200 and body.stat().st_size == entry["size"] and sha256(body) == entry["sha256"]
                assert "immutable" in headers
                signature = directory / "release.sig"
                signature.write_text(entry["signature"])
                verify_signature(body, signature, config["publicKey"], args.minisign)
                status, body, headers = fetch(expected_url, directory, args.origin_ip, byte_range="bytes=0-0")
                assert status == 206 and body.stat().st_size == 1 and f"content-range: bytes 0-0/{entry['size']}" in headers
                results.append({"variant": variant, "file": filename, "sha256": entry["sha256"], "signatureVerified": True, "rangeVerified": True})
                print(json.dumps({"verified": filename}), flush=True)
            permanent_urls = {
                "windows-x86_64": f"{ORIGIN}/releases/{variant}/yeschoy-windows-x86_64-installer.exe",
                "macos-universal": f"{ORIGIN}/releases/{variant}/yeschoy-macos-universal-installer.dmg",
            }
            for target, url in permanent_urls.items():
                expected = download["platforms"][target]
                status, body, headers = fetch(url, directory, args.origin_ip)
                assert status == 200 and body.stat().st_size == expected["size"] and sha256(body) == expected["sha256"]
                assert "cache-control: no-store" in headers
                results.append({"variant": variant, "permanentDownload": url, "sha256": expected["sha256"]})
        for path, method, expected in (("/releases/.env", "GET", 404), ("/apps/", "GET", 404), ("/health.json", "POST", 405)):
            status, _, _ = fetch(ORIGIN + path, directory, args.origin_ip, method=method)
            assert status == expected
            results.append({"path": path, "status": status})
    print(json.dumps({"status": "verified", "version": args.version, "tlsVerified": True,
                      "mode": "existing-local-proxy" if LOCAL_PROXY else "direct-origin" if args.origin_ip else "public-dns", "checks": results}, indent=2))


if __name__ == "__main__":
    main()
