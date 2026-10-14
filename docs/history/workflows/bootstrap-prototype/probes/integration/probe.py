#!/usr/bin/env python3
"""Independent final integration checks. Run after building core-s --all-features."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[5]
BINARY = ROOT / "core-s/target/debug/core-s"


def run(arguments, **kwargs):
    return subprocess.run([str(a) for a in arguments], capture_output=True, timeout=30, **kwargs)


def success(arguments, **kwargs):
    result = run(arguments, **kwargs)
    assert result.returncode == 0, (arguments, result.stderr.decode(errors="replace"))
    return result


def main():
    with tempfile.TemporaryDirectory(prefix="core-s-final-review-") as directory:
        temp = Path(directory)
        source = temp / "大跨度.vt"
        source.write_text(
            "fn 入口()->u32 { var x:usize=0; var n:u32=0; while n<2_u32 { "
            "if n==0_u32 { n=n+1; continue; } "
            + "x=x+1; " * 600
            + "n=n+1; } return (x%251_usize) as u32; }"
        )
        no_tools = dict(os.environ, PATH="")
        direct = temp / "direct.elf"
        assembly = temp / "reference.s"
        success([BINARY, "build", source, "--entry", "入口", "-o", direct], env=no_tools)
        assert run(["qemu-riscv64", direct]).returncode == 98
        success([BINARY, "asm", source, "--entry", "入口", "-o", assembly], env=no_tools)
        success(["clang", "--target=riscv64-unknown-elf", "-march=rv64im", "-mabi=lp64",
                 "-mno-relax", "-c", assembly, "-o", temp / "reference.o"])
        success(["ld.lld", "--no-relax", "-e", "_start", "-Ttext=0x10000",
                 temp / "reference.o", "-o", temp / "reference.elf"])
        assert run(["qemu-riscv64", temp / "reference.elf"]).returncode == 98

        small = temp / "small.vt"
        small_source = "fn run()->u32 { return 101; }"
        small.write_text(small_source)
        success([BINARY, "build", small, "--entry", "run", "-o", temp / "normal101.elf"])
        assert run(["qemu-riscv64", temp / "normal101.elf"]).returncode == 101
        old = temp / "old.s"
        old.write_bytes(b"old preserved")
        for arguments, code in [
            (["asm", small, "--entry", "absent", "-o", old], b"E_ENTRY"),
            (["asm", small, "--entry", "run", "-o", temp], b"I_IO"),
            (["build", temp / "absent", "--entry", "run", "-o", old], b"I_IO"),
        ]:
            result = run([BINARY, *arguments])
            assert result.returncode != 0 and code in result.stderr
            assert old.read_bytes() == b"old preserved"
        assert not list(temp.glob(".core-s-*"))
        for mode in ["build", "asm"]:
            for kind in ["hard", "symlink"]:
                alias = temp / f"{mode}-{kind}"
                if kind == "hard":
                    os.link(small, alias)
                else:
                    alias.symlink_to(small)
                result = run([BINARY, mode, small, "--entry", "run", "-o", alias])
                assert result.returncode != 0 and b"I_IO" in result.stderr
                assert small.read_text() == small_source

        clean = temp / "clean"
        snapshot = ROOT / "docs/workflows/bootstrap-prototype/I-clean-build-r2.json"
        inputs = json.loads(snapshot.read_text())["input_hashes"]
        for name, digest in inputs.items():
            original = ROOT / name
            assert hashlib.sha256(original.read_bytes()).hexdigest() == digest
            destination = clean / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(original, destination)
        clean_environment = dict(os.environ)
        clean_environment.pop("CARGO_TARGET_DIR", None)
        success(["cargo", "build", "--offline", "--manifest-path",
                 clean / "core-s/Cargo.toml", "--all-features"], env=clean_environment)
        success([clean / "core-s/target/debug/core-s", "build", source,
                 "--entry", "入口", "-o", temp / "clean.elf"], env=no_tools)
        assert direct.read_bytes() == (temp / "clean.elf").read_bytes()
        metrics = json.loads((ROOT / "docs/workflows/bootstrap-prototype/metrics-r1.json").read_text())
        for group in ["own", "path_reused", "generated", "tests"]:
            files = [ROOT / entry["path"] for entry in metrics[group]["files"]]
            counted = json.loads(success(["cloc", "--json", "--quiet", *files]).stdout)
            assert counted["SUM"]["code"] == metrics[group]["total"]["code"]
        print(json.dumps({"status": "passed", "source_files_in_clean_build": len(inputs),
                          "large_elf_bytes": direct.stat().st_size,
                          "large_elf_sha256": hashlib.sha256(direct.read_bytes()).hexdigest()}, indent=2))


if __name__ == "__main__":
    main()
