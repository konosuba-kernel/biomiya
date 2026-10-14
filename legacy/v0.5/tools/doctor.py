#!/usr/bin/env python3
"""Compare explicit host tools with the reproducible reference versions."""
import argparse
import json
import pathlib
import re
import subprocess
import shutil
import sys
import platform

ROOT = pathlib.Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    lock = json.loads((ROOT / "toolchain-lock.json").read_text())
    checks = [{"tool": "host", "expected": lock["host"], "observed": platform.machine()+"-"+platform.system().lower(), "passed": platform.machine() == "x86_64" and platform.system() == "Linux"}, {"tool": "python", "expected": ">=3.9", "observed": platform.python_version(), "passed": sys.version_info >= (3, 9)}]
    for key, command in [("rust", "rustc"), ("clang", "clang"), ("lld", "ld.lld"), ("qemu", "qemu-system-riscv64")]:
        try:
            result = subprocess.run([command, "--version"], capture_output=True, text=True, timeout=10)
            first_line = result.stdout.splitlines()[0] if result.stdout else result.stderr.strip()
            version = re.search(r"\b\d+\.\d+\.\d+\b", first_line)
            observed = version.group(0) if version else None
            checks.append({"tool": command, "expected": lock[key], "observed": observed, "path": shutil.which(command), "version_line": first_line, "passed": result.returncode == 0 and observed == lock[key]})
        except (OSError, subprocess.TimeoutExpired) as error:
            checks.append({"tool": command, "expected": lock[key], "passed": False, "error": str(error)})
    passed = all(check["passed"] for check in checks)
    if args.json:
        print(json.dumps({"schema": 1, "reference_environment": passed, "checks": checks}))
    else:
        for check in checks:
            print(f'{check["tool"]}: {"OK" if check["passed"] else "MISMATCH"} expected {check["expected"]}, found {check.get("observed", check.get("error"))}')
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
