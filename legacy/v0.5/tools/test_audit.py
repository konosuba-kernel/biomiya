#!/usr/bin/env python3
"""Additional source-level coverage derived from normative Core clauses."""
import argparse
import json
import pathlib
import tempfile
from conformance import execute_case, ROOT


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=pathlib.Path, default=ROOT / "target/debug/biomiya")
    parser.add_argument("--filter", default="")
    parser.add_argument("--json", type=pathlib.Path)
    args = parser.parse_args()
    manifest = json.loads((ROOT / "tests/audit/manifest.json").read_text())
    cases = [case for case in manifest["cases"] if args.filter in case["id"]]
    results = []
    with tempfile.TemporaryDirectory(prefix="biomiya-audit-") as temporary:
        for case in cases:
            result = execute_case(case, args.compiler.resolve(), pathlib.Path(temporary) / (case["id"] + ".elf"))
            results.append(result)
            diagnostics = result["compilation"].get("report", {}).get("diagnostics", [])
            detail = "; ".join(f'{d["code"]}: {d["message"]}' for d in diagnostics)
            if "execution" in result:
                detail = str(result["execution"].get("value", result["execution"]["status"]))
            print(f'{case["id"]}: {"PASS" if result["passed"] else "FAIL"} {detail}', flush=True)
    report = {"schema": 1, "selected": len(cases), "passed": sum(r["passed"] for r in results), "results": results}
    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps(report, indent=2) + "\n")
    print(f'Additional Core coverage: {report["passed"]}/{report["selected"]}')
    return 0 if cases and all(r["passed"] for r in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
