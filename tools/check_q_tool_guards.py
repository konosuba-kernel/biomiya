#!/usr/bin/env python3
"""Check maintained verifier entry points and a real wrong-expectation counterexample."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

CORE = Path(__file__).resolve().parents[1]
TOOLS = CORE / "tools"
GUARDED = (
    "check_next", "check_next_reference", "check_next_trials", "check_q",
    "check_q_blocking", "check_q_copy", "check_q_input", "check_q_interfaces",
    "check_q_limits", "check_q_machine", "selfhost_next",
)
MODES = (("normal", (), None), ("-O", ("-O",), None),
         ("-OO", ("-OO",), None), ("PYTHONOPTIMIZE", (), "1"))
REFUSAL = "verification requires assertions; unset PYTHONOPTIMIZE and omit -O/-OO"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(arguments, mode, cwd):
    name, flags, optimize = mode
    env = dict(os.environ)
    env.pop("PYTHONOPTIMIZE", None)
    if optimize is not None:
        env["PYTHONOPTIMIZE"] = optimize
    output = subprocess.run([sys.executable, *flags, *arguments], cwd=cwd,
                            env=env, capture_output=True, text=True, timeout=60)
    return {"mode": name, "exit": output.returncode,
            "stdout": output.stdout, "stderr": output.stderr}


def check_entry_points():
    results = []
    for name in GUARDED:
        for imported in (False, True):
            arguments = (["-c", f"import {name}"] if imported else
                         [str(TOOLS / (name + ".py")), "--help"])
            for mode in MODES:
                result = run(arguments, mode, TOOLS)
                result.update(tool=name, imported=imported)
                if mode[0] == "normal":
                    require(result["exit"] == 0, result)
                else:
                    require(result["exit"] != 0 and REFUSAL in result["stderr"], result)
                results.append(result)
    return results


def check_corpus(work, compiler):
    core = work / "fixture"
    (core / "tools").mkdir(parents=True)
    for name in ("check_q.py", "check_next.py", "check_reference.py", "verify.py"):
        shutil.copyfile(TOOLS / name, core / "tools" / name)
    for name in ("verify.py", "check_reference.py"):
        source = CORE / "bootstrap/frozen/core-s/tools" / name
        target = core / "bootstrap/frozen/core-s/tools" / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    (core / "compiler").mkdir(parents=True)
    (core / "compiler/compiler.build").write_text("format 1\nprofile linux-user\n")
    for phase in ("p2", "p3"):
        (core / "tests/language" / phase).mkdir(parents=True)
    (core / "tests/language/main.vt").write_text(
        "module sample;\nfn run() -> u32 { return 0; }\n")
    results = []
    # The tool and compiler are real; only the corpus is replaced in a temporary tree.
    for expected in (0, 7):
        spec = [{"name": "exit_expectation", "source": "main.vt",
                 "entry": "sample::run", "exit": expected}]
        for phase in ("p2", "p3"):
            (core / "tests/language" / phase / "cases.json").write_text(json.dumps(spec))
        for mode in (MODES[:1] if expected == 0 else MODES):
            record = core / (str(expected) + "-" + mode[0])
            result = run([str(core / "tools/check_q.py"), "--compiler", str(compiler),
                          "--record-dir", str(record)], mode, core)
            result["expected_exit"] = expected
            if mode[0] != "normal":
                require(result["exit"] != 0 and REFUSAL in result["stderr"], result)
                require(not record.exists(), "optimized verifier created evidence")
                result["record_created"] = False
            else:
                evidence = json.loads((record / "verification.json").read_text())
                result["verification"] = evidence
                commands = evidence["cases"][0]["commands"]
                require(len(commands) == 3 and all(c["exit"] == 0 for c in commands), result)
                require(evidence["passed"] == (1 if expected == 0 else 0), result)
                require((result["exit"] == 0) == (expected == 0), result)
                require(evidence["failed"] == ([] if expected == 0 else ["exit_expectation"]), result)
            results.append(result)
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, required=True)
    args = parser.parse_args()
    compiler = args.compiler.resolve()
    seed = json.loads((CORE / "bootstrap/transition/seed.json").read_text())
    frozen = seed["source_sha256"]
    changed = [name for name, digest in frozen.items() if sha(CORE / "bootstrap/frozen" / name) != digest]
    require(not changed, {"changed_frozen_inputs": changed})
    inputs = {name: sha(TOOLS / name) for name in
              [*(name + ".py" for name in GUARDED), "verify.py", "check_reference.py",
               "check_q_tool_guards.py"]}
    with tempfile.TemporaryDirectory(prefix="verification-modes-") as directory:
        work = Path(directory)
        entries = check_entry_points()
        corpus = check_corpus(work, compiler)
    require(all(sha(TOOLS / name) == digest for name, digest in inputs.items()),
            "tool inputs changed during verification")
    result = {
        "passed": True, "compiler_sha256": sha(compiler), "tool_sha256": inputs,
        "frozen_inputs_unchanged": len(frozen), "entry_points": entries, "corpus": corpus,
        "scope": "Maintained verifier modules, including their import paths. Frozen verify.py "
                 "and check_reference.py are preserved; invoke those legacy tools without optimization.",
    }
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
