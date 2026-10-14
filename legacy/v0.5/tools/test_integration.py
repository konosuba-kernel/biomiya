#!/usr/bin/env python3
"""Supplemental Core groups built from language sources and executed as RV64 ELF.

Hand-built MIR and independent C platform probes cannot satisfy these groups.
Artifacts and source/object/ELF hashes remain available for review.
"""

import argparse
import hashlib
import json
import pathlib
import re
import subprocess
from conformance import invoke
from test_runtime import FLAGS, run as run_target

ROOT = pathlib.Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "tests/integration"
GROUPS = {
    "modules": [
        {
            "id": "merged-modules",
            "sources": ["modules_a.vt", "modules_b.vt"],
            "test": "application::test",
            "value": 42,
        }
    ],
    "c-abi": [
        {
            "id": "c-split-sret-callback",
            "sources": ["c_abi.vt"],
            "objects": ["c_abi.c"],
            "value": 42,
        }
    ],
    "call-snapshots": [
        {
            "id": "argument-snapshot",
            "sources": ["../conformance/run_call_snapshot.vt"],
            "value": 29,
        },
        {
            "id": "aggregate-result",
            "sources": ["../conformance/run_aggregate_call.vt"],
            "value": 92,
        },
        {
            "id": "return-snapshot-and-zst-evaluation",
            "sources": ["return_snapshot.vt"],
            "value": 183,
        },
    ],
    "raw-storage": [
        {
            "id": "packed-uninitialized-storage",
            "sources": ["raw_storage.vt"],
            "value": 42,
        },
        {
            "id": "raw-slice-view",
            "sources": ["../conformance/run_raw_slice.vt"],
            "value": 16,
        },
        {
            "id": "raw-constant-address",
            "sources": ["../conformance/run_const_raw_address.vt"],
            "value": 7,
        },
    ],
    "external-data-relocations": [
        {
            "id": "byte-exact-export-and-section",
            "sources": ["byte_names.vt"],
            "value": 42,
            "byte_names": True,
        },
        {
            "id": "external-object-constant-relocation",
            "sources": ["external_data.vt"],
            "objects": ["external_data.c"],
            "value": 42,
        },
    ],
    "atomics": [
        {
            "id": "narrow-rmw-strong-cas",
            "sources": ["../conformance/run_atomic.vt"],
            "value": 262,
        },
        {
            "id": "all-widths-operations-neighbor-storage",
            "sources": ["atomic_widths.vt"],
            "value": 42,
        },
        {
            "id": "two-hart-publication-rmw-cas",
            "sources": ["atomic_multihart.vt"],
            "objects": ["atomic_multihart.c"],
            "value": 7,
            "harts": 2,
        },
        {
            "id": "strong-cas-forced-reservation-loss",
            "sources": ["cas_reservation_loss.vt"],
            "value": 42,
            "force_reservation_loss": True,
        },
    ],
    "mmio": [
        {
            "id": "uart-byte-access",
            "sources": ["mmio.vt"],
            "value": 145,
            "trace": "uart",
        },
        {
            "id": "clint-wide-access",
            "sources": ["mmio_wide.vt"],
            "value": 1,
            "trace": "clint",
        },
        {
            "id": "cfi-halfword-access",
            "sources": ["mmio16.vt"],
            "value": 1,
            "trace": "flash",
        },
    ],
    "traps": [
        {
            "id": "trap-frame-registers-and-transfer-defer",
            "sources": ["traps.vt"],
            "value": 33,
        },
        {"id": "software-irq-csr", "sources": ["irq.vt"], "value": 1},
        {
            "id": "all-integer-registers",
            "sources": ["registers.vt"],
            "objects": ["registers.c", "../runtime/registers.S"],
            "value": 31,
        },
        {
            "id": "nested-trap-reliable-panic",
            "sources": ["nested_trap.vt"],
            "value": None,
            "expected_status": "panic",
        },
    ],
    "link-runtime": [
        {
            "id": "aligned-stack-runtime-link",
            "sources": ["link_runtime.vt"],
            "value": 42,
        }
    ],
}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def trace_evidence(path, kind):
    trace = path.read_text(errors="replace")
    addresses = {
        "uart": ["0x10000007"],
        "clint": ["0x2004000"],
        "flash": ["0x20000000", "0x20000040", "0x20000044", "0x20000048"],
    }[kind]
    lines = [
        line
        for line in trace.splitlines()
        if "memory_region_ops_" in line
        and any("addr " + address + " " in line for address in addresses)
    ]
    operations = []
    for line in lines:
        match = re.search(
            r"memory_region_ops_(read|write).*addr (0x[0-9a-f]+) value (0x[0-9a-f]+) size (\d+)",
            line,
        )
        if match:
            operations.append(list(match.groups()))
    expected = {
        "uart": [
            ["write", "0x10000007", "0x5a", "1"],
            ["read", "0x10000007", "0x5a", "1"],
            ["write", "0x10000007", "0x37", "1"],
            ["read", "0x10000007", "0x37", "1"],
        ],
        "clint": [
            ["write", "0x2004000", "0x7fffffffffffffff", "8"],
            ["read", "0x2004000", "0x7fffffffffffffff", "8"],
            ["write", "0x2004000", "0xfffffffe", "4"],
            ["read", "0x2004000", "0xfffffffe", "4"],
        ],
        "flash": [
            ["write", "0x20000000", "0x98", "2"],
            ["read", "0x20000040", "0x510051", "2"],
            ["read", "0x20000044", "0x520052", "2"],
            ["read", "0x20000048", "0x590059", "2"],
            ["write", "0x20000000", "0xff", "2"],
        ],
    }[kind]
    fences = sorted(set(re.findall(r"^(0x[0-9a-f]+):.*fence\s+iorw,iorw", trace, re.M)))
    return {
        "trace_path": str(path),
        "operations": operations,
        "expected_operations": expected,
        "device_fence_sites": fences,
        "passed": operations == expected and (kind != "uart" or len(fences) >= 2),
    }


def forced_reservation_loss(command, compiler, directory):
    assembly_path = directory / "production.s"
    asm_command = list(command)
    asm_command[asm_command.index("--emit") + 1] = "asm"
    asm_command[asm_command.index("-o") + 1] = str(assembly_path)
    compilation = invoke(asm_command)
    evidence = {
        "method": "test-only injection into compiler-produced assembly; original production ELF executed separately",
        "assembly_compilation": compilation,
        "passed": False,
    }
    if compilation["status"] != "ok" or not assembly_path.is_file():
        return evidence
    original = assembly_path.read_text()
    lines = original.splitlines()
    loads = [
        n
        for n, line in enumerate(lines)
        if re.match(r"\s*lr\.w\.aqrl t2, \(t0\)", line)
    ]
    stores = [
        n
        for n, line in enumerate(lines)
        if re.match(r"\s*sc\.w\.rl t3, t4, \(t0\)", line)
    ]
    if len(loads) != 1 or len(stores) != 1:
        evidence["error"] = (
            "fixture must contain exactly one compiler strong-CAS LR/SC sequence"
        )
        return evidence
    load, store = loads[0], stores[0]
    sequence = [
        line.strip()
        for line in lines[load : store + 2]
        if line.strip() and not line.lstrip().startswith((".", "#"))
    ]
    evidence["production_lrsc_sequence"] = sequence
    retry_label = lines[load - 1].strip().removesuffix(":")
    allowed_ops = {"lr.w.aqrl", "slli", "srli", "bne", "sc.w.rl", "bnez"}
    evidence["production_constrained_loop_valid"] = (
        len(sequence) == 6
        and all(line.split()[0] in allowed_ops for line in sequence)
        and sequence[0] == "lr.w.aqrl t2, (t0)"
        and sequence[4] == "sc.w.rl t3, t4, (t0)"
        and sequence[5] == f"bnez t3, {retry_label}"
        and lines[load - 1].strip().endswith(":")
    )
    evidence["production_loop_invariants"] = {
        "instruction_count": len(sequence),
        "bytes": len(sequence) * 4,
        "isa_instructions_between_lr_sc": [line.split()[0] for line in sequence[1:4]],
        "same_address_and_width": True,
        "retry_branch_targets_lr": sequence[5] == f"bnez t3, {retry_label}",
        "calls_loads_stores_fences_system_between_lr_sc": False,
        "memory_eventuality": "QEMU virt ordinary RAM; platform contract",
        "isa_source": "https://docs.riscv.org/reference/isa/v20240411/unpriv/a-st-ext.html",
        "mapping_source": "https://github.com/riscv-non-isa/riscv-elf-psabi-doc/blob/48969b7995b3ae1582f230dcce32a12d443d1a7a/riscv-atomic.adoc",
    }
    if not evidence["production_constrained_loop_valid"]:
        evidence["error"] = (
            "production LR/SC sequence violated the reviewed constrained-loop shape"
        )
        return evidence
    # The one-shot flag is initialized before the retry label. A dummy successful
    # SC writes the same expected zero, consuming the reservation. The original
    # SC must then fail, and the production bnez must retry. Original LR/SC
    # instructions remain unchanged; injected guards/checks are test-only.
    label = load - 1
    if not lines[label].rstrip().endswith(":"):
        evidence["error"] = "could not identify production CAS retry label"
        return evidence
    transformed = []
    for n, line in enumerate(lines):
        if n == label:
            transformed += ["li t5, 1"]
        if n == store:
            transformed += [
                "beqz t5, .Lforced_skip",
                "li t5, 2",
                "sc.w t6, zero, (t0)",
                "bnez t6, .Lforced_bad",
                ".Lforced_skip:",
            ]
        transformed.append(line)
        if n == store:
            transformed += [
                "li t6, 2",
                "bne t5, t6, .Lforced_checked",
                "beqz t3, .Lforced_bad",
                "li t5, 0",
                ".Lforced_checked:",
            ]
    transformed += ['.section .text,"ax",@progbits', ".Lforced_bad:", "unimp"]
    instrumented = directory / "forced-reservation-loss.S"
    instrumented.write_text("\n".join(transformed) + "\n")
    evidence["production_assembly_sha256"] = sha(assembly_path)
    evidence["instrumented_assembly_sha256"] = sha(instrumented)
    evidence["injection_count"] = 1
    evidence["assertions"] = [
        "dummy SC succeeded while writing unchanged expected zero",
        "first original SC returned failure after reservation consumption",
        "original retry branch executed",
        "second attempt returned Ok(0) and stored 42",
    ]
    commands = []
    objects = []
    for n, source in enumerate(
        [
            ROOT / "platforms/qemu-virt/start.S",
            ROOT / "runtime/rv64/trap.S",
            ROOT / "runtime/rv64/runtime.c",
            instrumented,
        ]
    ):
        object_path = directory / f"forced-{n}.o"
        cmd = ["clang", *FLAGS, "-c", str(source), "-o", str(object_path)]
        process = subprocess.run(
            cmd, cwd=ROOT, capture_output=True, text=True, timeout=30
        )
        commands.append(
            {"command": cmd, "exit": process.returncode, "stderr": process.stderr}
        )
        if process.returncode:
            evidence.update(
                commands=commands, error="instrumented assembly build failed"
            )
            return evidence
        objects.append(str(object_path))
    elf = directory / "forced-reservation-loss.elf"
    cmd = [
        "ld.lld",
        "-m",
        "elf64lriscv",
        "--no-relax",
        "-T",
        str(ROOT / "platforms/qemu-virt/link.ld"),
        *objects,
        "-o",
        str(elf),
    ]
    process = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, timeout=30)
    commands.append(
        {"command": cmd, "exit": process.returncode, "stderr": process.stderr}
    )
    evidence["commands"] = commands
    if process.returncode:
        evidence["error"] = "instrumented ELF link failed"
        return evidence
    execution = run_target(elf)
    evidence["execution"] = execution
    evidence["instrumented_elf_sha256"] = sha(elf)
    evidence["passed"] = execution["status"] == "pass" and execution.get("value") == 42
    return evidence


def elf_names(path):
    import struct

    data = path.read_bytes()
    headers = struct.unpack_from("<Q", data, 40)[0]
    count, strings = struct.unpack_from("<HH", data, 60)
    sections = [
        struct.unpack_from("<IIQQQQIIQQ", data, headers + n * 64) for n in range(count)
    ]
    string_section = sections[strings]
    table = data[string_section[4] : string_section[4] + string_section[5]]
    section_names = [table[section[0] :].split(b"\0", 1)[0] for section in sections]
    symbols = []
    for section in sections:
        if section[1] != 2:
            continue
        names = sections[section[6]]
        table = data[names[4] : names[4] + names[5]]
        for n in range(section[5] // 24):
            index = struct.unpack_from("<I", data, section[4] + n * 24)[0]
            symbols.append(table[index:].split(b"\0", 1)[0])
    return symbols, section_names


def byte_name_evidence(command, compiler, elf, directory):
    expected_symbols = [
        b"byte_\xff",
        b"byte_\xfe",
        b'quote"slash\\end\\',
        b"line\nbreak",
        "汉字".encode(),
        b'data_\xfe"\\',
    ]
    expected_sections = [b'.custom_\xff"\\', b".data_custom_\xfe\\"]
    evidence = {
        "expected_symbols_hex": [name.hex() for name in expected_symbols],
        "expected_sections_hex": [name.hex() for name in expected_sections],
        "passed": True,
        "outputs": [],
    }

    def inspect(path, kind):
        symbols, sections = elf_names(path)
        result = {
            "kind": kind,
            "path": str(path),
            "sha256": sha(path),
            "matched_symbols_hex": [
                name.hex() for name in expected_symbols if name in symbols
            ],
            "matched_sections_hex": [
                name.hex() for name in expected_sections if name in sections
            ],
            "unrestored_aliases": [
                name.hex()
                for name in symbols + sections
                if name.startswith((b"__biomiya_bytes_", b".biomiya.section."))
            ],
        }
        result["passed"] = (
            all(name in symbols for name in expected_symbols)
            and all(name in sections for name in expected_sections)
            and not result["unrestored_aliases"]
        )
        evidence["outputs"].append(result)
        evidence["passed"] &= result["passed"]

    inspect(elf, "compiler-linked ELF")
    object_path = directory / "byte-names.o"
    object_command = list(command)
    object_command[object_command.index("--emit") + 1] = "obj"
    object_command[object_command.index("-o") + 1] = str(object_path)
    object_build = invoke(object_command)
    evidence["object_compilation"] = object_build
    if object_build["status"] != "ok" or not object_path.is_file():
        evidence["passed"] = False
        return evidence
    inspect(object_path, "compiler object")
    assembly_path = directory / "byte-names.s"
    assembly_command = list(command)
    assembly_command[assembly_command.index("--emit") + 1] = "asm"
    assembly_command[assembly_command.index("-o") + 1] = str(assembly_path)
    assembly_build = invoke(assembly_command)
    evidence["assembly_compilation"] = assembly_build
    sidecar = pathlib.Path(str(assembly_path) + ".linkage.json")
    if assembly_build["status"] != "ok" or not sidecar.is_file():
        evidence["passed"] = False
        return evidence
    evidence["sidecar"] = {
        "path": str(sidecar),
        "sha256": sha(sidecar),
        "mapping": json.loads(sidecar.read_text()),
    }
    assembled = directory / "reassembled.o"
    result = invoke(
        [
            str(compiler),
            "assemble",
            "--diagnostics",
            "json",
            "-o",
            str(assembled),
            str(assembly_path),
        ]
    )
    evidence["manual_assembly"] = result
    if result["status"] != "ok" or not assembled.is_file():
        evidence["passed"] = False
        return evidence
    inspect(assembled, "assemble command with automatic linkage sidecar")
    return evidence


def execute(case, compiler, directory):
    directory.mkdir(parents=True, exist_ok=True)
    record = {
        "id": case["id"],
        "expected_value": case["value"],
        "passed": False,
        "evidence_kind": "compiled-language-ELF",
    }
    objects = []
    record["object_builds"] = []
    for object_index, source_name in enumerate(case.get("objects", [])):
        source = (FIXTURES / source_name).resolve()
        object_path = directory / (str(object_index) + "-" + source.stem + ".o")
        command = ["clang", *FLAGS, "-c", str(source), "-o", str(object_path)]
        try:
            process = subprocess.run(
                command, cwd=ROOT, capture_output=True, text=True, timeout=30
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            record["outcome"] = "infrastructure-failure"
            record["error"] = str(error)
            return record
        build = {
            "command": command,
            "exit": process.returncode,
            "stdout": process.stdout,
            "stderr": process.stderr,
            "source_sha256": sha(source),
        }
        record["object_builds"].append(build)
        if process.returncode:
            record["outcome"] = "infrastructure-failure"
            return record
        build["object_sha256"] = sha(object_path)
        objects.append(object_path)
    sources = [(FIXTURES / source).resolve() for source in case["sources"]]
    record["sources"] = [
        {"path": str(source), "sha256": sha(source)} for source in sources
    ]
    elf = directory / "program.elf"
    if elf.exists():
        elf.unlink()
    command = [
        str(compiler),
        "build",
        "--target",
        "rv64-core-v1",
        "--diagnostics",
        "json",
        "--platform",
        "qemu-virt-core-v1",
        "--emit",
        "elf",
        "--test",
        case.get("test", "test"),
        "-o",
        str(elf),
    ]
    for object_path in objects:
        command += ["--object", str(object_path)]
    command += list(map(str, sources))
    compilation = invoke(command)
    record["compilation"] = compilation
    if compilation["status"] != "ok":
        record["outcome"] = compilation["status"]
        return record
    if not elf.is_file():
        record["outcome"] = "infrastructure-failure"
        record["error"] = "successful compiler build omitted ELF"
        return record
    header = elf.read_bytes()[:20]
    if (
        len(header) < 20
        or header[:4] != b"\x7fELF"
        or header[4:6] != b"\x02\x01"
        or int.from_bytes(header[18:20], "little") != 243
    ):
        record["outcome"] = "infrastructure-failure"
        record["error"] = "output is not RV64 little-endian ELF"
        return record
    record["elf"] = {
        "path": str(elf),
        "sha256": sha(elf),
        "class": "ELF64",
        "machine": "RISC-V",
        "endian": "little",
    }
    trace = directory / "qemu.trace"
    extra = (
        [
            "-trace",
            "enable=memory_region_ops_read",
            "-trace",
            "enable=memory_region_ops_write",
            "-d",
            "in_asm",
            "-D",
            str(trace),
        ]
        if case.get("trace")
        else []
    )
    execution = run_target(elf, harts=case.get("harts", 1), extra=extra)
    record["execution"] = execution
    record["passed"] = (
        execution["status"] == case.get("expected_status", "pass")
        and execution.get("value") == case["value"]
    )
    if case.get("trace") and trace.is_file():
        evidence = trace_evidence(trace, case["trace"])
        record["device_evidence"] = evidence
        record["passed"] &= evidence["passed"]
    if case.get("force_reservation_loss") and record["passed"]:
        evidence = forced_reservation_loss(command, compiler, directory)
        record["reservation_loss_evidence"] = evidence
        record["passed"] &= evidence["passed"]
    if case.get("byte_names") and record["passed"]:
        evidence = byte_name_evidence(command, compiler, elf, directory)
        record["byte_name_evidence"] = evidence
        record["passed"] &= evidence["passed"]
    record["outcome"] = (
        "matched-execution"
        if record["passed"]
        else execution["status"]
        if execution["status"] != "pass"
        else "evidence-mismatch"
    )
    return record


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", default=str(ROOT / "target/debug/biomiya"))
    parser.add_argument("--json", type=pathlib.Path)
    parser.add_argument(
        "--artifacts", type=pathlib.Path, default=ROOT / "build/integration"
    )
    parser.add_argument("--group", action="append", choices=list(GROUPS))
    args = parser.parse_args(argv)
    compiler = pathlib.Path(args.compiler).resolve()
    groups = []
    for name, cases in GROUPS.items():
        if args.group and name not in args.group:
            groups.append(
                {
                    "id": name,
                    "status": "incomplete",
                    "reason": "not selected",
                    "cases": [],
                }
            )
            continue
        results = []
        for case in cases:
            result = execute(case, compiler, args.artifacts / name / case["id"])
            results.append(result)
            print(
                f"{name}/{case['id']}: {'PASS' if result['passed'] else 'FAIL'} {result['outcome']}",
                flush=True,
            )
        status = (
            "passed"
            if all(r["passed"] for r in results)
            else "incomplete"
            if any(
                r["outcome"] in ("unsupported", "infrastructure-failure")
                for r in results
            )
            else "failed"
        )
        groups.append({"id": name, "status": status, "cases": results})
    report = {
        "schema": 1,
        "target": "rv64-core-v1",
        "platform": "qemu-virt-core-v1",
        "evidence_kind": "compiled-language-ELF",
        "groups": groups,
    }
    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps(report, indent=2) + "\n")
    return (
        0
        if all(
            g["status"] == "passed"
            for g in groups
            if not args.group or g["id"] in args.group
        )
        else 1
    )


if __name__ == "__main__":
    raise SystemExit(main())
