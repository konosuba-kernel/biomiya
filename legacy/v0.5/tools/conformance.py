#!/usr/bin/env python3
"""Run normative Core cases through the real CLI and RV64 ELF execution."""

import argparse
import json
import pathlib
import re
import subprocess
import tempfile
import time
from test_runtime import run as run_target

ROOT = pathlib.Path(__file__).resolve().parents[1]

KINDS = {"accept", "reject", "run", "panic", "extend-accept", "extend-reject"}


def compiler_report(stdout, exit_code):
    """Reject malformed or contradictory compiler protocol, even on exit zero."""
    try:
        report = json.loads(stdout)
    except (json.JSONDecodeError, TypeError):
        return {
            "status": "infrastructure-failure",
            "error": "compiler did not emit valid JSON",
        }
    if not isinstance(report, dict) or report.get("status") not in ("ok", "error"):
        return {
            "status": "infrastructure-failure",
            "error": "invalid compiler result shape",
        }
    diagnostics = report.get("diagnostics")
    if not isinstance(diagnostics, list) or any(
        not isinstance(d, dict) or not isinstance(d.get("code"), str)
        for d in diagnostics
    ):
        return {
            "status": "infrastructure-failure",
            "error": "invalid compiler diagnostics shape",
            "report": report,
        }
    codes = [d["code"] for d in diagnostics]
    if "I_UNSUPPORTED" in codes:
        status = "unsupported"
    elif any(code.startswith("I_") for code in codes) or exit_code not in (0, 1):
        status = "infrastructure-failure"
    elif exit_code == 0 and report["status"] == "ok" and not diagnostics:
        status = "ok"
    elif exit_code == 1 and report["status"] == "error" and diagnostics:
        status = "error"
    else:
        status = "infrastructure-failure"
    return {"status": status, "report": report, "codes": codes, "exit": exit_code}


def invoke(command, timeout=30):
    try:
        p = subprocess.run(
            command, cwd=ROOT, capture_output=True, text=True, timeout=timeout
        )
    except (OSError, subprocess.TimeoutExpired, UnicodeError) as e:
        return {"status": "infrastructure-failure", "error": str(e), "command": command}
    result = compiler_report(p.stdout, p.returncode)
    result.update(command=command, stderr=p.stderr, stdout=p.stdout, exit=p.returncode)
    return result


def validate_elf_image(image):
    """Require an RV64 little-endian LP64 executable before treating a file as ELF."""
    if len(image) < 64 or image[:7] != b"\x7fELF\x02\x01\x01":
        return False
    return (
        int.from_bytes(image[16:18], "little") == 2
        and int.from_bytes(image[18:20], "little") == 243
        and int.from_bytes(image[48:52], "little") == 0
        and int.from_bytes(image[52:54], "little") == 64
    )


def validate_cases(cases):
    if not isinstance(cases, list) or not cases:
        raise ValueError("empty or invalid case manifest")
    seen = set()
    for case in cases:
        if (
            not isinstance(case, dict)
            or not isinstance(case.get("id"), str)
            or not re.fullmatch(r"[A-Za-z0-9_.-]+", case["id"])
        ):
            raise ValueError("invalid case ID")
        if case["id"] in seen:
            raise ValueError("duplicate case ID: " + case["id"])
        seen.add(case["id"])
        if case.get("kind") not in KINDS or not isinstance(case.get("file"), str):
            raise ValueError("invalid case kind/file: " + case["id"])
        if case["kind"] == "run" and (
            type(case.get("result")) is not int
            or not -(1 << 31) <= case["result"] < (1 << 31)
        ):
            raise ValueError("invalid i32 result: " + case["id"])
        if case["kind"] == "reject" and (
            not isinstance(case.get("category"), str)
            or not case["category"].startswith("E_")
        ):
            raise ValueError("invalid rejection category: " + case["id"])
    return cases


def validate_result_report(report, cases):
    """Require every expected ID exactly once and recompute summary counts."""
    errors = []
    try:
        validate_cases(cases)
    except ValueError as e:
        return [str(e)]
    if not isinstance(report, dict) or report.get("schema") != 1:
        return ["invalid result report schema"]
    results = report.get("results")
    if not isinstance(results, list):
        return ["missing result records"]
    expected = {case["id"]: case for case in cases}
    seen = set()
    passed = 0
    for result in results:
        if not isinstance(result, dict) or not isinstance(result.get("id"), str):
            errors.append("invalid result record")
            continue
        name = result["id"]
        if name in seen:
            errors.append("duplicate result ID: " + name)
        seen.add(name)
        if name not in expected:
            errors.append("unexpected result ID: " + name)
        elif result.get("kind") != expected[name]["kind"]:
            errors.append("wrong result kind: " + name)
        if result.get("passed") is True:
            passed += 1
        else:
            errors.append("failed/skipped/nonboolean result: " + name)
        if name in expected:
            case = expected[name]
            kind = case["kind"]
            outcome = {
                "reject": "matched-rejection",
                "run": "matched-execution",
                "panic": "matched-execution",
            }.get(kind, "Core-accepted")
            if result.get("outcome") != outcome:
                errors.append("nonpassing or missing outcome: " + name)
            compilation = result.get("compilation")
            if not isinstance(compilation, dict):
                errors.append("missing compiler evidence: " + name)
            else:
                protocol = compiler_report(
                    compilation.get("stdout"), compilation.get("exit")
                )
                if protocol["status"] != ("error" if kind == "reject" else "ok"):
                    errors.append("invalid compiler evidence: " + name)
                if kind == "reject" and case["category"] not in protocol.get(
                    "codes", []
                ):
                    errors.append("wrong rejection evidence: " + name)
            if kind in ("run", "panic"):
                execution = result.get("execution")
                if not isinstance(execution, dict) or execution.get("status") != (
                    "panic" if kind == "panic" else "pass"
                ):
                    errors.append("missing or wrong execution evidence: " + name)
                elif kind == "run" and (
                    type(execution.get("value")) is not int
                    or execution["value"] != case["result"]
                ):
                    errors.append("wrong returned i32 evidence: " + name)
    if seen != set(expected):
        errors.append("result ID set does not match manifest")
    if (
        len(results) != len(cases)
        or type(report.get("selected")) is not int
        or report["selected"] != len(cases)
    ):
        errors.append("selected/result count mismatch")
    if type(report.get("passed")) is not int or report["passed"] != passed:
        errors.append("passed summary mismatch")
    if "failed" in report and (
        type(report["failed"]) is not int or report["failed"] != len(results) - passed
    ):
        errors.append("failed summary mismatch")
    if "manifest_cases" in report and report["manifest_cases"] != len(cases):
        errors.append("manifest summary mismatch")
    return errors


def execute_case(case, compiler, output):
    try:
        validate_cases([case])
    except ValueError as error:
        return {
            "id": case.get("id", "invalid") if isinstance(case, dict) else "invalid",
            "kind": case.get("kind") if isinstance(case, dict) else None,
            "expected": None,
            "compilation": {"status": "infrastructure-failure", "error": str(error)},
            "passed": False,
            "outcome": "infrastructure-failure",
        }
    source = (ROOT / "tests/conformance" / case["file"]).resolve()
    if not source.is_relative_to(ROOT):
        return {
            "id": case["id"],
            "kind": case["kind"],
            "expected": None,
            "compilation": {
                "status": "infrastructure-failure",
                "error": "case source lies outside repository",
            },
            "passed": False,
            "outcome": "infrastructure-failure",
        }
    if case["kind"] in ("run", "panic"):
        try:
            output.unlink(missing_ok=True)
        except OSError as error:
            return {
                "id": case["id"],
                "kind": case["kind"],
                "expected": None,
                "compilation": {
                    "status": "infrastructure-failure",
                    "error": str(error),
                },
                "passed": False,
                "outcome": "infrastructure-failure",
            }
    kind = case["kind"]
    expected = case.get(
        "result", case.get("category", "panic" if kind == "panic" else "accept")
    )
    command = [
        str(compiler),
        "build" if kind in ("run", "panic") else "check",
        "--target",
        "rv64-core-v1",
        "--diagnostics",
        "json",
    ]
    if kind in ("run", "panic"):
        command += [
            "--platform",
            "qemu-virt-core-v1",
            "--emit",
            "elf",
            "--test",
            "test",
            "-o",
            str(output),
        ]
    command.append(str(source))
    start = time.monotonic()
    compilation = invoke(command)
    result = {
        "id": case["id"],
        "kind": kind,
        "expected": expected,
        "compilation": compilation,
        "passed": False,
    }
    if kind.startswith("extend-"):
        result["verification"] = "NotProvided"
        result["assessment"] = (
            "Core acceptance only; real extension verification is unavailable"
        )
    if compilation["status"] in ("unsupported", "infrastructure-failure"):
        result["outcome"] = compilation["status"]
    elif kind == "reject":
        result["passed"] = (
            compilation["status"] == "error"
            and compilation["exit"] == 1
            and (case["category"] in compilation["codes"])
        )
        result["outcome"] = (
            "matched-rejection"
            if result["passed"]
            else "wrong-diagnostic-or-acceptance"
        )
    elif compilation["status"] != "ok":
        result["outcome"] = "unexpected-rejection"
    elif kind in ("run", "panic"):
        if not output.is_file() or not validate_elf_image(output.read_bytes()):
            result["outcome"] = "infrastructure-failure"
            result["error"] = "successful build did not create a valid RV64 ELF"
        else:
            execution = run_target(output)
            result["execution"] = execution
            result["passed"] = (
                execution["status"] == "panic"
                if kind == "panic"
                else execution["status"] == "pass"
                and execution["value"] == case["result"]
            )
            result["outcome"] = (
                "matched-execution" if result["passed"] else execution["status"]
            )
    else:
        result["passed"] = True
        result["outcome"] = "Core-accepted"
    result["elapsed_seconds"] = round(time.monotonic() - start, 3)
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", default=str(ROOT / "target/debug/biomiya"))
    parser.add_argument(
        "--kind",
        action="append",
        choices=["accept", "reject", "run", "panic", "extend-accept", "extend-reject"],
    )
    parser.add_argument("--filter", default="")
    parser.add_argument("--json", type=pathlib.Path)
    args = parser.parse_args(argv)
    manifest = json.loads((ROOT / "tests/conformance/manifest.json").read_text())
    validate_cases(manifest["cases"])
    cases = [
        c
        for c in manifest["cases"]
        if (not args.kind or c["kind"] in args.kind) and args.filter in c["id"]
    ]
    compiler = pathlib.Path(args.compiler).resolve()
    results = []
    with tempfile.TemporaryDirectory(prefix="biomiya-conformance-") as d:
        for case in cases:
            result = execute_case(
                case, compiler, pathlib.Path(d) / (case["id"] + ".elf")
            )
            results.append(result)
            print(
                f"{case['id']}: {('PASS' if result['passed'] else 'FAIL')} {result['outcome']}",
                flush=True,
            )
    report = {
        "schema": 1,
        "target": manifest["target"],
        "selected": len(cases),
        "manifest_cases": len(manifest["cases"]),
        "passed": sum((r["passed"] for r in results)),
        "failed": sum((not r["passed"] for r in results)),
        "extension_verification": "NotProvided",
        "results": results,
    }
    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps(report, indent=2) + "\n")
    print(
        f"Core cases: {report['passed']}/{report['selected']} passed; real extension verification: NotProvided"
    )
    return 0 if cases and all((r["passed"] for r in results)) else 1


if __name__ == "__main__":
    raise SystemExit(main())
