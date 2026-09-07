"""Guarded additive ingress deployment. Never imports credentials or changes DNS.

The shared Caddyfile is bind-mounted as a file, so preserve its inode. Persist a
root-only baseline first; validate before promotion and gracefully reload after
promotion. The running Caddy configuration remains active while bytes are copied.
"""
from __future__ import annotations

import argparse
import fcntl
import hashlib
import os
from pathlib import Path
import re
import stat

BEGIN = b"# BEGIN YESCHOY DOWNLOAD ORIGIN"
END = b"# END YESCHOY DOWNLOAD ORIGIN"


def digest(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def prepare(current: bytes, snippet: bytes) -> bytes:
    if snippet.count(BEGIN) != 1 or snippet.count(END) != 1:
        raise ValueError("invalid managed snippet")
    if not snippet.startswith(BEGIN) or not snippet.rstrip().endswith(END):
        raise ValueError("invalid managed snippet boundaries")
    if BEGIN in current or END in current:
        if current.count(BEGIN) != 1 or current.count(END) != 1:
            raise ValueError("duplicate managed block")
        start = current.index(BEGIN)
        if current[start:] != snippet:
            raise ValueError("managed block changed or not at end")
        prefix = current[:start]
        if b"ergou.qzz.io" in prefix:
            raise ValueError("existing host conflicts with managed block")
        return current
    if re.search(rb"ergou\.qzz\.io", current):
        raise ValueError("host already exists outside managed block")
    return current + (b"\n" if current.endswith(b"\n") else b"\n\n") + snippet


def guarded_replace(path: Path, expected: bytes, desired: bytes) -> bool:
    """Compare-and-replace regular file bytes under lock, keeping the bind inode."""
    fd = os.open(path, os.O_RDWR | os.O_NOFOLLOW)
    with os.fdopen(fd, "r+b") as handle:
        fcntl.flock(handle, fcntl.LOCK_EX)
        meta = os.fstat(handle.fileno())
        live = path.lstat()
        if not stat.S_ISREG(meta.st_mode) or (live.st_dev, live.st_ino) != (meta.st_dev, meta.st_ino):
            raise ValueError("ingress identity changed")
        current = handle.read()
        if current == desired:
            return False
        if current != expected:
            raise ValueError("ingress changed since preflight; refusing overwrite")
        handle.seek(0)
        handle.write(desired)
        handle.truncate()
        handle.flush()
        os.fsync(handle.fileno())
        return True


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=("prepare", "promote", "restore"))
    parser.add_argument("--current", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--snippet", type=Path, required=True)
    parser.add_argument("--expected-sha256", required=True)
    args = parser.parse_args()
    baseline = args.baseline.read_bytes()
    if digest(baseline) != args.expected_sha256:
        raise ValueError("baseline hash mismatch")
    candidate = prepare(baseline, args.snippet.read_bytes())
    if args.action == "prepare":
        current = args.current.read_bytes()
        if current not in (baseline, candidate):
            raise ValueError("current ingress differs from captured baseline")
        # This generated candidate is outside the shared ingress and public root.
        if args.candidate.is_symlink():
            raise ValueError("candidate must not be a symlink")
        try:
            with args.candidate.open("xb") as handle:
                handle.write(candidate)
                handle.flush()
                os.fsync(handle.fileno())
        except FileExistsError:
            if args.candidate.read_bytes() != candidate:
                raise ValueError("existing candidate differs from prepared bytes") from None
        changed = False
    else:
        if args.candidate.read_bytes() != candidate:
            raise ValueError("candidate changed after preparation")
        expected, desired = (baseline, candidate) if args.action == "promote" else (candidate, baseline)
        changed = guarded_replace(args.current, expected, desired)
    print(f"action={args.action} changed={str(changed).lower()} baseline={digest(baseline)} candidate={digest(candidate)}")


if __name__ == "__main__":
    main()
