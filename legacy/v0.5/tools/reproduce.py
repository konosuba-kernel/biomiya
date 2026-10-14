#!/usr/bin/env python3
"""Build and test a clean source copy without reusing repository build outputs."""

import argparse
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

from conformance import compiler_report, validate_elf_image
from accept import (
    attach_json,
    case_evidence,
    validate_integration_report,
    validate_platform_report,
    validate_toolchain_report,
    protect_report_path,
)

ROOT = pathlib.Path(__file__).resolve().parents[1]
INPUTS = [
    "README.md",
    ".gitignore",
    "Cargo.toml",
    "Cargo.lock",
    "toolchain-lock.json",
    "src",
    "runtime",
    "platforms",
    "tests",
    "tools",
    "vendor",
    "examples",
    "docs",
]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_inventory(root):
    result = {}
    for name in INPUTS:
        source = root / name
        paths = source.rglob("*") if source.is_dir() else [source]
        for path in paths:
            if (
                path.is_file()
                and "__pycache__" not in path.parts
                and path.suffix != ".pyc"
            ):
                result[str(path.relative_to(root))] = digest(path)
    return dict(sorted(result.items()))


def execute(command, cwd, environment, timeout=600):
    try:
        result = subprocess.run(
            command,
            cwd=cwd,
            env=environment,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        return {
            "passed": result.returncode == 0,
            "command": command,
            "exit": result.returncode,
            "stdout": result.stdout,
            "stderr": result.stderr,
        }
    except (OSError, subprocess.TimeoutExpired, UnicodeError) as error:
        return {"passed": False, "command": command, "error": str(error)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", type=pathlib.Path)
    parser.add_argument(
        "--compiler", type=pathlib.Path, default=ROOT / "target/debug/biomiya"
    )
    args = parser.parse_args()
    protect_report_path(args.json)
    checks = {}
    before = source_inventory(ROOT)
    inventory = {}
    with tempfile.TemporaryDirectory(prefix="biomiya-clean-") as temporary:
        copy = pathlib.Path(temporary) / "source"
        copy.mkdir()
        for name in INPUTS:
            source, destination = ROOT / name, copy / name
            if source.is_dir():
                shutil.copytree(
                    source,
                    destination,
                    ignore=shutil.ignore_patterns("__pycache__", "*.pyc"),
                )
            else:
                shutil.copy2(source, destination)
        inventory = source_inventory(copy)
        after_copy = source_inventory(ROOT)
        changed = sorted(
            name
            for name in before.keys() | inventory.keys() | after_copy.keys()
            if before.get(name) != inventory.get(name)
            or before.get(name) != after_copy.get(name)
        )
        checks["snapshot-consistency"] = {
            "passed": not changed,
            "changed_during_copy": changed,
        }

        environment = os.environ.copy()
        environment["CARGO_TARGET_DIR"] = str(copy / "target")
        environment["BIOMIYA_SYSROOT"] = str(copy)
        # Dependency downloads may come from the normal Cargo cache. The compiler,
        # target objects, ELF files and all test results are built from this copy.
        stages = [
            ("locked-toolchain", [sys.executable, "tools/doctor.py", "--json"]),
            ("compiler-build", ["cargo", "build", "--offline", "--locked"]),
            ("rust-tests", ["cargo", "test", "--offline", "--locked", "--all-targets"]),
            ("gate-regressions", [sys.executable, "tools/test_harness.py"]),
            (
                "Core-conformance",
                [
                    sys.executable,
                    "tools/conformance.py",
                    "--json",
                    "build/conformance.json",
                ],
            ),
            (
                "additional-Core-audit",
                [sys.executable, "tools/test_audit.py", "--json", "build/audit.json"],
            ),
            (
                "supplemental-integration",
                [
                    sys.executable,
                    "tools/test_integration.py",
                    "--json",
                    "build/integration.json",
                ],
            ),
            (
                "independent-platform",
                [sys.executable, "tools/test_runtime.py", "--json"],
            ),
        ]
        for name, command in stages:
            if not checks["snapshot-consistency"]["passed"]:
                break
            print(f"clean copy: {name}", flush=True)
            checks[name] = execute(command, copy, environment)
            result = checks[name]
            if name == "locked-toolchain":
                data = attach_json(result)
                errors = validate_toolchain_report(data)
                result["validation_errors"] = errors
                result["passed"] = result["passed"] is True and not errors
            elif name in ("Core-conformance", "additional-Core-audit"):
                suite = "conformance" if name == "Core-conformance" else "audit"
                case_evidence(
                    result,
                    copy / f"build/{suite}.json",
                    copy / f"tests/{suite}/manifest.json",
                )
            elif name == "supplemental-integration":
                data = attach_json(result, copy / "build/integration.json")
                errors = validate_integration_report(data)
                result["validation_errors"] = errors
                result["passed"] = result["passed"] is True and not errors
            elif name == "independent-platform":
                data = attach_json(result)
                errors = validate_platform_report(data)
                result["validation_errors"] = errors
                result["passed"] = result["passed"] is True and not errors

            print(
                f"clean copy: {name} {'PASS' if checks[name]['passed'] else 'FAIL'}",
                flush=True,
            )
            if not checks[name]["passed"]:
                break
        if len(checks) == len(stages) + 1 and all(
            c["passed"] is True for c in checks.values()
        ):
            images, builds = [], []
            for compiler, root, tag in [
                (args.compiler.resolve(), ROOT, "original"),
                (copy / "target/debug/biomiya", copy, "clean"),
            ]:
                output = pathlib.Path(temporary) / (tag + ".elf")
                command = [
                    str(compiler),
                    "build",
                    "--platform",
                    "qemu-virt-core-v1",
                    "--sysroot",
                    str(root),
                    "--diagnostics",
                    "json",
                    "-o",
                    str(output),
                    str(root / "examples/hello.vt"),
                ]
                result = execute(command, root, environment)
                protocol = compiler_report(
                    result.get("stdout", ""), result.get("exit", -1)
                )
                result["compiler_protocol"] = protocol
                result["passed"] = (
                    result["passed"] is True and protocol["status"] == "ok"
                )
                builds.append(result)
                if (
                    result["passed"]
                    and output.is_file()
                    and validate_elf_image(output.read_bytes())
                ):
                    images.append(digest(output))
            checks["matching-image"] = {
                "passed": len(images) == 2 and images[0] == images[1],
                "sha256": images,
                "builds": builds,
                "scope": "hello ELF across original and isolated compiler builds",
            }
            if checks["matching-image"]["passed"]:
                sys.path.insert(0, str(ROOT / "tools"))
                from test_runtime import run

                execution = run(pathlib.Path(temporary) / "clean.elf")
                checks["hello-execution"] = {
                    "passed": execution["status"] == "pass"
                    and execution.get("value") == 0
                    and "biomiya Core\n" in execution["output"],
                    "execution": execution,
                }
        current = source_inventory(ROOT)
        changed = [
            name
            for name in sorted(before.keys() | current.keys())
            if before.get(name) != current.get(name)
        ]
        checks["source-stability"] = {
            "passed": not changed,
            "changed_during_run": changed,
        }
    passed = len(checks) == len(stages) + 4 and all(
        c["passed"] is True for c in checks.values()
    )
    report = {
        "schema": 1,
        "passed": passed,
        "checks": checks,
        "source_sha256": before,
        "copied_source_sha256": inventory,
        "scope": "isolated source and build directories; existing installed tools and Cargo download cache",
        "excluded": [".git", "target", "build", "Python bytecode"],
    }
    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Clean reproduction: {'PASS' if passed else 'FAIL'}", flush=True)
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
