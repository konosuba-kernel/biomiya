#!/usr/bin/env python3
"""Descriptive counts only; no complexity score or mechanical style verdict."""
import hashlib
import json
from pathlib import Path
import re
import subprocess


REPO = Path(__file__).resolve().parents[3]
CORE = REPO / "core-s"


def measure(path):
    data = path.read_bytes()
    source = data.decode()
    result = {"path": str(path.relative_to(REPO)), "lines": len(source.splitlines()),
              "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
    if path.suffix != ".vt":
        return result
    # Mask comments and string literals while retaining offsets/newlines.
    masked = re.sub(r'"(?:\\.|[^"\\])*"|//[^\n]*|/\*[\s\S]*?\*/',
                    lambda m: re.sub(r"[^\n]", " ", m[0]), source)
    functions = []
    for match in re.finditer(r"^\s*(?:pub\s+)?(?:unsafe\s+)?fn\s+(\w+)\s*\([^;{]*\)[^{;]*\{", masked, re.M):
        start = masked.index("fn", match.start(), match.end())
        opening = match.end() - 1
        depth = 1
        end = opening
        while depth and end + 1 < len(masked):
            end += 1
            depth += (masked[end] == "{") - (masked[end] == "}")
        first = masked.count("\n", 0, start) + 1
        last = masked.count("\n", 0, end) + 1
        functions.append({"name": match[1], "start": first, "end": last,
                          "physical_span": last - first + 1})
    result["function_count"] = len(functions)
    result["functions_over_60_physical_lines"] = [f for f in functions if f["physical_span"] > 60]
    result["public_mutable_statics"] = [
        {"name": m[1], "line": masked.count("\n", 0, m.start()) + 1}
        for m in re.finditer(r"\bpub\s+static\s+mut\s+(\w+)", masked)]
    return result


def main():
    groups = {
        "q_production": sorted((CORE / "next/q").glob("*.vt")),
        "q_runtime": sorted((CORE / "next/q/runtime").glob("*.vt"))
                     + sorted((CORE / "next/q/runtime").glob("*.s0")),
        "maintained_tools": sorted((CORE / "tools").glob("*.py"))
                            + sorted((CORE / "tools").glob("*.rs")),
    }
    output = {
        "baseline_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
        "standard_sha256": hashlib.sha256((REPO.parent / "suba/CODE_STYLE.md").read_bytes()).hexdigest(),
        "method": "Physical lines including comments/blanks. VT spans use masked strings/comments and matched braces; >60 is a review signal, not a failure. No nesting/complexity rating. Runtime counted separately.",
        "groups": {},
    }
    for name, paths in groups.items():
        files = [measure(path) for path in paths]
        output["groups"][name] = {"file_count": len(files), "lines": sum(f["lines"] for f in files),
                                  "bytes": sum(f["bytes"] for f in files), "files": files}
    print(json.dumps(output, indent=2) + "\n", end="")


if __name__ == "__main__":
    main()
