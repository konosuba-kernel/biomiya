#!/usr/bin/env python3
"""Run the unchanged check_q tool on a deliberately wrong expectation, in isolation."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


REPO = Path(__file__).resolve().parents[3]
CORE = REPO / "core-s"
COMPILER = CORE / "build/snext-p4-r2/e3.elf"


def main():
    result = {
        "purpose": "Tool counterexample, not a release suite rerun",
        "compiler_sha256": hashlib.sha256(COMPILER.read_bytes()).hexdigest(),
        "fixture": "module sample;\nfn run() -> u32 { return 0; }\n",
        "expected_exit_deliberately_wrong": 7,
        "tools_sha256": {},
        "runs": [],
    }
    with tempfile.TemporaryDirectory(prefix="biomiya-style-asserts-") as directory:
        core = Path(directory)
        (core / "tools").mkdir()
        # Copy tool sources byte-for-byte; only the test corpus is synthetic.
        for source in (CORE / "tools").glob("*.py"):
            shutil.copyfile(source, core / "tools" / source.name)
            result["tools_sha256"][source.name] = hashlib.sha256(source.read_bytes()).hexdigest()
        (core / "next/q").mkdir(parents=True)
        (core / "next/q/compiler.build").write_text("format 1\nprofile linux-user\n")
        spec = [{"name": "wrong_expected_exit", "source": "main.vt",
                 "entry": "sample::run", "exit": 7}]
        for phase in ["p2", "p3"]:
            path = core / "next/tests" / phase
            path.mkdir(parents=True)
            (path / "cases.json").write_text(json.dumps(spec))
        (core / "next/tests/main.vt").write_text(result["fixture"])
        for mode, flags in [("normal", []), ("optimized", ["-O"])]:
            record = core / mode
            command = [sys.executable, *flags, str(core / "tools/check_q.py"),
                       "--compiler", str(COMPILER), "--record-dir", str(record)]
            output = subprocess.run(command, text=True, capture_output=True, timeout=60)
            verification = json.loads((record / "verification.json").read_text())
            case = verification["cases"][0]
            result["runs"].append({
                "mode": mode, "tool_exit": output.returncode, "stdout": output.stdout,
                "reported_passed": verification["passed"], "failed": verification["failed"],
                "commands": [{"exit": c["exit"], "stdout": c["stdout"], "stderr": c["stderr"]}
                             for c in case["commands"]],
            })
    normal, optimized = result["runs"]
    reproduced = (normal["tool_exit"] != 0 and normal["reported_passed"] == 0
                  and optimized["tool_exit"] == 0 and optimized["reported_passed"] == 1
                  and all(r["commands"][-1]["exit"] == 0 for r in result["runs"]))
    result["false_pass_reproduced"] = reproduced
    print(json.dumps(result, indent=2) + "\n", end="")
    if not reproduced:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
