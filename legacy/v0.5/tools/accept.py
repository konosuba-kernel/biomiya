#!/usr/bin/env python3
"""Acceptance gate; stage passes never imply complete Core delivery."""

import argparse
import hashlib
import json
import pathlib
import re
import subprocess
import sys
import tempfile
from conformance import (
    invoke,
    validate_cases,
    validate_result_report,
    validate_elf_image,
)

ROOT = pathlib.Path(__file__).resolve().parents[1]
REQUIRED_GROUPS = {
    "modules",
    "c-abi",
    "call-snapshots",
    "raw-storage",
    "external-data-relocations",
    "atomics",
    "mmio",
    "traps",
    "link-runtime",
}


def command_result(command, timeout=300):
    print("Running: " + " ".join(map(str, command)), flush=True)
    try:
        p = subprocess.run(
            command, cwd=ROOT, capture_output=True, text=True, timeout=timeout
        )
        return {
            "passed": p.returncode == 0,
            "exit": p.returncode,
            "command": command,
            "stdout": p.stdout,
            "stderr": p.stderr,
        }
    except (OSError, subprocess.TimeoutExpired, UnicodeError) as e:
        return {"passed": False, "command": command, "error": str(e)}


def attach_json(result, path=None):
    try:
        data = json.loads(
            path.read_text() if path is not None else result.get("stdout", "")
        )
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        result.update(
            passed=False, error="invalid/missing JSON evidence: " + str(error)
        )
        return None
    result["evidence"] = data
    return data


def case_evidence(result, path, manifest):
    data = attach_json(result, path)
    try:
        cases = json.loads(manifest.read_text())["cases"]
        errors = validate_result_report(data, cases)
    except (
        OSError,
        UnicodeError,
        json.JSONDecodeError,
        KeyError,
        TypeError,
        ValueError,
    ) as error:
        errors = [str(error)]
    result["validation_errors"] = errors
    result["passed"] = result.get("passed") is True and not errors
    return result


def validate_integration_report(data):
    from test_integration import GROUPS

    errors = []
    if (
        not isinstance(data, dict)
        or data.get("schema") != 1
        or not isinstance(data.get("groups"), list)
    ):
        return ["invalid integration report shape"]
    expected = {name: {case["id"] for case in cases} for name, cases in GROUPS.items()}
    groups = {}
    seen = set()
    for group in data["groups"]:
        if not isinstance(group, dict) or not isinstance(group.get("id"), str):
            errors.append("invalid integration group")
            continue
        name = group["id"]
        if name in seen:
            errors.append("duplicate integration group: " + name)
        seen.add(name)
        groups[name] = group
        if group.get("status") != "passed":
            errors.append("failed/incomplete integration group: " + name)
        results = group.get("cases")
        if not isinstance(results, list) or not results:
            errors.append("missing integration cases: " + name)
            continue
        ids = []
        for case in results:
            if not isinstance(case, dict) or not isinstance(case.get("id"), str):
                errors.append("invalid integration case")
                continue
            ids.append(case["id"])
            if case.get("passed") is not True or case.get("outcome") in (
                "unsupported",
                "incomplete",
                "timeout",
                "infrastructure-failure",
                "skipped",
            ):
                errors.append("nonpassing integration case: " + case["id"])
        if len(ids) != len(set(ids)) or set(ids) != expected.get(name, set()):
            errors.append("integration case ID set mismatch: " + name)
    if seen != REQUIRED_GROUPS or seen != set(expected):
        errors.append("integration group ID set mismatch")
    return errors


def validate_platform_report(data):
    expected = {p.stem for p in (ROOT / "tests/runtime").glob("*.c")}
    if not isinstance(data, list) or not data:
        return ["invalid/empty platform report"]
    errors = []
    ids = []
    for result in data:
        if not isinstance(result, dict) or not isinstance(result.get("id"), str):
            errors.append("invalid platform record")
            continue
        ids.append(result["id"])
        if result.get("ok") is not True:
            errors.append("failed/skipped/nonboolean platform result: " + result["id"])
    if len(ids) != len(set(ids)) or set(ids) != expected:
        errors.append("platform case ID set mismatch")
    return errors


def validate_toolchain_report(data):
    expected = {"host", "python", "rustc", "clang", "ld.lld", "qemu-system-riscv64"}
    if (
        not isinstance(data, dict)
        or data.get("schema") != 1
        or data.get("reference_environment") is not True
    ):
        return ["invalid or failing toolchain report"]
    records = data.get("checks")
    if not isinstance(records, list) or any(not isinstance(c, dict) for c in records):
        return ["missing toolchain records"]
    ids = [c.get("tool") for c in records]
    if (
        len(ids) != len(expected)
        or set(ids) != expected
        or any(c.get("passed") is not True for c in records)
    ):
        return ["incomplete or nonpassing toolchain records"]
    return []


def validate_clean_report(data):
    expected = {
        "snapshot-consistency",
        "locked-toolchain",
        "compiler-build",
        "rust-tests",
        "gate-regressions",
        "Core-conformance",
        "additional-Core-audit",
        "supplemental-integration",
        "independent-platform",
        "matching-image",
        "hello-execution",
        "source-stability",
    }
    if (
        not isinstance(data, dict)
        or data.get("schema") != 1
        or data.get("passed") is not True
    ):
        return ["invalid or failing clean reproduction report"]
    checks = data.get("checks")
    if (
        not isinstance(checks, dict)
        or set(checks) != expected
        or any(
            not isinstance(c, dict) or c.get("passed") is not True
            for c in checks.values()
        )
    ):
        return ["incomplete or nonpassing clean reproduction checks"]
    original, copied = data.get("source_sha256"), data.get("copied_source_sha256")
    if not isinstance(original, dict) or not original or original != copied:
        return ["clean reproduction source inventory mismatch"]
    return []


def protect_report_path(path):
    """Reports must not overwrite or add files in compiler source inputs."""
    if path is None:
        return
    from reproduce import INPUTS

    resolved = path.resolve()
    for name in INPUTS:
        source = (ROOT / name).resolve()
        if resolved == source or (source.is_dir() and resolved.is_relative_to(source)):
            raise ValueError("report output overlaps source input: " + str(path))


def reproducibility(compiler, d):
    source = ROOT / "tests/conformance/run_integer_context.vt"
    reports = []
    images = []
    for name in ["first", "second"]:
        output = d / (name + ".elf")
        cmd = [
            str(compiler),
            "build",
            "--target",
            "rv64-core-v1",
            "--platform",
            "qemu-virt-core-v1",
            "--emit",
            "elf",
            "--test",
            "test",
            "--diagnostics",
            "json",
            "-o",
            str(output),
            str(source),
        ]
        r = invoke(cmd)
        reports.append(r)
        if (
            r["status"] == "ok"
            and output.is_file()
            and validate_elf_image(output.read_bytes())
        ):
            images.append(hashlib.sha256(output.read_bytes()).hexdigest())
    return {
        "passed": len(images) == 2 and images[0] == images[1],
        "sha256": images,
        "builds": reports,
        "scope": "same locked toolchain; one representative ELF built twice",
    }


REQUIRED_AREAS = {f"C{i:02d}" for i in range(1, 12)} | {"X01"}
REQUIRED_CLAUSES = (
    {f"reference:{i}" for i in range(1, 29)}
    | {f"reference:{c}" for c in "ABCDEFGHI"}
    | {f"rv64:R{i}" for i in range(1, 10)}
    | {"extend:interface"}
)


def _validate_coverage(path, checks):
    errors = []
    try:
        data = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as e:
        return {
            "passed": False,
            "status": "incomplete",
            "path": str(path),
            "errors": [str(e)],
        }
    if (
        data.get("schema") != 1
        or data.get("reference_version") != "0.5"
        or data.get("target") != "rv64-core-v1"
    ):
        errors.append("coverage schema/reference/target mismatch")
    review = data.get("review", {})
    if (
        review.get("status") != "reviewed"
        or not review.get("reviewed_by")
        or (not review.get("summary"))
    ):
        errors.append("independent coverage review is not recorded")
    if data.get("known_gaps") != []:
        errors.append("known gaps are present or not explicitly enumerated")
    areas = data.get("areas", [])
    seen = set()
    clauses = set()
    for area in areas:
        name = area.get("id")
        context = str(name)
        if name in seen:
            errors.append(f"{context}: duplicate coverage area")
        seen.add(name)
        clauses.update(area.get("clauses", []))
        if area.get("status") != "complete":
            errors.append(f"{context}: implementation coverage is incomplete")
        implementations = area.get("implementation", [])
        if not implementations or any(
            (
                not (ROOT / p).is_file()
                or not (ROOT / p).resolve().is_relative_to(ROOT)
                for p in implementations
            )
        ):
            errors.append(f"{context}: implementation paths are missing")
        evidence = area.get("evidence", [])
        if not evidence:
            errors.append(f"{context}: no passing evidence references")
        for reference in evidence:
            check_name = reference.get("check")
            check = checks.get(check_name)
            if not isinstance(check, dict) or check.get("passed") is not True:
                errors.append(
                    f"{context}: referenced check {check_name} is missing or failed"
                )
                continue
            tests = reference.get("tests", [])
            for test in tests:
                if not re.search(
                    "\\btest " + re.escape(test) + "\\s+\\.\\.\\. ok\\b",
                    check.get("stdout", ""),
                ):
                    errors.append(f"{context}: Rust test {test} has no passing output")
            if tests and check_name != "rust-tests":
                errors.append(f"{context}: Rust test reference used outside rust-tests")
            cases = reference.get("cases", [])
            group = reference.get("group")
            if cases or group:
                details = check.get("evidence")
                if details is None:
                    try:
                        details = json.loads(check.get("stdout", ""))
                    except json.JSONDecodeError:
                        details = None
                if isinstance(details, list):
                    results = details
                elif isinstance(details, dict) and "groups" in details:
                    groups = {g["id"]: g for g in details["groups"]}
                    if group:
                        g = groups.get(group)
                        if not g or g.get("status") != "passed":
                            errors.append(
                                f"{context}: integration group {group} is missing or failed"
                            )
                            results = []
                        else:
                            results = g.get("cases", [])
                    else:
                        results = [
                            c for g in groups.values() for c in g.get("cases", [])
                        ]
                elif isinstance(details, dict):
                    results = details.get("results", [])
                else:
                    results = []
                by_id = {r.get("id"): r for r in results}
                for case in cases:
                    item = by_id.get(case)
                    if (
                        not item
                        or item.get("passed", item.get("ok", False)) is not True
                    ):
                        errors.append(
                            f"{context}: case {case} has no actual passing result"
                        )
    if seen != REQUIRED_AREAS:
        errors.append(
            "missing/unknown coverage areas: "
            + str(sorted(REQUIRED_AREAS - seen))
            + " / "
            + str(sorted((str(a) for a in seen - REQUIRED_AREAS)))
        )
    if not REQUIRED_CLAUSES <= clauses:
        errors.append(
            "unmapped normative clauses: "
            + ", ".join(sorted(REQUIRED_CLAUSES - clauses))
        )
    return {
        "passed": not errors,
        "status": "reviewed" if not errors else "incomplete",
        "path": str(path),
        "errors": errors,
        "evidence": data,
    }


def validate_coverage(path, checks):
    try:
        return _validate_coverage(path, checks)
    except (AttributeError, TypeError, ValueError, KeyError, OSError) as error:
        return {
            "passed": False,
            "status": "incomplete",
            "path": str(path),
            "errors": ["malformed coverage evidence: " + str(error)],
        }


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--stage", choices=["full", "core", "platform"], default="full")
    p.add_argument("--compiler", default=str(ROOT / "target/debug/biomiya"))
    p.add_argument("--json", type=pathlib.Path)
    p.add_argument(
        "--coverage", type=pathlib.Path, default=ROOT / "docs/core-coverage.json"
    )
    args = p.parse_args()
    protect_report_path(args.json)
    compiler = pathlib.Path(args.compiler).resolve()
    checks = {}
    from reproduce import source_inventory

    before = source_inventory(ROOT)
    versions = {}
    for name, cmd in [
        ("rust", ["rustc", "--version"]),
        ("clang", ["clang", "--version"]),
        ("lld", ["ld.lld", "--version"]),
        ("qemu", ["qemu-system-riscv64", "--version"]),
    ]:
        versions[name] = command_result(cmd)
    doctor = ROOT / "tools/doctor.py"
    checks["locked-toolchain"] = (
        command_result([sys.executable, str(doctor), "--json"])
        if doctor.is_file()
        else {"passed": False, "error": "missing locked toolchain validator"}
    )
    toolchain = checks["locked-toolchain"]
    toolchain_errors = validate_toolchain_report(attach_json(toolchain))
    toolchain["validation_errors"] = toolchain_errors
    toolchain["passed"] = toolchain["passed"] is True and not toolchain_errors
    checks["gate-regressions"] = command_result(
        [sys.executable, str(ROOT / "tools/test_harness.py")]
    )
    with tempfile.TemporaryDirectory(prefix="biomiya-accept-") as tmp:
        d = pathlib.Path(tmp)
        if args.stage in ("full", "core"):
            checks["rust-tests"] = command_result(
                ["cargo", "test", "--locked", "--all-targets"]
            )
            checks["compiler-build"] = command_result(["cargo", "build", "--locked"])
            report_path = d / "conformance.json"
            checks["Core-conformance"] = command_result(
                [
                    sys.executable,
                    str(ROOT / "tools/conformance.py"),
                    "--compiler",
                    str(compiler),
                    "--json",
                    str(report_path),
                ]
            )
            case_evidence(
                checks["Core-conformance"],
                report_path,
                ROOT / "tests/conformance/manifest.json",
            )
            audit_runner = ROOT / "tools/test_audit.py"
            audit_path = d / "audit.json"
            if audit_runner.is_file():
                audit = command_result(
                    [
                        sys.executable,
                        str(audit_runner),
                        "--compiler",
                        str(compiler),
                        "--json",
                        str(audit_path),
                    ]
                )
                case_evidence(audit, audit_path, ROOT / "tests/audit/manifest.json")
                checks["additional-Core-audit"] = audit
            else:
                checks["additional-Core-audit"] = {
                    "passed": False,
                    "status": "incomplete",
                    "error": "missing required source audit runner",
                }
            checks["reproducibility"] = reproducibility(compiler, d)
        if args.stage in ("full", "platform"):
            checks["independent-platform"] = command_result(
                [sys.executable, str(ROOT / "tools/test_runtime.py"), "--json"]
            )
            platform = checks["independent-platform"]
            data = attach_json(platform)
            errors = validate_platform_report(data)
            platform["validation_errors"] = errors
            platform["passed"] = platform["passed"] is True and not errors
        if args.stage == "full":
            runner = ROOT / "tools/test_integration.py"
            evidence = d / "integration.json"
            if not runner.is_file():
                checks["supplemental-integration"] = {
                    "passed": False,
                    "status": "incomplete",
                    "missing": sorted(REQUIRED_GROUPS),
                    "error": "supplemental integration runner not implemented",
                }
            else:
                result = command_result(
                    [
                        sys.executable,
                        str(runner),
                        "--compiler",
                        str(compiler),
                        "--json",
                        str(evidence),
                    ]
                )
                data = attach_json(result, evidence)
                errors = validate_integration_report(data)
                result["validation_errors"] = errors
                result["passed"] = result["passed"] is True and not errors
                checks["supplemental-integration"] = result
            reproduce = ROOT / "tools/reproduce.py"
            repro_path = d / "clean-reproduction.json"
            if reproduce.is_file():
                reproduction = command_result(
                    [
                        sys.executable,
                        str(reproduce),
                        "--compiler",
                        str(compiler),
                        "--json",
                        str(repro_path),
                    ],
                    timeout=600,
                )
                details = attach_json(reproduction, repro_path)
                errors = validate_clean_report(details)
                reproduction["validation_errors"] = errors
                reproduction["passed"] = reproduction["passed"] is True and not errors
                checks["clean-reproduction"] = reproduction
            else:
                checks["clean-reproduction"] = {
                    "passed": False,
                    "status": "incomplete",
                    "error": "missing clean reproduction runner",
                }
    after = source_inventory(ROOT)
    changed = sorted(
        name
        for name in before.keys() | after.keys()
        if before.get(name) != after.get(name)
    )
    checks["source-stability"] = {
        "passed": not changed,
        "changed_during_run": changed,
        "source_sha256": before,
    }
    test_passed = all((c["passed"] for c in checks.values()))
    coverage = (
        validate_coverage(args.coverage, checks) if args.stage == "full" else None
    )
    if coverage is not None:
        checks["reviewed-coverage"] = coverage
    complete = test_passed and args.stage == "full" and coverage["passed"]
    passed = complete if args.stage == "full" else test_passed
    report = {
        "schema": 1,
        "stage": args.stage,
        "complete_Core_delivery": complete,
        "full_suite_passed": test_passed and args.stage == "full",
        "test_gate_passed": test_passed,
        "stage_passed": passed,
        "extension_verification": "NotProvided",
        "tools": versions,
        "checks": checks,
    }
    for name, c in checks.items():
        print(f"{name}: {('PASS' if c['passed'] else 'FAIL')}")
    print(
        "Full Core delivery acceptance passed"
        if complete
        else "Full test suite passed; reviewed coverage is incomplete"
        if report["full_suite_passed"]
        else f"{args.stage} stage {('passed' if passed else 'failed')}; complete Core delivery is not established"
    )
    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps(report, indent=2) + "\n")
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
