#!/usr/bin/env python3
"""Independent platform probes, deliberately not language conformance results."""

import argparse
import json
import pathlib
import re
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
FLAGS = [
    "--target=riscv64-unknown-elf",
    "-march=rv64ima_zicsr_zifencei",
    "-mabi=lp64",
    "-mno-relax",
    "-mcmodel=medany",
    "-ffreestanding",
    "-fno-builtin",
    "-fno-stack-protector",
    "-O1",
    "-I" + str(ROOT / "runtime/rv64"),
]


def build(source, out):
    objects = []
    for i, p in enumerate(
        [
            ROOT / "platforms/qemu-virt/start.S",
            ROOT / "runtime/rv64/trap.S",
            ROOT / "runtime/rv64/runtime.c",
            source,
            *(
                [ROOT / "tests/runtime/registers.S"]
                if source.stem == "registers"
                else []
            ),
        ]
    ):
        obj = out.parent / f"{i}.o"
        subprocess.run(["clang", *FLAGS, "-c", str(p), "-o", str(obj)], check=True)
        objects.append(str(obj))
    subprocess.run(
        [
            "ld.lld",
            "-m",
            "elf64lriscv",
            "--no-relax",
            "-T",
            str(ROOT / "platforms/qemu-virt/link.ld"),
            *objects,
            "-o",
            str(out),
        ],
        check=True,
    )


def run(elf, harts=1, extra=(), timeout=5):
    cmd = [
        "qemu-system-riscv64",
        "-machine",
        "virt",
        "-accel",
        "tcg",
        "-cpu",
        "rv64",
        "-m",
        "128M",
        "-smp",
        str(harts),
        "-bios",
        "none",
        "-display",
        "none",
        "-monitor",
        "none",
        "-serial",
        "stdio",
        "-kernel",
        str(elf),
        *extra,
    ]
    try:
        p = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as e:
        output = e.stdout or ""
        stderr = e.stderr or ""
        if isinstance(output, bytes):
            output = output.decode("utf-8", errors="replace")
        if isinstance(stderr, bytes):
            stderr = stderr.decode("utf-8", errors="replace")
        return {"status": "timeout", "output": output + stderr, "command": cmd}
    if p.returncode != 0:
        return {
            "status": "platform-fault",
            "output": p.stdout + p.stderr,
            "exit": p.returncode,
            "command": cmd,
        }
    matches = re.findall("BIOMIYA:(PASS:(-?\\d+)|PANIC|FAULT)", p.stdout)
    if len(matches) != 1:
        return {
            "status": "platform-fault",
            "output": p.stdout + p.stderr,
            "exit": p.returncode,
            "command": cmd,
        }
    tag, value = matches[0]
    return {
        "status": "pass"
        if tag.startswith("PASS:")
        else "panic"
        if tag == "PANIC"
        else "platform-fault",
        "value": int(value) if value else None,
        "output": p.stdout,
        "command": cmd,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    results = []
    expected = {
        "normal": ("pass", -2147483648),
        "panic": ("panic", None),
        "fault": ("platform-fault", None),
        "trap": ("pass", 31),
        "nested": ("panic", None),
        "atomic": ("pass", 7),
        "irq": ("pass", 1),
        "registers": ("pass", 31),
        "mmio": ("pass", 145),
        "timeout": ("timeout", None),
        "bad_return": ("platform-fault", None),
        "kernel": ("pass", 0),
        "mmio16": ("pass", 1),
    }
    for name, (status, value) in expected.items():
        with tempfile.TemporaryDirectory(prefix="biomiya-runtime-") as d:
            elf = pathlib.Path(d) / "probe.elf"
            build(ROOT / f"tests/runtime/{name}.c", elf)
            r = run(
                elf,
                2 if name == "atomic" else 1,
                extra=[
                    "-trace",
                    "enable=memory_region_ops_read",
                    "-trace",
                    "enable=memory_region_ops_write",
                    "-d",
                    "in_asm",
                    "-D",
                    str(pathlib.Path(d) / "trace"),
                ]
                if name in ("mmio", "mmio16")
                else [],
                timeout=0.2 if name == "timeout" else 5,
            )
            r["id"] = name
            r["ok"] = r["status"] == status and r.get("value") == value
            if name == "mmio":
                trace = (pathlib.Path(d) / "trace").read_text()
                accesses = [
                    line
                    for line in trace.splitlines()
                    if "memory_region_ops_" in line and "addr 0x10000007 " in line
                ]
                r["mmio_accesses"] = accesses
                kinds = [
                    re.search(
                        "memory_region_ops_(read|write).*value (0x[0-9a-f]+) size (\\d+)",
                        line,
                    ).groups()
                    for line in accesses
                ]
                fences = set(
                    re.findall("^(0x[0-9a-f]+):.*fence\\s+iorw,iorw", trace, re.M)
                )
                timer = [
                    line
                    for line in trace.splitlines()
                    if "memory_region_ops_" in line and "addr 0x2004000 " in line
                ]
                r["timer_accesses"] = timer
                timer_kinds = [
                    re.search(
                        "memory_region_ops_(read|write).*value (0x[0-9a-f]+) size (\\d+)",
                        line,
                    ).groups()
                    for line in timer
                ]
                r["ok"] &= timer_kinds == [
                    ("write", "0x7fffffffffffffff", "8"),
                    ("read", "0x7fffffffffffffff", "8"),
                    ("write", "0xfffffffe", "4"),
                    ("read", "0xfffffffe", "4"),
                ]
                r["device_fence_sites"] = sorted(fences)
                r["ok"] &= (
                    kinds
                    == [
                        ("write", "0x5a", "1"),
                        ("read", "0x5a", "1"),
                        ("write", "0x37", "1"),
                        ("read", "0x37", "1"),
                    ]
                    and len(fences) == 2
                )
            if name == "mmio16":
                trace = (pathlib.Path(d) / "trace").read_text()
                flash = [
                    line
                    for line in trace.splitlines()
                    if "memory_region_ops_" in line and "name 'virt.flash0'" in line
                ]
                kinds = [
                    re.search(
                        "memory_region_ops_(read|write).*addr (0x[0-9a-f]+) value (0x[0-9a-f]+) size (\\d+)",
                        line,
                    ).groups()
                    for line in flash
                ]
                r["flash_accesses"] = flash
                r["ok"] &= kinds == [
                    ("write", "0x20000000", "0x98", "2"),
                    ("read", "0x20000040", "0x510051", "2"),
                    ("read", "0x20000044", "0x520052", "2"),
                    ("read", "0x20000048", "0x590059", "2"),
                    ("write", "0x20000000", "0xff", "2"),
                ]
            results.append(r)
    print(
        json.dumps(results, indent=2)
        if args.json
        else "\n".join(
            (
                f"{r['id']}: {('ok' if r['ok'] else 'FAIL')} {r['status']} {r.get('value')}"
                for r in results
            )
        )
    )
    return 0 if all((r["ok"] for r in results)) else 1


if __name__ == "__main__":
    raise SystemExit(main())
