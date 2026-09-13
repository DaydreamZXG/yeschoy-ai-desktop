#!/usr/bin/env python3
"""Closed, machine-verifiable scenario test runner adapters.

Scenario commands are semantic declarations.  This module, rather than the
governance bundle, constructs the executable argv.  Test stdout is diagnostic
only; a scenario passes solely from adapter-owned machine evidence produced
inside an OS-enforced, read-only project-source sandbox.
"""

from __future__ import annotations

import argparse
import base64
import binascii
import copy
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
from typing import Any, Mapping
from urllib.parse import unquote, urlparse


SCHEMA_VERSION = 1
SUPPORTED_ADAPTERS = {"pytest-json@1", "vitest-json@1"}
REQUIRED_COMMAND_FIELDS = {"kind", "adapter", "selector", "cwd", "timeoutSeconds"}
OPTIONAL_COMMAND_FIELDS = {"id"}
MAX_CAPTURE_CHARS = 100_000
MAX_DIAGNOSTIC_CHARS = 20_000
ISOLATION_ADAPTER = "darwin-sandbox-exec-readonly-source@1"
SANDBOX_EXECUTABLE = Path("/usr/bin/sandbox-exec")
EXECUTION_TEMP_ROOT = Path(".product-governance/execution/tmp")
EXACT_VERSION = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?$")
NPM_INTEGRITY = re.compile(r"^(?:sha512|sha384|sha256)-[A-Za-z0-9+/]+={0,2}$")


def _canonical_bytes(value: object) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")


def _sha256_bytes(payload: bytes) -> str:
    return f"sha256:{hashlib.sha256(payload).hexdigest()}"


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65_536), b""):
            digest.update(chunk)
    return f"sha256:{digest.hexdigest()}"


def _is_within(path: Path, parent: Path) -> bool:
    try:
        path.relative_to(parent)
        return True
    except ValueError:
        return False


def _safe_relative_path(value: Any, *, allow_dot: bool) -> bool:
    if not isinstance(value, str) or not value or value != value.strip():
        return False
    if "\\" in value or "\x00" in value or "\n" in value or "\r" in value:
        return False
    candidate = PurePosixPath(value)
    if candidate.is_absolute() or ".." in candidate.parts:
        return False
    if any(token in value for token in ("*", "?", "{", "}")):
        return False
    if not allow_dot and candidate in {PurePosixPath("."), PurePosixPath("")}:
        return False
    return True


def _selector_path_part(adapter: str, selector: str) -> str:
    return selector.split("::", 1)[0] if adapter == "pytest-json@1" else selector


def validate_scenario_command(command: Any) -> list[str]:
    """Return every structural error in a semantic scenario command.

    Validation is deliberately closed: scenario commands cannot provide argv,
    expected exit codes, arbitrary runner flags, or compatibility fields.
    """

    errors: list[str] = []
    if not isinstance(command, dict):
        return ["scenario command must be an object"]

    fields = set(command)
    non_string_fields = [field for field in fields if not isinstance(field, str)]
    if non_string_fields:
        errors.append("scenario command field names must be strings")
    missing = REQUIRED_COMMAND_FIELDS - fields
    extra = fields - REQUIRED_COMMAND_FIELDS - OPTIONAL_COMMAND_FIELDS
    if missing:
        errors.append(f"scenario command is missing fields: {sorted(missing)}")
    if extra:
        errors.append(
            f"scenario command has unsupported fields: {sorted(repr(field) for field in extra)}"
        )

    command_id = command.get("id")
    if "id" in command and (not isinstance(command_id, str) or not command_id.strip()):
        errors.append("scenario command id must be a non-empty string when present")
    if command.get("kind") != "scenario_test":
        errors.append("scenario command kind must equal scenario_test")

    adapter = command.get("adapter")
    if adapter not in SUPPORTED_ADAPTERS:
        errors.append(
            "scenario command adapter must be pytest-json@1 or vitest-json@1"
        )

    cwd = command.get("cwd")
    if not _safe_relative_path(cwd, allow_dot=True):
        errors.append("scenario command cwd must be a traversal-free project-relative path")

    selector = command.get("selector")
    if not isinstance(selector, str) or not selector or selector != selector.strip():
        errors.append("scenario command selector must be one non-empty exact selector")
    elif selector.startswith("-") or "\x00" in selector or "\n" in selector or "\r" in selector:
        errors.append("scenario command selector cannot be an option or contain control characters")
    elif isinstance(adapter, str):
        if adapter == "vitest-json@1" and "::" in selector:
            errors.append("vitest-json@1 selector must be exactly one test file")
        path_part = _selector_path_part(adapter, selector)
        if not _safe_relative_path(path_part, allow_dot=False):
            errors.append(
                "scenario command selector path must be one traversal-free path inside cwd"
            )
        if adapter == "pytest-json@1" and "::" in selector:
            node_parts = selector.split("::")[1:]
            if any(not part.strip() for part in node_parts):
                errors.append("pytest-json@1 selector contains an empty node-id component")

    timeout = command.get("timeoutSeconds")
    if (
        not isinstance(timeout, int)
        or isinstance(timeout, bool)
        or not 1 <= timeout <= 3600
    ):
        errors.append("scenario command timeoutSeconds must be an integer from 1 to 3600")
    return errors


def _resolve_execution_paths(
    root: Path | str, command: Mapping[str, Any]
) -> tuple[Path, Path, Path]:
    project_root = Path(root).expanduser().resolve(strict=True)
    if not project_root.is_dir():
        raise ValueError("project root must be an existing directory")
    cwd = (project_root / str(command["cwd"])).resolve(strict=True)
    if not cwd.is_dir() or not _is_within(cwd, project_root):
        raise ValueError("scenario command cwd resolves outside the project or is not a directory")
    selector_part = _selector_path_part(str(command["adapter"]), str(command["selector"]))
    selector_path = (cwd / selector_part).resolve(strict=True)
    if not selector_path.is_file() or not _is_within(selector_path, cwd):
        raise ValueError("scenario selector must resolve to one file inside cwd")
    return project_root, cwd, selector_path


def _read_json_object(path: Path, label: str) -> dict[str, Any]:
    try:
        with path.open(encoding="utf-8") as handle:
            value = json.load(handle)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        raise ValueError(f"{label} is not readable JSON: {exc}") from exc
    if not isinstance(value, dict):
        raise ValueError(f"{label} must contain a JSON object")
    return value


def _process_environment(
    provided: Mapping[str, str] | None, writable_root: Path
) -> dict[str, str]:
    if provided is None:
        provided = {}
    if not isinstance(provided, Mapping) or not all(
        isinstance(key, str) and isinstance(value, str)
        for key, value in provided.items()
    ):
        raise ValueError("scenario environment must be a string-to-string mapping")
    environment = os.environ.copy()
    environment.update(provided)
    for name in (
        "BASH_ENV",
        "ENV",
        "NODE_OPTIONS",
        "NODE_PATH",
        "PYTHONHOME",
        "PYTHONPATH",
        "PYTHONSTARTUP",
        "PYTEST_ADDOPTS",
    ):
        environment.pop(name, None)
    writable = str(writable_root)
    environment.update(
        {
            "CI": "1",
            "FORCE_COLOR": "0",
            "NO_COLOR": "1",
            "TMPDIR": writable,
            "TMP": writable,
            "TEMP": writable,
            "XDG_CACHE_HOME": str(writable_root / "xdg-cache"),
            "npm_config_cache": str(writable_root / "npm-cache"),
            "PYTHONDONTWRITEBYTECODE": "1",
        }
    )
    return environment


def _sandbox_quote(value: str) -> str:
    return value.replace("\\", "\\\\").replace('"', '\\"')


def _sandbox_policy(writable_root: Path) -> str:
    writable = _sandbox_quote(str(writable_root.resolve()))
    return (
        "(version 1)\n"
        "(allow default)\n"
        "(deny file-write*\n"
        "  (require-not\n"
        "    (require-any\n"
        f'      (subpath "{writable}")\n'
        '      (literal "/dev/null"))))\n'
    )


def _isolation_metadata(project_root: Path, writable_root: Path) -> dict[str, Any]:
    if platform.system() != "Darwin":
        raise ValueError("no supported OS write-isolation adapter is available")
    if not SANDBOX_EXECUTABLE.is_file() or SANDBOX_EXECUTABLE.is_symlink():
        raise ValueError("Darwin sandbox-exec is unavailable or not a trusted regular file")
    execution_root = (project_root / EXECUTION_TEMP_ROOT).resolve()
    writable = writable_root.resolve()
    if not _is_within(writable, execution_root) or writable == execution_root:
        raise ValueError("scenario writable root is outside governance execution temp")
    policy = _sandbox_policy(writable)
    return {
        "adapter": ISOLATION_ADAPTER,
        "executable": str(SANDBOX_EXECUTABLE),
        "executableSha256": _sha256_file(SANDBOX_EXECUTABLE),
        "policySha256": _sha256_bytes(policy.encode("utf-8")),
        "projectRoot": str(project_root),
        "writableRoot": str(writable),
        "policyVersion": 1,
    }


def _sandbox_argv(runner_argv: list[str], isolation: Mapping[str, Any]) -> list[str]:
    writable = Path(str(isolation["writableRoot"]))
    policy = _sandbox_policy(writable)
    return [str(SANDBOX_EXECUTABLE), "-p", policy, *runner_argv]


def _run_sandboxed(
    argv: list[str],
    cwd: Path,
    environment: Mapping[str, str],
    timeout_seconds: int,
) -> dict[str, Any]:
    try:
        completed = subprocess.run(
            argv,
            cwd=cwd,
            env=dict(environment),
            check=False,
            capture_output=True,
            text=True,
            timeout=timeout_seconds,
        )
        return {
            "actualExitCode": completed.returncode,
            "timedOut": False,
            "stdout": completed.stdout[-MAX_CAPTURE_CHARS:],
            "stderr": completed.stderr[-MAX_CAPTURE_CHARS:],
        }
    except subprocess.TimeoutExpired as exc:
        stdout = (
            exc.stdout.decode(errors="replace")
            if isinstance(exc.stdout, bytes)
            else exc.stdout or ""
        )
        stderr = (
            exc.stderr.decode(errors="replace")
            if isinstance(exc.stderr, bytes)
            else exc.stderr or ""
        )
        return {
            "actualExitCode": None,
            "timedOut": True,
            "stdout": stdout[-MAX_CAPTURE_CHARS:],
            "stderr": stderr[-MAX_CAPTURE_CHARS:],
        }


class _PytestMachinePlugin:
    """Small internal pytest plugin that records outcomes, not presentation text."""

    def __init__(self) -> None:
        self.collected: list[str] = []
        self.deselected: list[str] = []
        self.collection_errors: list[dict[str, str]] = []
        self.tests: dict[str, dict[str, Any]] = {}
        self.root_path: str | None = None

    def pytest_collection_finish(self, session: Any) -> None:
        self.root_path = str(Path(session.config.rootpath).resolve())
        self.collected = [str(item.nodeid) for item in session.items]

    def pytest_deselected(self, items: list[Any]) -> None:
        self.deselected.extend(str(item.nodeid) for item in items)

    def pytest_collectreport(self, report: Any) -> None:
        if getattr(report, "failed", False):
            self.collection_errors.append(
                {
                    "nodeId": str(getattr(report, "nodeid", "<collection>")),
                    "diagnostic": str(getattr(report, "longrepr", "collection failed"))[
                        -MAX_DIAGNOSTIC_CHARS:
                    ],
                }
            )

    def pytest_runtest_logreport(self, report: Any) -> None:
        node_id = str(report.nodeid)
        test = self.tests.setdefault(node_id, {"nodeId": node_id, "phases": {}})
        phase: dict[str, Any] = {"outcome": str(report.outcome)}
        if hasattr(report, "wasxfail"):
            phase["wasXfail"] = str(report.wasxfail)
        test["phases"][str(report.when)] = phase


def _write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n",
        encoding="utf-8",
    )


def _pytest_child_main(selector: str, report_path: Path, basetemp: Path) -> int:
    try:
        import pytest  # type: ignore

        plugin = _PytestMachinePlugin()
        exit_code = pytest.main(
            [
                selector,
                "-o",
                "addopts=",
                "-p",
                "no:cacheprovider",
                f"--basetemp={basetemp}",
            ],
            plugins=[plugin],
        )
        module_path = Path(str(pytest.__file__)).resolve()
        try:
            distribution_version = importlib.metadata.version("pytest")
        except importlib.metadata.PackageNotFoundError:
            distribution_version = None
        machine_report = {
            "schemaVersion": SCHEMA_VERSION,
            "adapter": "pytest-json@1",
            "selector": selector,
            "runner": {
                "name": "pytest",
                "moduleVersion": str(getattr(pytest, "__version__", "")),
                "distributionVersion": distribution_version,
                "modulePath": str(module_path),
                "pythonExecutable": sys.executable,
                "pythonVersion": platform.python_version(),
            },
            "pytestExitCode": int(exit_code),
            "pytestRootPath": plugin.root_path,
            "collectedNodeIds": plugin.collected,
            "deselectedNodeIds": plugin.deselected,
            "collectionErrors": plugin.collection_errors,
            "tests": [plugin.tests[node_id] for node_id in sorted(plugin.tests)],
        }
        _write_json(report_path, machine_report)
        return int(exit_code)
    except BaseException as exc:  # child must leave a closed failure artifact
        _write_json(
            report_path,
            {
                "schemaVersion": SCHEMA_VERSION,
                "adapter": "pytest-json@1",
                "selector": selector,
                "runnerError": f"{type(exc).__name__}: {exc}"[-MAX_DIAGNOSTIC_CHARS:],
            },
        )
        return 4


def _pytest_runner(
    project_root: Path,
    cwd: Path,
    selector_path: Path,
    command: Mapping[str, Any],
    writable_root: Path,
    environment: Mapping[str, str],
    isolation: Mapping[str, Any],
) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    report_path = writable_root / "pytest-machine-report.json"
    basetemp = writable_root / "pytest-basetemp"
    runner_argv = [
        sys.executable,
        "-B",
        str(Path(__file__).resolve()),
        "__pytest_child",
        "--selector",
        str(command["selector"]),
        "--report",
        str(report_path),
        "--basetemp",
        str(basetemp),
    ]
    sandboxed_argv = _sandbox_argv(runner_argv, isolation)
    process = _run_sandboxed(
        sandboxed_argv,
        cwd,
        environment,
        int(command["timeoutSeconds"]),
    )
    machine_report: dict[str, Any]
    if report_path.is_file():
        try:
            machine_report = _read_json_object(report_path, "pytest machine report")
        except ValueError as exc:
            machine_report = {
                "schemaVersion": SCHEMA_VERSION,
                "adapter": "pytest-json@1",
                "selector": command["selector"],
                "runnerError": str(exc),
            }
    else:
        machine_report = {
            "schemaVersion": SCHEMA_VERSION,
            "adapter": "pytest-json@1",
            "selector": command["selector"],
            "runnerError": "pytest did not produce its machine report",
        }
    machine_runner = machine_report.get("runner")
    module_path: Path | None = None
    if isinstance(machine_runner, dict) and isinstance(machine_runner.get("modulePath"), str):
        candidate = Path(machine_runner["modulePath"])
        if candidate.is_file():
            module_path = candidate.resolve()
    python_path = Path(sys.executable).resolve()
    runner_metadata = {
        "adapter": "pytest-json@1",
        "declaredCwd": command["cwd"],
        "cwd": str(cwd),
        "selector": command["selector"],
        "selectorRealpath": str(selector_path),
        "runnerArgv": runner_argv,
        "sandboxedArgv": sandboxed_argv,
        "pythonExecutable": sys.executable,
        "pythonRealpath": str(python_path),
        "pythonExecutableSha256": _sha256_file(python_path),
        "pytestVersion": (
            machine_runner.get("moduleVersion") if isinstance(machine_runner, dict) else None
        ),
        "pytestDistributionVersion": (
            machine_runner.get("distributionVersion")
            if isinstance(machine_runner, dict)
            else None
        ),
        "pytestModulePath": str(module_path) if module_path is not None else None,
        "pytestModuleSha256": _sha256_file(module_path) if module_path is not None else None,
        "isolation": dict(isolation),
    }
    return runner_metadata, machine_report, process


def _exact_vitest_version(package_json: Mapping[str, Any]) -> str:
    dependency_values: list[Any] = []
    for field in ("devDependencies", "dependencies"):
        dependencies = package_json.get(field)
        if isinstance(dependencies, dict) and "vitest" in dependencies:
            dependency_values.append(dependencies["vitest"])
    if len(dependency_values) != 1:
        raise ValueError("package.json must declare vitest exactly once")
    version = dependency_values[0]
    if not isinstance(version, str) or not EXACT_VERSION.fullmatch(version):
        raise ValueError("package.json must pin vitest to one exact semantic version")
    return version


def _verified_npm_package_tree(
    package_dir: Path, integrity: str
) -> dict[str, Any]:
    algorithm, encoded_digest = integrity.split("-", 1)
    try:
        expected_digest = base64.b64decode(encoded_digest, validate=True)
    except (binascii.Error, ValueError) as exc:
        raise ValueError("locked vitest integrity is not valid base64") from exc
    cache_value = os.environ.get("npm_config_cache")
    cache_root = (
        Path(cache_value).expanduser().resolve()
        if cache_value
        else (Path.home() / ".npm").resolve()
    )
    digest_hex = expected_digest.hex()
    tarball = (
        cache_root
        / "_cacache"
        / "content-v2"
        / algorithm
        / digest_hex[:2]
        / digest_hex[2:4]
        / digest_hex[4:]
    )
    if not tarball.is_file() or tarball.is_symlink():
        raise ValueError("the integrity-addressed Vitest npm tarball is unavailable")
    hasher = hashlib.new(algorithm)
    with tarball.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65_536), b""):
            hasher.update(chunk)
    if hasher.digest() != expected_digest:
        raise ValueError("the cached Vitest npm tarball fails its lock integrity digest")

    archive_hashes: dict[str, str] = {}
    try:
        with tarfile.open(tarball, mode="r:*") as archive:
            for member in archive.getmembers():
                if member.isdir():
                    continue
                if not member.isfile() or not member.name.startswith("package/"):
                    raise ValueError("the locked Vitest tarball contains an unsupported entry")
                relative = member.name.removeprefix("package/")
                relative_path = PurePosixPath(relative)
                if (
                    not relative
                    or relative_path.is_absolute()
                    or ".." in relative_path.parts
                    or relative in archive_hashes
                ):
                    raise ValueError("the locked Vitest tarball contains an unsafe path")
                extracted = archive.extractfile(member)
                if extracted is None:
                    raise ValueError("the locked Vitest tarball contains an unreadable file")
                archive_hashes[relative] = _sha256_bytes(extracted.read())
    except (OSError, tarfile.TarError) as exc:
        raise ValueError(f"the integrity-addressed Vitest tarball is unreadable: {exc}") from exc

    installed_hashes: dict[str, str] = {}
    for path in package_dir.rglob("*"):
        if path.is_symlink():
            raise ValueError("the installed Vitest package tree cannot contain symlinks")
        if path.is_file():
            relative = path.relative_to(package_dir).as_posix()
            installed_hashes[relative] = _sha256_file(path)
    if installed_hashes != archive_hashes:
        raise ValueError("the installed Vitest package tree differs from its integrity tarball")
    return {
        "integrityAlgorithm": algorithm,
        "integrityTarballPath": str(tarball.resolve()),
        "integrityTarballSha256": _sha256_file(tarball),
        "installedPackageTreeSha256": _sha256_bytes(_canonical_bytes(installed_hashes)),
        "integrityVerified": True,
    }


def _vitest_installation(cwd: Path) -> dict[str, Any]:
    manifest_path = cwd / "package.json"
    lockfile_path = cwd / "package-lock.json"
    manifest = _read_json_object(manifest_path, "package.json")
    expected_version = _exact_vitest_version(manifest)
    lockfile = _read_json_object(lockfile_path, "package-lock.json")
    lockfile_version = lockfile.get("lockfileVersion")
    if (
        not isinstance(lockfile_version, int)
        or isinstance(lockfile_version, bool)
        or lockfile_version < 2
    ):
        raise ValueError("package-lock.json lockfileVersion must be at least 2")
    packages = lockfile.get("packages")
    if not isinstance(packages, dict):
        raise ValueError("package-lock.json must contain a packages map")
    root_lock = packages.get("")
    if not isinstance(root_lock, dict):
        raise ValueError("package-lock.json is missing its root package entry")
    root_versions: list[Any] = []
    for field in ("devDependencies", "dependencies"):
        dependencies = root_lock.get(field)
        if isinstance(dependencies, dict) and "vitest" in dependencies:
            root_versions.append(dependencies["vitest"])
    if root_versions != [expected_version]:
        raise ValueError("package-lock root vitest declaration does not equal package.json")
    locked = packages.get("node_modules/vitest")
    if not isinstance(locked, dict):
        raise ValueError("package-lock.json has no node_modules/vitest lock entry")
    locked_version = locked.get("version")
    integrity = locked.get("integrity")
    if locked_version != expected_version:
        raise ValueError("locked vitest version does not equal the exact manifest version")
    if not isinstance(integrity, str) or not NPM_INTEGRITY.fullmatch(integrity):
        raise ValueError("locked vitest entry lacks a supported integrity digest")

    package_dir = cwd / "node_modules" / "vitest"
    package_manifest_path = package_dir / "package.json"
    cli_path = package_dir / "vitest.mjs"
    if package_dir.is_symlink() or package_manifest_path.is_symlink() or cli_path.is_symlink():
        raise ValueError("installed vitest package and CLI cannot be symlinks")
    package_realpath = package_dir.resolve(strict=True)
    if package_realpath != package_dir.absolute() or not _is_within(package_realpath, cwd):
        raise ValueError("installed vitest package resolves outside cwd")
    cli_realpath = cli_path.resolve(strict=True)
    if not cli_realpath.is_file() or not _is_within(cli_realpath, package_realpath):
        raise ValueError("installed vitest CLI resolves outside its locked package")
    installed_manifest = _read_json_object(package_manifest_path, "installed vitest package.json")
    if (
        installed_manifest.get("name") != "vitest"
        or installed_manifest.get("version") != expected_version
    ):
        raise ValueError("installed vitest identity does not equal the locked package identity")
    integrity_metadata = _verified_npm_package_tree(package_realpath, integrity)

    return {
        "manifestPath": str(manifest_path.resolve()),
        "manifestSha256": _sha256_file(manifest_path),
        "lockfilePath": str(lockfile_path.resolve()),
        "lockfileSha256": _sha256_file(lockfile_path),
        "lockfileVersion": lockfile_version,
        "packageVersion": expected_version,
        "lockIntegrity": integrity,
        "installedPackagePath": str(package_realpath),
        "installedPackageManifestPath": str(package_manifest_path.resolve()),
        "installedPackageManifestSha256": _sha256_file(package_manifest_path),
        "cliPath": str(cli_realpath),
        "cliSha256": _sha256_file(cli_realpath),
        **integrity_metadata,
    }


def _node_executable(environment: Mapping[str, str]) -> tuple[Path, str]:
    discovered = shutil.which("node", path=environment.get("PATH"))
    if not discovered:
        raise ValueError("node executable is unavailable")
    executable = Path(discovered).resolve(strict=True)
    if not executable.is_file():
        raise ValueError("node executable does not resolve to a regular file")
    result = subprocess.run(
        [str(executable), "--version"],
        check=False,
        capture_output=True,
        text=True,
        timeout=10,
        env=dict(environment),
    )
    version = result.stdout.strip()
    if result.returncode != 0 or not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", version):
        raise ValueError("node executable did not report a supported version")
    return executable, version


def _vitest_runner(
    project_root: Path,
    cwd: Path,
    selector_path: Path,
    command: Mapping[str, Any],
    writable_root: Path,
    environment: Mapping[str, str],
    isolation: Mapping[str, Any],
) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    installation = _vitest_installation(cwd)
    node_path, node_version = _node_executable(environment)
    report_path = writable_root / "vitest-machine-report.json"
    runner_argv = [
        str(node_path),
        installation["cliPath"],
        "run",
        str(command["selector"]),
        "--reporter=json",
        f"--outputFile={report_path}",
        "--cache=false",
    ]
    sandboxed_argv = _sandbox_argv(runner_argv, isolation)
    process = _run_sandboxed(
        sandboxed_argv,
        cwd,
        environment,
        int(command["timeoutSeconds"]),
    )
    if report_path.is_file():
        try:
            machine_report = _read_json_object(report_path, "Vitest JSON report")
        except ValueError as exc:
            machine_report = {
                "schemaVersion": SCHEMA_VERSION,
                "adapter": "vitest-json@1",
                "runnerError": str(exc),
            }
    else:
        machine_report = {
            "schemaVersion": SCHEMA_VERSION,
            "adapter": "vitest-json@1",
            "runnerError": "Vitest did not produce its JSON report",
        }
    runner_metadata = {
        "adapter": "vitest-json@1",
        "declaredCwd": command["cwd"],
        "cwd": str(cwd),
        "selector": command["selector"],
        "selectorRealpath": str(selector_path),
        "runnerArgv": runner_argv,
        "sandboxedArgv": sandboxed_argv,
        "nodeExecutable": str(node_path),
        "nodeExecutableSha256": _sha256_file(node_path),
        "nodeVersion": node_version,
        **installation,
        "isolation": dict(isolation),
    }
    return runner_metadata, machine_report, process


def _pytest_node_matches(selector: str, node_id: str) -> bool:
    if "::" in selector:
        return (
            node_id == selector
            or node_id.startswith(f"{selector}[")
            or node_id.startswith(f"{selector}::")
        )
    return node_id == selector or node_id.startswith(f"{selector}::")


def _pytest_selector_for_root(
    selector: str, expected_cwd: Path, root_path_value: str
) -> str | None:
    """Normalize a declared selector to the node-id emitted by pytest.

    Pytest reports node IDs relative to its discovered ``rootdir``.  A governed
    selector is deliberately relative to the command cwd, so a nested
    ``pyproject.toml`` can make the two valid spellings differ (for example,
    ``backend/tests/test_api.py`` versus ``tests/test_api.py``).  Resolve both
    roots and derive the only admissible rebased spelling instead of accepting
    suffix matches.
    """

    path_part, *node_parts = selector.split("::")
    try:
        root_path = Path(root_path_value).resolve(strict=True)
        selector_path = (expected_cwd / path_part).resolve(strict=True)
        if not root_path.is_dir() or not _is_within(root_path, expected_cwd):
            return None
        relative_path = selector_path.relative_to(root_path).as_posix()
    except (OSError, RuntimeError, ValueError):
        return None
    return "::".join([relative_path, *node_parts])


def _verify_pytest_machine_report(
    command: Mapping[str, Any], report: Any, expected_cwd: Path
) -> int | None:
    if not isinstance(report, dict):
        return None
    if (
        report.get("schemaVersion") != SCHEMA_VERSION
        or report.get("adapter") != "pytest-json@1"
        or report.get("selector") != command.get("selector")
        or report.get("pytestExitCode") != 0
        or report.get("runnerError") is not None
    ):
        return None
    runner = report.get("runner")
    if not isinstance(runner, dict):
        return None
    for field in (
        "moduleVersion",
        "distributionVersion",
        "modulePath",
        "pythonExecutable",
        "pythonVersion",
    ):
        if not isinstance(runner.get(field), str) or not runner[field]:
            return None
    if runner["moduleVersion"] != runner["distributionVersion"]:
        return None
    collected = report.get("collectedNodeIds")
    deselected = report.get("deselectedNodeIds")
    collection_errors = report.get("collectionErrors")
    tests = report.get("tests")
    if (
        not isinstance(collected, list)
        or not collected
        or not all(isinstance(item, str) and item for item in collected)
        or len(set(collected)) != len(collected)
        or deselected != []
        or collection_errors != []
        or not isinstance(tests, list)
    ):
        return None
    selector = str(command["selector"])
    pytest_root_path = report.get("pytestRootPath")
    if pytest_root_path is not None:
        if not isinstance(pytest_root_path, str) or not pytest_root_path:
            return None
        normalized_selector = _pytest_selector_for_root(
            selector, expected_cwd, pytest_root_path
        )
        if normalized_selector is None:
            return None
        selector = normalized_selector
    if not all(_pytest_node_matches(selector, item) for item in collected):
        return None
    tests_by_id: dict[str, dict[str, Any]] = {}
    for item in tests:
        if not isinstance(item, dict):
            return None
        node_id = item.get("nodeId")
        if not isinstance(node_id, str) or node_id in tests_by_id:
            return None
        tests_by_id[node_id] = item
    if set(tests_by_id) != set(collected):
        return None
    for node_id in collected:
        phases = tests_by_id[node_id].get("phases")
        if not isinstance(phases, dict) or set(phases) != {"setup", "call", "teardown"}:
            return None
        for phase_name in ("setup", "call", "teardown"):
            phase = phases.get(phase_name)
            if not isinstance(phase, dict) or set(phase) != {"outcome"}:
                return None
            if phase.get("outcome") != "passed":
                return None
    return len(collected)


def _report_test_file(value: str, cwd: Path) -> Path | None:
    parsed = urlparse(value)
    if parsed.scheme:
        if parsed.scheme != "file":
            return None
        candidate = Path(unquote(parsed.path))
    else:
        candidate = Path(value)
    if not candidate.is_absolute():
        candidate = cwd / candidate
    try:
        return candidate.resolve(strict=True)
    except (OSError, RuntimeError):
        return None


def _integer_field(value: Any) -> int | None:
    return value if isinstance(value, int) and not isinstance(value, bool) else None


def _verify_vitest_machine_report(
    command: Mapping[str, Any], metadata: Mapping[str, Any], report: Any
) -> int | None:
    if not isinstance(report, dict) or report.get("success") is not True:
        return None
    required_zero = (
        "numFailedTestSuites",
        "numPendingTestSuites",
        "numFailedTests",
        "numPendingTests",
        "numTodoTests",
    )
    if any(_integer_field(report.get(field)) != 0 for field in required_zero):
        return None
    runtime_error_suites = report.get("numRuntimeErrorTestSuites", 0)
    if _integer_field(runtime_error_suites) != 0:
        return None
    total_tests = _integer_field(report.get("numTotalTests"))
    passed_tests = _integer_field(report.get("numPassedTests"))
    total_suites = _integer_field(report.get("numTotalTestSuites"))
    passed_suites = _integer_field(report.get("numPassedTestSuites"))
    if (
        total_tests is None
        or total_tests <= 0
        or passed_tests != total_tests
        or total_suites is None
        or total_suites <= 0
        or passed_suites != total_suites
    ):
        return None
    test_results = report.get("testResults")
    if not isinstance(test_results, list) or not test_results:
        return None
    cwd_value = metadata.get("cwd")
    selector_realpath = metadata.get("selectorRealpath")
    if not isinstance(cwd_value, str) or not isinstance(selector_realpath, str):
        return None
    cwd = Path(cwd_value)
    expected_file = Path(selector_realpath)
    assertion_count = 0
    observed_files: set[Path] = set()
    for suite in test_results:
        if not isinstance(suite, dict) or suite.get("status") != "passed":
            return None
        name = suite.get("name")
        if not isinstance(name, str):
            return None
        observed = _report_test_file(name, cwd)
        if observed is None:
            return None
        observed_files.add(observed)
        assertions = suite.get("assertionResults")
        if not isinstance(assertions, list) or not assertions:
            return None
        for assertion in assertions:
            if not isinstance(assertion, dict) or assertion.get("status") != "passed":
                return None
            assertion_count += 1
    if observed_files != {expected_file} or assertion_count != total_tests:
        return None
    return total_tests


def _verify_isolation(metadata: Mapping[str, Any]) -> dict[str, Any] | None:
    isolation = metadata.get("isolation")
    if not isinstance(isolation, dict):
        return None
    if (
        isolation.get("adapter") != ISOLATION_ADAPTER
        or isolation.get("executable") != str(SANDBOX_EXECUTABLE)
        or isolation.get("policyVersion") != 1
    ):
        return None
    project_value = isolation.get("projectRoot")
    writable_value = isolation.get("writableRoot")
    if not isinstance(project_value, str) or not isinstance(writable_value, str):
        return None
    project = Path(project_value)
    writable = Path(writable_value)
    execution_root = (project / EXECUTION_TEMP_ROOT).resolve()
    if (
        not project.is_absolute()
        or not writable.is_absolute()
        or writable == execution_root
        or not _is_within(writable, execution_root)
    ):
        return None
    try:
        executable_hash = _sha256_file(SANDBOX_EXECUTABLE)
    except OSError:
        return None
    if isolation.get("executableSha256") != executable_hash:
        return None
    policy = _sandbox_policy(writable)
    policy_hash = _sha256_bytes(policy.encode("utf-8"))
    if isolation.get("policySha256") != policy_hash:
        return None
    sandboxed = metadata.get("sandboxedArgv")
    runner_argv = metadata.get("runnerArgv")
    if not isinstance(runner_argv, list) or not all(isinstance(item, str) for item in runner_argv):
        return None
    if sandboxed != [str(SANDBOX_EXECUTABLE), "-p", policy, *runner_argv]:
        return None
    return {
        "adapter": ISOLATION_ADAPTER,
        "policySha256": policy_hash,
        "executableSha256": executable_hash,
    }


def _metadata_file_hash_matches(
    metadata: Mapping[str, Any], path_field: str, hash_field: str
) -> bool:
    path_value = metadata.get(path_field)
    expected_hash = metadata.get(hash_field)
    if not isinstance(path_value, str) or not isinstance(expected_hash, str):
        return False
    path = Path(path_value)
    try:
        return path.is_file() and _sha256_file(path) == expected_hash
    except OSError:
        return False


def _declared_temp_path_is_inside(
    value: Any, writable_root: Path, expected_name: str
) -> bool:
    if not isinstance(value, str):
        return False
    try:
        path = Path(value).resolve()
    except (OSError, RuntimeError):
        return False
    return path.name == expected_name and _is_within(path, writable_root)


def _proof_from_result(
    command: Mapping[str, Any], result: Any
) -> dict[str, Any] | None:
    if validate_scenario_command(command) or not isinstance(result, dict):
        return None
    if result.get("schemaVersion") != SCHEMA_VERSION or result.get("declaredCommand") != command:
        return None
    if result.get("validationErrors") != []:
        return None
    metadata = result.get("runnerMetadata")
    report = result.get("machineReport")
    process = result.get("process")
    if not isinstance(metadata, dict) or not isinstance(process, dict):
        return None
    if (
        metadata.get("adapter") != command.get("adapter")
        or metadata.get("declaredCwd") != command.get("cwd")
        or metadata.get("selector") != command.get("selector")
        or process.get("actualExitCode") != 0
        or process.get("timedOut") is not False
    ):
        return None
    isolation_proof = _verify_isolation(metadata)
    if isolation_proof is None:
        return None
    isolation = metadata["isolation"]
    try:
        project_root = Path(str(isolation["projectRoot"])).resolve(strict=True)
        expected_cwd = (project_root / str(command["cwd"])).resolve(strict=True)
        expected_selector = (
            expected_cwd
            / _selector_path_part(str(command["adapter"]), str(command["selector"]))
        ).resolve(strict=True)
    except (KeyError, OSError, RuntimeError):
        return None
    if (
        str(expected_cwd) != metadata.get("cwd")
        or str(expected_selector) != metadata.get("selectorRealpath")
        or not expected_selector.is_file()
        or not _is_within(expected_selector, expected_cwd)
    ):
        return None
    writable_root = Path(str(isolation["writableRoot"])).resolve()
    adapter = command["adapter"]
    if adapter == "pytest-json@1":
        passed_count = _verify_pytest_machine_report(command, report, expected_cwd)
        runner_argv = metadata.get("runnerArgv")
        expected_prefix = [
            metadata.get("pythonExecutable"),
            "-B",
            str(Path(__file__).resolve()),
            "__pytest_child",
            "--selector",
            command["selector"],
            "--report",
        ]
        if (
            passed_count is None
            or not isinstance(runner_argv, list)
            or runner_argv[: len(expected_prefix)] != expected_prefix
            or len(runner_argv) != len(expected_prefix) + 3
            or runner_argv[-2] != "--basetemp"
            or not _declared_temp_path_is_inside(
                runner_argv[len(expected_prefix)],
                writable_root,
                "pytest-machine-report.json",
            )
            or not _declared_temp_path_is_inside(
                runner_argv[-1], writable_root, "pytest-basetemp"
            )
            or not _metadata_file_hash_matches(
                metadata, "pythonRealpath", "pythonExecutableSha256"
            )
            or not _metadata_file_hash_matches(
                metadata, "pytestModulePath", "pytestModuleSha256"
            )
        ):
            return None
        machine_runner = report.get("runner") if isinstance(report, dict) else None
        if not isinstance(machine_runner, dict):
            return None
        if (
            metadata.get("pytestVersion") != machine_runner.get("moduleVersion")
            or metadata.get("pytestDistributionVersion")
            != machine_runner.get("distributionVersion")
            or metadata.get("pytestModulePath") != machine_runner.get("modulePath")
            or metadata.get("pythonExecutable") != machine_runner.get("pythonExecutable")
            or not isinstance(metadata.get("pytestModuleSha256"), str)
        ):
            return None
    elif adapter == "vitest-json@1":
        passed_count = _verify_vitest_machine_report(command, metadata, report)
        runner_argv = metadata.get("runnerArgv")
        try:
            current_installation = _vitest_installation(expected_cwd)
        except (OSError, RuntimeError, TypeError, ValueError):
            return None
        installation_fields = {
            "manifestPath",
            "manifestSha256",
            "lockfilePath",
            "lockfileSha256",
            "lockfileVersion",
            "packageVersion",
            "lockIntegrity",
            "installedPackagePath",
            "installedPackageManifestPath",
            "installedPackageManifestSha256",
            "cliPath",
            "cliSha256",
            "integrityAlgorithm",
            "integrityTarballPath",
            "integrityTarballSha256",
            "installedPackageTreeSha256",
            "integrityVerified",
        }
        if (
            passed_count is None
            or not isinstance(runner_argv, list)
            or runner_argv[:4]
            != [
                metadata.get("nodeExecutable"),
                metadata.get("cliPath"),
                "run",
                command["selector"],
            ]
            or len(runner_argv) != 7
            or runner_argv[4] != "--reporter=json"
            or not isinstance(runner_argv[5], str)
            or not runner_argv[5].startswith("--outputFile=")
            or not _declared_temp_path_is_inside(
                runner_argv[5].split("=", 1)[1],
                writable_root,
                "vitest-machine-report.json",
            )
            or runner_argv[6] != "--cache=false"
            or metadata.get("packageVersion") is None
            or metadata.get("lockIntegrity") is None
            or not _metadata_file_hash_matches(
                metadata, "nodeExecutable", "nodeExecutableSha256"
            )
            or not _metadata_file_hash_matches(metadata, "cliPath", "cliSha256")
            or not _metadata_file_hash_matches(
                metadata, "manifestPath", "manifestSha256"
            )
            or not _metadata_file_hash_matches(
                metadata, "lockfilePath", "lockfileSha256"
            )
            or not _metadata_file_hash_matches(
                metadata,
                "installedPackageManifestPath",
                "installedPackageManifestSha256",
            )
            or any(
                metadata.get(field) != current_installation.get(field)
                for field in installation_fields
            )
        ):
            return None
    else:
        return None
    return {
        "schemaVersion": SCHEMA_VERSION,
        "adapter": adapter,
        "selector": command["selector"],
        "cwd": command["cwd"],
        "executedPassingTests": passed_count,
        "actualExitCode": 0,
        "runnerMetadataSha256": _sha256_bytes(_canonical_bytes(metadata)),
        "machineReportSha256": _sha256_bytes(_canonical_bytes(report)),
        "isolation": isolation_proof,
    }


def verify_scenario_result(command: Any, result: Any) -> dict[str, Any] | None:
    """Recompute a scenario proof, or return ``None`` on any inconsistency."""

    if not isinstance(command, dict) or not isinstance(result, dict):
        return None
    if result.get("passed") is not True:
        return None
    try:
        proof = _proof_from_result(command, result)
    except (KeyError, OSError, RuntimeError, TypeError, ValueError):
        return None
    if proof is None or result.get("runnerProof") != proof:
        return None
    return proof


def _archived_passing_test_count(
    command: Mapping[str, Any], result: Mapping[str, Any]
) -> int | None:
    """Validate captured machine success without consulting today's filesystem."""

    report = result.get("machineReport")
    if not isinstance(report, dict):
        return None
    adapter = command.get("adapter")
    if adapter == "pytest-json@1":
        if (
            report.get("schemaVersion") != SCHEMA_VERSION
            or report.get("adapter") != adapter
            or report.get("selector") != command.get("selector")
            or report.get("pytestExitCode") != 0
            or report.get("runnerError") is not None
            or report.get("deselectedNodeIds") != []
            or report.get("collectionErrors") != []
        ):
            return None
        collected = report.get("collectedNodeIds")
        tests = report.get("tests")
        if (
            not isinstance(collected, list)
            or not collected
            or not all(isinstance(item, str) and item for item in collected)
            or len(set(collected)) != len(collected)
            or not isinstance(tests, list)
        ):
            return None
        tests_by_id = {
            item.get("nodeId"): item
            for item in tests
            if isinstance(item, dict) and isinstance(item.get("nodeId"), str)
        }
        if len(tests_by_id) != len(tests) or set(tests_by_id) != set(collected):
            return None
        for node_id in collected:
            phases = tests_by_id[node_id].get("phases")
            if not isinstance(phases, dict) or set(phases) != {
                "setup",
                "call",
                "teardown",
            }:
                return None
            if any(
                not isinstance(phases.get(name), dict)
                or phases[name] != {"outcome": "passed"}
                for name in ("setup", "call", "teardown")
            ):
                return None
        return len(collected)
    if adapter == "vitest-json@1":
        required_zero = (
            "numFailedTestSuites",
            "numPendingTestSuites",
            "numFailedTests",
            "numPendingTests",
            "numTodoTests",
        )
        if (
            report.get("success") is not True
            or any(_integer_field(report.get(field)) != 0 for field in required_zero)
            or _integer_field(report.get("numRuntimeErrorTestSuites", 0)) != 0
        ):
            return None
        total_tests = _integer_field(report.get("numTotalTests"))
        total_suites = _integer_field(report.get("numTotalTestSuites"))
        if (
            total_tests is None
            or total_tests <= 0
            or _integer_field(report.get("numPassedTests")) != total_tests
            or total_suites is None
            or total_suites <= 0
            or _integer_field(report.get("numPassedTestSuites")) != total_suites
        ):
            return None
        suites = report.get("testResults")
        if not isinstance(suites, list) or not suites:
            return None
        assertion_count = 0
        for suite in suites:
            assertions = (
                suite.get("assertionResults") if isinstance(suite, dict) else None
            )
            if (
                not isinstance(suite, dict)
                or suite.get("status") != "passed"
                or not isinstance(assertions, list)
                or not assertions
            ):
                return None
            if any(
                not isinstance(assertion, dict)
                or assertion.get("status") != "passed"
                for assertion in assertions
            ):
                return None
            assertion_count += len(assertions)
        return total_tests if assertion_count == total_tests else None
    return None


def verify_archived_scenario_result(
    command: Any, result: Any
) -> dict[str, Any] | None:
    """Revalidate immutable runner evidence without coupling it to current files.

    The execution registry separately verifies the persisted report's hash and
    frozen authority identity. New executions still use ``verify_scenario_result``
    before evidence is written, so current runners remain strictly checked once.
    """

    if (
        not isinstance(command, dict)
        or validate_scenario_command(command)
        or not isinstance(result, dict)
        or result.get("schemaVersion") != SCHEMA_VERSION
        or result.get("declaredCommand") != command
        or result.get("validationErrors") != []
        or result.get("passed") is not True
    ):
        return None
    metadata = result.get("runnerMetadata")
    report = result.get("machineReport")
    process = result.get("process")
    proof = result.get("runnerProof")
    if not all(isinstance(item, dict) for item in (metadata, report, process, proof)):
        return None
    assert isinstance(metadata, dict) and isinstance(report, dict)
    assert isinstance(process, dict) and isinstance(proof, dict)
    if (
        metadata.get("adapter") != command.get("adapter")
        or metadata.get("declaredCwd") != command.get("cwd")
        or metadata.get("selector") != command.get("selector")
        or process.get("actualExitCode") != 0
        or process.get("timedOut") is not False
    ):
        return None
    passing_tests = _archived_passing_test_count(command, result)
    isolation = metadata.get("isolation")
    if passing_tests is None or not isinstance(isolation, dict):
        return None
    isolation_proof = {
        "adapter": isolation.get("adapter"),
        "policySha256": isolation.get("policySha256"),
        "executableSha256": isolation.get("executableSha256"),
    }
    expected = {
        "schemaVersion": SCHEMA_VERSION,
        "adapter": command["adapter"],
        "selector": command["selector"],
        "cwd": command["cwd"],
        "executedPassingTests": passing_tests,
        "actualExitCode": 0,
        "runnerMetadataSha256": _sha256_bytes(_canonical_bytes(metadata)),
        "machineReportSha256": _sha256_bytes(_canonical_bytes(report)),
        "isolation": isolation_proof,
    }
    return expected if proof == expected else None


def _closed_failure(
    command: Any,
    errors: list[str],
    *,
    runner_metadata: dict[str, Any] | None = None,
    machine_report: dict[str, Any] | None = None,
    process: dict[str, Any] | None = None,
) -> dict[str, Any]:
    declared = copy.deepcopy(command) if isinstance(command, dict) else command
    adapter = command.get("adapter") if isinstance(command, dict) else None
    selector = command.get("selector") if isinstance(command, dict) else None
    return {
        "schemaVersion": SCHEMA_VERSION,
        "declaredCommand": declared,
        "runnerMetadata": runner_metadata,
        "machineReport": machine_report
        or {
            "schemaVersion": SCHEMA_VERSION,
            "adapter": adapter,
            "selector": selector,
            "runnerError": "; ".join(errors),
        },
        "process": process,
        "validationErrors": errors,
        "runnerProof": None,
        "passed": False,
    }


def execute_scenario_command(
    root: Path | str,
    command: Any,
    env: Mapping[str, str] | None,
) -> dict[str, Any]:
    """Execute one closed scenario declaration and return machine evidence.

    Unsupported operating systems, missing isolation, malformed runners,
    missing reports and test failures all produce a closed ``passed: false``
    result.  There is intentionally no direct-command compatibility path.
    """

    errors = validate_scenario_command(command)
    if errors:
        return _closed_failure(command, errors)
    assert isinstance(command, dict)
    try:
        project_root, cwd, selector_path = _resolve_execution_paths(root, command)
        declared_execution_root = project_root / EXECUTION_TEMP_ROOT
        declared_execution_root.mkdir(parents=True, exist_ok=True)
        execution_root = declared_execution_root.resolve(strict=True)
        if not _is_within(execution_root, project_root):
            raise ValueError("governance execution temp resolves outside the project")
        with tempfile.TemporaryDirectory(prefix="scenario-", dir=execution_root) as temporary:
            writable_root = Path(temporary).resolve()
            environment = _process_environment(env, writable_root)
            isolation = _isolation_metadata(project_root, writable_root)
            if command["adapter"] == "pytest-json@1":
                runner_metadata, machine_report, process = _pytest_runner(
                    project_root,
                    cwd,
                    selector_path,
                    command,
                    writable_root,
                    environment,
                    isolation,
                )
            elif command["adapter"] == "vitest-json@1":
                runner_metadata, machine_report, process = _vitest_runner(
                    project_root,
                    cwd,
                    selector_path,
                    command,
                    writable_root,
                    environment,
                    isolation,
                )
            else:  # validate_scenario_command closes this branch
                raise ValueError("unsupported scenario adapter")
            result = {
                "schemaVersion": SCHEMA_VERSION,
                "declaredCommand": copy.deepcopy(command),
                "runnerMetadata": runner_metadata,
                "machineReport": machine_report,
                "process": process,
                "validationErrors": [],
                "runnerProof": None,
                "passed": False,
            }
            proof = _proof_from_result(command, result)
            result["runnerProof"] = proof
            result["passed"] = proof is not None
            return result
    except (OSError, RuntimeError, TypeError, ValueError, subprocess.SubprocessError) as exc:
        return _closed_failure(command, [f"{type(exc).__name__}: {exc}"])


def _hidden_cli() -> int:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("mode")
    parser.add_argument("--selector")
    parser.add_argument("--report", type=Path)
    parser.add_argument("--basetemp", type=Path)
    arguments = parser.parse_args()
    if (
        arguments.mode != "__pytest_child"
        or not arguments.selector
        or arguments.report is None
        or arguments.basetemp is None
    ):
        return 2
    return _pytest_child_main(arguments.selector, arguments.report, arguments.basetemp)


if __name__ == "__main__":
    raise SystemExit(_hidden_cli())
