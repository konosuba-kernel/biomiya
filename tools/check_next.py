#!/usr/bin/env python3
"""Build the S-next prototype compiler with its frozen B3.1 seed and verify behavior."""

# Assertions are verification checks, including in imported helpers.
if not __debug__:
    raise SystemExit("verification requires assertions; unset PYTHONOPTIMIZE and omit -O/-OO")

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile

from check_reference import extract
from verify import memory


CORE = Path(__file__).resolve().parents[1]
REPO = CORE / "bootstrap/frozen"
NEXT = CORE / "bootstrap/transition"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def invoke(command, cwd, timeout=60, env=None):
    output = subprocess.run(command, cwd=cwd, env={"PATH": ""} if env is None else env,
                            capture_output=True, text=True, timeout=timeout)
    return {"command": list(map(str, command)), "exit": output.returncode,
            "stdout": output.stdout, "stderr": output.stderr}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seed", type=Path, default=CORE / "build/b31-refactored.elf")
    parser.add_argument("--record-dir", type=Path)
    parser.add_argument("--stage", choices=["P1", "P2", "P3"], default="P2")
    args = parser.parse_args()
    qemu = shutil.which("qemu-riscv64")
    assert qemu, "qemu-riscv64 is required"
    seed = args.seed.resolve().read_bytes()
    baseline = json.loads((NEXT / "seed.json").read_text())
    assert sha(seed) == baseline["compiler_sha256"], "wrong B3.1 seed"
    frozen = {}
    for name, digest in baseline["source_sha256"].items():
        data = (REPO / name).read_bytes()
        assert sha(data) == digest, f"frozen input changed: {name}"
        frozen[name] = data
    identity = sha(json.dumps(baseline["source_sha256"], sort_keys=True,
                              separators=(",", ":")).encode())
    assert identity == baseline["source_identity"]
    manifest = (NEXT / "compiler.build").read_text()
    inputs = [line.split()[1] for line in manifest.splitlines()
              if line.startswith(("source ", "assembly "))]
    sources = {name: (CORE / name).read_bytes() for name in inputs}
    tracked = {str(p.relative_to(CORE)): p.read_bytes() for folder in (NEXT, CORE / "tests/language") for p in folder.rglob("*")
               if p.is_file()
               and p.suffix in [".vt", ".s0", ".json", ".build", ".blocking"]}
    tracked.update({"tools/check_next.py": Path(__file__).read_bytes()})
    for obsolete in [b"begin_capture", b"end_capture", b"instantiate", b"struct Fragment", b"max_saved"]:
        assert obsolete not in sources["bootstrap/transition/ir.vt"], obsolete
    assert b"AST_TRY" not in sources["bootstrap/transition/check.vt"]
    record = args.record_dir.resolve() if args.record_dir else Path(tempfile.mkdtemp(prefix="snext-p1-"))
    if args.record_dir:
        record.mkdir(parents=True, exist_ok=False)
    print(f"Evidence: {record}", flush=True)
    evidence = {"stage": args.stage, "self_hosted": False, "seed_sha256": sha(seed),
                "seed_source_identity": identity, "guest_stack_bytes": 1048576,
                "source_sha256": {name: sha(data) for name, data in sources.items()},
                "verification_inputs": {name: sha(data) for name, data in tracked.items()},
                "cases": []}

    def save():
        (record / "verification.json").write_text(json.dumps(evidence, indent=2) + "\n")

    # Reproducible local source capsule; rebuilding B still uses the pinned tools.
    with tarfile.open(record / "seed-source.tar", "w") as archive:
        for name, data in sorted(frozen.items()):
            info = tarfile.TarInfo(name)
            info.size, info.mode, info.mtime = len(data), 0o644, 0
            archive.addfile(info, io.BytesIO(data))
    with tarfile.open(record / "candidate-source.tar", "w") as archive:
        for name, data in sorted({**sources, **tracked}.items()):
            info = tarfile.TarInfo(name)
            info.size, info.mode, info.mtime = len(data), 0o644, 0
            archive.addfile(info, io.BytesIO(data))
    save()
    with tempfile.TemporaryDirectory(prefix="snext-input-") as directory:
        work = Path(directory)
        for name, data in sources.items():
            path = work / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        (work / "seed.elf").write_bytes(seed)
        (work / "seed.elf").chmod(0o755)
        (work / "compiler.build").write_text(re.sub(r"^output .*$", "output compiler.elf", manifest, flags=re.M))
        built = invoke([qemu, "-s", "1048576", "-strace", "seed.elf", "build", "compiler.build"], work, 600)
        (record / "build.log").write_text(built["stderr"])
        assert built["exit"] == 0, built
        assert "execve(" not in built["stderr"]
        opens = re.findall(r'openat\([^\n]*?"([^"\n]+)"', built["stderr"])
        assert set(opens) <= set(inputs + ["compiler.build", "compiler.elf"]), opens
        binary = (work / "compiler.elf").read_bytes()
        compiler = record / "compiler.elf"
        compiler.write_bytes(binary)
        compiler.chmod(0o755)
        evidence.update(compiler_sha256=sha(binary), elf_bytes=len(binary), memory=memory(binary),
                        allowed_build_opens=sorted(set(opens)))
        save()

        def case(name, files, settings):
            folder = work / name
            folder.mkdir()
            for filename, body in files.items():
                (folder / filename).write_text(body)
            build = files.get("program.build")
            if build is None:
                build = f"format 1\nprofile linux-user\nentry {settings.get('entry', 'run')}\n"
                build += "".join(f"source {n}\n" for n in files if n.endswith(".vt"))
                build += "".join(f"assembly {n}\n" for n in files if n.endswith(".s0"))
                build += "output program.elf\n"
                (folder / "program.build").write_text(build)
            result = {"name": name, "expected": settings, "source_sha256":
                      {n: sha(s.encode()) for n, s in files.items()}, "commands": []}
            evidence["cases"].append(result)

            def run(arguments):
                output = invoke([qemu, "-s", "1048576", *arguments], folder)
                result["commands"].append(output)
                save()
                return output

            for mode in ["check", "build"]:
                output = run([str(compiler), mode, "program.build"])
                if "reject" in settings:
                    assert output["exit"] != 0 and re.search(r"\b" + settings["reject"] + r"\b", output["stderr"]), result
                    assert not (folder / "program.elf").exists(), result
                    if "diagnostic" in settings:
                        where = settings["diagnostic"]
                        assert f"file={where['file']} byte={where['byte']}" in output["stderr"], result
                else:
                    assert output["exit"] == 0, result
                    if mode == "check":
                        assert not (folder / "program.elf").exists(), result
            if "exit" in settings:
                output = run(["program.elf"])
                assert output["exit"] == settings["exit"] and output["stdout"] == settings.get("stdout", ""), result
                result["elf_sha256"] = sha((folder / "program.elf").read_bytes())
            if "blocking" in settings:
                contract = settings["blocking"]
                for mode, expected in [("nonblocking", "Verified"), ("unknown", "Unknown"), ("may_block", "Violation")]:
                    policy = f"format 1\nrequire {contract['root']}\n"
                    if mode != "unknown":
                        policy += f"{mode} {contract['external']}\n"
                    (folder / "policy.blocking").write_text(policy)
                    (folder / "analysis.build").write_text(build.replace("output program.elf", f"output {mode}.elf"))
                    output = run([str(compiler), "build", "analysis.build", "--blocking", "policy.blocking"])
                    assert f": {expected}\n" in output["stderr"], result
                    if mode == "nonblocking":
                        assert output["exit"] == 0 and (folder / f"{mode}.elf").read_bytes() == (folder / "program.elf").read_bytes(), result
                    else:
                        assert output["exit"] != 0 and not (folder / f"{mode}.elf").exists(), result
            result["passed"] = True
            save()
            print(f"PASS {name}", flush=True)

        case_path = "tests/language/cases.json" if args.stage == "P1" else "tests/language/" + args.stage.lower() + "/cases.json"
        cases = json.loads(tracked[case_path])
        names = [case["name"] for case in cases]
        assert len(names) == len(set(names)), "duplicate case identity"
        if args.stage != "P1":
            retained = {c["name"] for c in json.loads(tracked["tests/language/cases.json"])}
            retained |= {"reference_" + c["name"] for c in json.loads(frozen["core-s/docs/reference/b3.1.json"])["cases"]}
            assert retained <= set(names), f"missing migrated cases: {retained - set(names)}"
        failures = []
        for settings in cases:
            if "files" in settings:
                files = {name: tracked["tests/language/" + path].decode()
                         for name, path in settings["files"].items()}
            else:
                files = {"main.vt": tracked["tests/language/" + settings["source"]].decode()}
                if "assembly" in settings:
                    files["trace.s0"] = tracked["tests/language/" + settings["assembly"]].decode()
            try:
                case(settings["name"], files, settings)
            except AssertionError as error:
                failures.append(settings["name"])
                evidence["cases"][-1]["failure"] = str(error)
                save()
                print(f"FAIL {settings['name']}", flush=True)

        if args.stage == "P1":
            reference = json.loads(frozen["core-s/docs/reference/b3.1.json"])
            examples = extract(frozen["core-s/docs/reference/b3.1.md"].decode())
            for settings in reference["cases"]:
                settings = dict(settings)
                if settings["name"] == "assignment_order":
                    settings["exit"] = 21
                if settings["name"] in ["cleanup", "cleanup_snapshot", "defer_bad"]:
                    settings.pop("exit", None)
                    settings["reject"] = "I_UNSUPPORTED"
                case("reference_" + settings["name"], examples[settings["name"]], settings)

        # Run existing target input/machine probes against the NEW VT implementation.
        tree = work / "retained"
        for name, data in frozen.items():
            path = tree / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        for name, data in sources.items():
            if name.startswith("bootstrap/transition/"):
                (tree / "core-s/bootstrap" / Path(name).name).write_bytes(data)
        if args.stage == "P3":
            (tree / "core-s/bootstrap/tests/machine.vt").write_bytes(tracked["tests/language/machine.vt"])
        environment = os.environ.copy()
        for name in ["CARGO_TARGET_DIR", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"]:
            environment.pop(name, None)
        tests = invoke(["cargo", "test", "--offline", "--locked", "--all-features",
                        "--test", "b3_input", "--test", "b3_machine"], tree / "core-s", 600, environment)
        (record / "retained-tests.log").write_text(tests["stdout"] + tests["stderr"])
        assert tests["exit"] == 0, tests
        names = re.findall(r"^test (\S+) \.\.\. ok$", tests["stdout"], re.M)
        assert len(names) == 11 and set(names) <= set(baseline["tests"]), names
        evidence["retained_target_tests"] = names
        # Evidence must describe the still-current candidate and test revision.
        for name, digest in {**evidence["source_sha256"], **evidence["verification_inputs"]}.items():
            assert sha((CORE / name).read_bytes()) == digest, f"candidate changed during verification: {name}"
        evidence["failed"] = failures
        evidence["passed"] = sum(c.get("passed", False) for c in evidence["cases"])
        save()
        assert not failures, f"failed cases: {failures}"
        print(f"All {evidence['passed']} cases and 11 retained target tests passed.\nCompiler: {compiler}")


if __name__ == "__main__":
    main()
