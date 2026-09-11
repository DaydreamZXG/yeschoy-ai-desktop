#!/usr/bin/env python3
"""Validate and switch only the Yeschoy origin Caddy configuration, with CAS backup."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

CONFIG = Path("/opt/yeschoy-download/Caddyfile.origin")
IMAGE = "sha256:af32e97399febea808609119bb21544d0265c58a02836576e32a2d082c262c17"
CONTAINER = "yeschoy-download-origin"


def run(*args):
    return subprocess.run(args, capture_output=True, text=True, check=True, timeout=60).stdout.strip()


def replace_in_place(value):
    # Existing container bind-mount pins this inode: atomic rename would leave
    # it reading the old configuration. Validate first, write/fsync, restart.
    with CONFIG.open("r+b") as file:
        file.write(value)
        file.truncate()
        file.flush()
        os.fsync(file.fileno())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate", required=True)
    parser.add_argument("--expected-sha256", required=True)
    parser.add_argument("--backup-dir", required=True)
    args = parser.parse_args()
    candidate, backup = Path(args.candidate), Path(args.backup_dir)
    assert candidate.is_absolute() and candidate.is_file() and not candidate.is_symlink()
    assert backup.is_absolute() and str(backup).startswith("/opt/yeschoy-download/backups/")
    assert not backup.exists() and not CONFIG.is_symlink()
    new = candidate.read_bytes()
    with open("/opt/yeschoy-download/publish.lock", "a+") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        old = CONFIG.read_bytes()
        assert hashlib.sha256(old).hexdigest() == args.expected_sha256, "Live configuration changed"
        assert run("docker", "inspect", "--format", "{{.Image}}", CONTAINER) == IMAGE
        run("docker", "run", "--rm", "--network", "none", "--read-only", "--user", "65532:65532",
            "--cap-drop", "ALL", "--cap-add", "NET_BIND_SERVICE", "--security-opt", "no-new-privileges:true",
            "--entrypoint", "caddy", "-v", f"{candidate}:/etc/caddy/Caddyfile:ro", IMAGE,
            "validate", "--config", "/etc/caddy/Caddyfile", "--adapter", "caddyfile")
        backup.mkdir(mode=0o700)
        (backup / "Caddyfile.origin").write_bytes(old)
        for path in (Path("/srv/yeschoy-download/public/health.json"),):
            (backup / path.name).write_bytes(path.read_bytes())
        try:
            replace_in_place(new)
            run("docker", "restart", "--time", "10", CONTAINER)
            assert run("docker", "inspect", "--format", "{{.State.Running}}", CONTAINER) == "true"
            # Verify legacy routes on the actual running origin, not just text.
            for path in ("/updates/stable.json", "/updates/beta.json"):
                for attempt in range(5):
                    response = subprocess.run(["docker", "exec", CONTAINER, "wget", "-S", "--spider", f"http://127.0.0.1:8080{path}"], capture_output=True, text=True, timeout=10)
                    if response.returncode == 0 and "HTTP/1.1 204" in response.stderr:
                        break
                    if attempt == 4:
                        raise RuntimeError("Live legacy route did not return 204")
                    time.sleep(1)
        except BaseException:
            replace_in_place(old)
            run("docker", "restart", "--time", "10", CONTAINER)
            raise
        print(json.dumps({"status": "activated", "container": CONTAINER,
                          "previousSha256": args.expected_sha256, "currentSha256": hashlib.sha256(new).hexdigest(),
                          "backup": str(backup), "sharedIngressChanged": False}))


if __name__ == "__main__":
    main()
