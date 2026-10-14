#!/usr/bin/env python3
"""Run source-only regression tests and verify the current self-hosting fixed point."""

import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tempfile


CORE = Path(__file__).resolve().parents[1]
REPO = CORE.parent


def run(command, cwd, env, log):
    with log.open("w") as output:
        subprocess.run(command, cwd=cwd, env=env, stdout=output,
                       stderr=subprocess.STDOUT, timeout=900, check=True)
    return log.read_text()


def memory(data):
    offset = struct.unpack_from("<Q", data, 32)[0]
    stride, count = struct.unpack_from("<HH", data, 54)
    segments = [struct.unpack_from("<IIQQQQQQ", data, offset + i * stride)
                for i in range(count)]
    loads = [segment for segment in segments if segment[0] == 1]
    rw = sum(segment[6] for segment in loads if segment[1] & 2)
    span = max(segment[3] + segment[6] for segment in loads) - min(segment[3] for segment in loads)
    assert rw <= 64 * 1024 * 1024 and span <= 64 * 1024 * 1024
    return {"rw_bytes": rw, "image_span_bytes": span}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--record-dir", type=Path, help="new directory for logs and verified compiler")
    args = parser.parse_args()
    if args.record_dir:
        record = args.record_dir.resolve()
        record.mkdir(parents=True, exist_ok=False)
    else:
        record = Path(tempfile.mkdtemp(prefix="core-s-verification-"))
    print(f"Evidence: {record}", flush=True)
    qemu = shutil.which("qemu-riscv64")
    assert qemu, "qemu-riscv64 is required"
    env = os.environ.copy()
    for key in ["CARGO_TARGET_DIR", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"]:
        env.pop(key, None)

    with tempfile.TemporaryDirectory(prefix="core-s-clean-") as directory:
        source = Path(directory)
        project = source / "core-s"
        for name in ["src/source.rs", "src/diagnostic.rs", "src/syntax/lexer.rs", "src/syntax/unicode.rs"]:
            target = source / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REPO / name, target)
        for name in ["src", "tests", "fixtures", "runtime", "bootstrap"]:
            shutil.copytree(CORE / name, project / name)
        for name in ["Cargo.toml", "Cargo.lock"]:
            shutil.copyfile(CORE / name, project / name)
        hashes = {str(p.relative_to(source)): hashlib.sha256(p.read_bytes()).hexdigest()
                  for p in sorted(source.rglob("*")) if p.is_file()}
        print("Running all regression tests in a fresh source tree…", flush=True)
        output = run(["cargo", "test", "--offline", "--locked", "--all-features"],
                     project, env, record / "tests.log")
        passed = sum(map(int, re.findall(r"test result: ok\. (\d+) passed", output)))
        baseline = (REPO / "docs/workflows/bootstrap-b31/N5-clean/N5-clean-tests.log").read_text()
        names = lambda text: Counter(re.findall(r"^test (\S+) \.\.\. ok$", text, re.M))
        retained = names(baseline)
        assert sum(retained.values()) == 91 and not retained - names(output)
        print(f"{passed} tests passed; all 91 B3.1 tests retained. Building C1/C2/C3…", flush=True)
        (project / "build").mkdir()
        run([str(project / "target/debug/core-s"), "project", "bootstrap/c1.build"],
            project, dict(env, PATH=""), record / "c1.log")
        for compiler, manifest, stage in [("c1", "c2", "c2"), ("c2", "c3", "c3")]:
            command = [qemu, "-s", "1048576", f"build/{compiler}.elf", "build", f"bootstrap/{manifest}.build"]
            if stage == "c3":
                command += ["--blocking", "bootstrap/pure.blocking"]
            statistics = run(command, project, {"PATH": ""}, record / f"{stage}.log")
            assert int(re.search(r"max_frame=(\d+)", statistics)[1]) <= 262144
            if stage == "c3":
                assert statistics.count(": Verified\n") == 6
        images = {name: (project / f"build/{name}.elf").read_bytes() for name in ["c1", "c2", "c3"]}
        assert images["c2"] == images["c3"], "self-hosting/analysis fixed point mismatch"
        result = {
            "passed": passed, "retained_b31_tests": 91, "c2_equals_c3": True,
            "blocking_verified_roots": 6, "guest_stack_bytes": 1048576,
            "sha256": {name: hashlib.sha256(data).hexdigest() for name, data in images.items()},
            "memory": {name: memory(data) for name, data in images.items()},
            "source_sha256": hashes,
        }
        binary = record / "compiler.elf"
        binary.write_bytes(images["c3"])
        binary.chmod(0o755)
        (record / "verification.json").write_text(json.dumps(result, indent=2) + "\n")
        print(f"Verified compiler: {binary}\nSHA-256: {result['sha256']['c3']}", flush=True)


if __name__ == "__main__":
    main()
