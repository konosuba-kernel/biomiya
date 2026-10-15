#!/usr/bin/env python3
"""Compile actual Q input/lexer modules with a specified target compiler and replay b3_input identities."""

# Assertions are verification checks, including in imported helpers.
if not __debug__:
    raise SystemExit("verification requires assertions; unset PYTHONOPTIMIZE and omit -O/-OO")

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

CORE = Path(__file__).resolve().parents[1]
SOURCES = ['compiler/model.vt', 'compiler/target.vt', 'compiler/runtime/host_linux.vt', 'compiler/lex.vt', 'compiler/input.vt', 'compiler/runtime/linux.vt', 'compiler/runtime/bytes.vt']
IDENTITIES = [
    "target_lexer_decodes_numbers_strings_ranges_and_nested_comments",
    "target_input_keeps_manifest_snapshots_and_handles_short_io",
    "target_rejects_bad_numeric_tokens_utf8_and_unsupported_identifiers",
    "target_manifest_limits_and_source_aliases_preserve_inputs",
]
PROJECT = "format 1\nprofile linux-user\nentry probe::run\nsource input.vt\nassembly adapter.s0\noutput copied.vt\n"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, required=True)
    parser.add_argument("--record-dir", type=Path)
    args = parser.parse_args()
    compiler = args.compiler.resolve()
    qemu = shutil.which("qemu-riscv64")
    if not qemu:
        parser.error("qemu-riscv64 is required")
    record = args.record_dir.resolve() if args.record_dir else Path(tempfile.mkdtemp(prefix="q-input-"))
    record.mkdir(parents=True, exist_ok=True)
    if (record / "verification.json").exists():
        parser.error("record directory already contains a verification result")
    snapshot = record / "source"
    snapshot.mkdir()
    inputs = {name: (CORE / name).read_bytes() for name in SOURCES}
    for name in ["compiler/runtime/linux.s0", "tests/compiler/input.vt", "tests/compiler/input-short.s0", "tests/compiler/input-write-failure.s0"]:
        inputs[name] = (CORE / name).read_bytes()
    inputs["tools/check_q_input.py"] = Path(__file__).read_bytes()
    for name, data in inputs.items():
        destination = snapshot / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)
    (record / "compiler.elf").write_bytes(compiler.read_bytes())
    (record / "compiler.elf").chmod(0o755)
    report = {"compiler": str(compiler), "compiler_sha256": digest(compiler.read_bytes()),
              "input_sha256": {name: digest(data) for name, data in inputs.items()},
              "builds": [], "tests": [], "passed": False}

    def invoke(command, cwd, timeout=180):
        result = subprocess.run(command, cwd=cwd, env={"PATH": ""}, capture_output=True,
                                text=True, timeout=timeout)
        return {"command": list(map(str, command)), "exit": result.returncode,
                "stdout": result.stdout, "stderr": result.stderr}

    def build(name, entry, shim=None):
        work = record / ("build-" + name)
        work.mkdir()
        for filename in SOURCES:
            destination = work / filename
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(inputs[filename])
        (work / "probe.vt").write_bytes(inputs["tests/compiler/input.vt"])
        assembly = inputs["compiler/runtime/linux.s0"].decode()
        if shim:
            assembly = inputs["tests/compiler/" + shim].decode() + assembly.replace("__b2_syscall6:", "__b2_real:")
        (work / "linux.s0").write_text(assembly)
        manifest = f"format 1\nprofile linux-user\nentry {entry}\n"
        manifest += "".join(f"source {filename}\n" for filename in SOURCES)
        manifest += "source probe.vt\nassembly linux.s0\noutput probe.elf\n"
        (work / "probe.build").write_text(manifest)
        outcome = invoke([qemu, "-s", "1048576", str(record / "compiler.elf"), "build", "probe.build"], work)
        report["builds"].append({"name": name, **outcome})
        assert outcome["exit"] == 0, outcome
        image = work / "probe.elf"
        report["builds"][-1]["elf_sha256"] = digest(image.read_bytes())
        return image

    def prepare(name, source=b"", manifest=PROJECT):
        work = record / ("case-" + name)
        work.mkdir()
        (work / "input.vt").write_bytes(source)
        (work / "adapter.s0").write_text("# retained assembly snapshot\n")
        (work / "project").write_text(manifest)
        return work

    def execute(identity, name, image, work, expected, diagnostic=None):
        outcome = invoke([qemu, "-s", "1048576", str(image), "project"], work, 30)
        report["tests"].append({"identity": identity, "case": name, **outcome})
        assert outcome["exit"] == expected, outcome
        if diagnostic:
            assert diagnostic in outcome["stderr"], outcome
        return outcome

    try:
        images = {"tokens": build("tokens", "probe::tokens"),
                  "capacity": build("capacity", "probe::capacity"),
                  "normal": build("normal", "probe::run"),
                  "short": build("short", "probe::run", "input-short.s0"),
                  "failure": build("failure", "probe::run", "input-write-failure.s0")}
        execute(IDENTITIES[0], "tokens", images["tokens"], prepare("tokens"), 0)
        execute(IDENTITIES[3], "memory-capacity-reset", images["capacity"], prepare("memory-capacity-reset"), 0)
        work = prepare("memory-capacity-over")
        outcome = invoke([qemu, "-s", "1048576", str(images["capacity"]), "overflow"], work, 30)
        report["tests"].append({"identity": IDENTITIES[3], "case": "memory-capacity-over", **outcome})
        assert outcome["exit"] == 1 and outcome["stderr"] == "actual=65 limit=64 I_LIMIT file=0 byte=0\n", outcome
        assert not (work / "copied.vt").exists()
        text = ("/* 注释 */ fn example()->u32{return 0xff_u8 as u32;}\n" * 180).encode()
        for mode in ["normal", "short"]:
            work = prepare(mode, text, PROJECT + "source empty.vt\n")
            (work / "empty.vt").write_bytes(b"")
            execute(IDENTITIES[1], mode, images[mode], work, 0)
            assert (work / "copied.vt").read_bytes() == text
            execute(IDENTITIES[1], mode + "-existing-output", images[mode], work, 1, "E_IO")
            assert (work / "copied.vt").read_bytes() == text
        work = prepare("partial-write-cleanup", b" " * 32)
        execute(IDENTITIES[1], "partial-write-cleanup", images["failure"], work, 1, "E_IO")
        assert not (work / "copied.vt").exists(), "failed output must be removed"
        assert (work / "input.vt").read_bytes() == b" " * 32
        bad = [(b"0x", "E_PARSE", 0), (b"1__2", "E_PARSE", 2), (b"256_u8", "E_TYPE", 0),
               (b"18446744073709551616", "E_TYPE", 0), (b"0b2", "E_PARSE", 2),
               (b"1_u64", "I_UNSUPPORTED", 1), (b'"\\q"', "E_PARSE", 2),
               (b"/*bad", "E_PARSE", 0), (b"\xff", "E_PARSE", 0), ("变量".encode(), "I_UNSUPPORTED", 0)]
        for index, (source, code, offset) in enumerate(bad):
            name = f"bad-token-{index}"
            work = prepare(name, source)
            execute(IDENTITIES[2], name, images["normal"], work, 1, f"{code} file=1 byte={offset}\n")
            assert not (work / "copied.vt").exists()
        manifests = [PROJECT + "entry twice\n", PROJECT.replace("source input.vt", "source ../input.vt"),
                     PROJECT + "assembly input.vt\n", PROJECT.replace("linux-user", "other"),
                     PROJECT + "unknown x\n", PROJECT.replace("input.vt", "missing.vt")]
        source = b"fn f()->u32{return 1;}"
        for index, manifest in enumerate(manifests):
            name = f"bad-manifest-{index}"
            work = prepare(name, source, manifest)
            execute(IDENTITIES[3], name, images["normal"], work, 1)
            assert not (work / "copied.vt").exists()
            assert (work / "input.vt").read_bytes() == source
        work = prepare("output-alias", source, PROJECT.replace("output copied.vt", "output input.vt"))
        execute(IDENTITIES[3], "output-alias", images["normal"], work, 1)
        assert (work / "input.vt").read_bytes() == source
        work = prepare("source-limit", b" " * (4 * 1024 * 1024 + 1))
        execute(IDENTITIES[3], "source-limit", images["normal"], work, 1,
                "actual=4194305 limit=4194304 I_LIMIT file=1 byte=4194304")
        assert not (work / "copied.vt").exists()
        assert {test["identity"] for test in report["tests"]} == set(IDENTITIES)
        report["passed"] = True
    finally:
        (record / "verification.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"passed": True, "identities": len(IDENTITIES), "cases": len(report["tests"]),
                      "record": str(record), "compiler_sha256": report["compiler_sha256"]}))


if __name__ == "__main__":
    main()
