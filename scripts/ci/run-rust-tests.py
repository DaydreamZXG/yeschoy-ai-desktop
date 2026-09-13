"""Compile and verify the active native suite; never accept an empty green run."""

import json
from pathlib import Path
import re
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[2]
LIBRARY = "yeschoy_desktop_lib"
REQUIRED_TESTS = frozenset(
    {
        "connectivity_core::tests::diagnostics_contract_catalog_is_exact_and_fixed_to_tls_port",
        "connectivity_core::tests::diagnostics_contract_layer_catalog_is_fixed_and_ordered",
        "connectivity_core::tests::diagnostics_contract_request_id_rejects_network_or_path_input",
        "tool_discovery_core::tests::detected_version_is_exact_and_read_only",
        "tool_discovery_core::tests::fixed_catalog_and_empty_state",
        "tool_discovery_core::tests::multiple_installations_fail_closed",
        "tool_discovery_core::tests::probe_failure_and_timeout_are_isolated",
    }
)


class VerificationError(RuntimeError):
    pass


def run_command(argv: list[str], root: Path, timeout: int, *, cargo=False):
    # No shell, secret, home-directory or TEMP/TMP override. In particular,
    # Windows link.exe/mt.exe must keep using the native runner temp directory.
    result = subprocess.run(
        argv,
        cwd=root,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
    )
    if cargo:
        for line in result.stdout.splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(event, dict) and event.get("reason") == "compiler-message":
                print(event.get("message", {}).get("rendered", ""), end="")
    else:
        print(result.stdout, end="")
    print(result.stderr, end="", file=sys.stderr)
    if result.returncode != 0:
        raise VerificationError(f"{argv[0]} exited with {result.returncode}")
    return result.stdout


def select_test_binary(output: str) -> str:
    binaries = []
    for line in output.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (
            isinstance(event, dict)
            and event.get("reason") == "compiler-artifact"
            and event.get("target", {}).get("name") == LIBRARY
            and event.get("profile", {}).get("test") is True
            and isinstance(event.get("executable"), str)
            and event["executable"]
        ):
            binaries.append(event["executable"])
    if len(binaries) != 1:
        raise VerificationError(
            f"Expected one {LIBRARY} test binary; found {len(binaries)}"
        )
    return binaries[0]


def listed_tests(output: str) -> set[str]:
    names = re.findall(r"^([^\s]+): test$", output, flags=re.MULTILINE)
    if not names or len(names) != len(set(names)):
        raise VerificationError("Native test discovery is empty or duplicated")
    missing = REQUIRED_TESTS - set(names)
    if missing:
        raise VerificationError(f"Current candidate tests are missing: {sorted(missing)}")
    return set(names)


def verify_test_summary(output: str, expected_count: int) -> None:
    summaries = re.findall(
        r"^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; "
        r"(\d+) measured; (\d+) filtered out;",
        output,
        flags=re.MULTILINE,
    )
    if len(summaries) != 1 or tuple(map(int, summaries[0])) != (
        expected_count,
        0,
        0,
        0,
        0,
    ):
        raise VerificationError("Native tests did not all pass without skips or filters")


def verify(root: Path = ROOT) -> int:
    artifacts = run_command(
        [
            "cargo",
            "test",
            "--locked",
            "--manifest-path",
            "src-tauri/Cargo.toml",
            "--lib",
            "--no-run",
            "--message-format=json",
        ],
        root,
        timeout=2400,
        cargo=True,
    )
    binary = select_test_binary(artifacts)
    names = listed_tests(
        run_command([binary, "--list", "--format=terse"], root, timeout=120)
    )
    output = run_command(
        [binary, "--test-threads=1", "--color=never"], root, timeout=300
    )
    verify_test_summary(output, len(names))
    print(f"Verified {len(names)} current native tests: no failures, skips or filters.")
    return len(names)


def main() -> int:
    try:
        verify()
    except (VerificationError, OSError, subprocess.TimeoutExpired) as error:
        print(f"Native CI verification failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
