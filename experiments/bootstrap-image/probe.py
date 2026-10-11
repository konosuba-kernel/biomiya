#!/usr/bin/env python3
"""Direct RV64/ELF construction probe, not a language compiler or SOS runtime.

The tested image uses Linux write/exit solely as a qemu-user execution adapter.
Clang/LLD build an independent instruction-encoding oracle, never the tested image.
"""
import hashlib
import json
from pathlib import Path
import struct
import subprocess


PAGE = 4096
TEXT = 0x10000
MESSAGE = b"direct RV64 image: fixups and BSS passed\n"
PREFIX = 3000  # Exercise carry into the high half of a PC-relative address.


def align(n, a=PAGE):
    return (n + a - 1) & -a


def signed(n, bits):
    if not -(1 << (bits - 1)) <= n < (1 << (bits - 1)):
        raise ValueError(f"{n} does not fit signed {bits} bits")
    return n & ((1 << bits) - 1)


def iword(op, rd, f3, rs, imm):
    return signed(imm, 12) << 20 | rs << 15 | f3 << 12 | rd << 7 | op


def branch_eq(rs, rt, delta):
    if delta % 4:
        raise ValueError("unaligned branch in non-compressed profile")
    v = signed(delta, 13)
    return ((v >> 12) << 31 | ((v >> 5) & 63) << 25 | rt << 20
            | rs << 15 | ((v >> 1) & 15) << 8 | ((v >> 11) & 1) << 7 | 0x63)


def pcrel_words(rd, target, pc, link=None):
    delta = target - pc
    high = (delta + 0x800) // 4096
    low = delta - high * 4096
    upper = signed(high, 20) << 12 | rd << 7 | 0x17
    lower = iword(0x13 if link is None else 0x67,
                  rd if link is None else link, 0, rd, low)
    return upper, lower


class Code:
    def __init__(self):
        self.words = []
        self.labels = {}
        self.fixups = []
        self.asm = [".option norvc", ".option norelax", ".text", ".globl _start"]

    def label(self, name):
        if name in self.labels:
            raise ValueError("duplicate label")
        self.labels[name] = len(self.words) * 4
        self.asm.append(name + ":")

    def word(self, value, assembly):
        self.words.append(value)
        self.asm.append(assembly)

    def addi(self, rd, rs, n):
        self.word(iword(0x13, rd, 0, rs, n), f"addi x{rd},x{rs},{n}")

    def ld(self, rd, rs, n=0):
        self.word(iword(0x03, rd, 3, rs, n), f"ld x{rd},{n}(x{rs})")

    def address(self, rd, symbol, link=None):
        at = len(self.words) * 4
        anchor = f".Lref{len(self.fixups)}"
        self.fixups.append((at, rd, symbol, link))
        self.asm.append(anchor + ":")
        self.word(0, f"auipc x{rd},%pcrel_hi({symbol})")
        tail = (f"addi x{rd},x{rd},%pcrel_lo({anchor})" if link is None
                else f"jalr x{link},%pcrel_lo({anchor})(x{rd})")
        self.word(0, tail)

    def jump_if_ne(self, rs, rt, label):
        self.word(branch_eq(rs, rt, 12), f"beq x{rs},x{rt},.+12")
        self.address(5, label, link=0)  # x5 is the reserved expansion scratch.

    def ecall(self):
        self.word(0x73, "ecall")

    def exit(self, status):
        self.addi(10, 0, status)
        self.addi(17, 0, 93)
        self.ecall()


def make_image():
    c = Code()
    c.label("_start")
    c.address(5, "far_function", link=1)
    c.addi(6, 0, 42)
    c.jump_if_ne(10, 6, "failed")
    c.address(5, "global_pointer")
    c.ld(6, 5)
    c.address(7, "message")
    c.jump_if_ne(6, 7, "failed")
    c.ld(6, 5, 8)
    c.addi(7, 0, 123)
    c.jump_if_ne(6, 7, "failed")
    for label in ["bss_begin", "bss_last"]:
        c.address(5, label)
        c.ld(6, 5)
        c.jump_if_ne(6, 0, "failed")
    c.addi(6, 0, 1)
    c.jump_if_ne(6, 0, "passed")  # Taken branch beyond the B-type range.
    c.exit(99)
    c.label("failed")
    c.exit(77)
    c.label("return_42")
    c.addi(10, 0, 42)
    c.word(iword(0x67, 0, 0, 1, 0), "jalr x0,0(x1)")
    for _ in range(1300):
        c.addi(0, 0, 0)
    c.label("far_function")
    c.address(5, "return_42", link=0)  # Negative PC-relative displacement.
    c.label("passed")
    c.addi(10, 0, 1)
    c.address(11, "message")
    c.addi(12, 0, len(MESSAGE))
    c.addi(17, 0, 64)
    c.ecall()
    c.exit(0)

    rodata = bytes(PREFIX) + MESSAGE
    ro_va = align(TEXT + len(c.words) * 4)
    data_va = align(ro_va + len(rodata))
    symbols = {name: TEXT + off for name, off in c.labels.items()}
    symbols.update(message=ro_va + PREFIX, global_pointer=data_va,
                   bss_begin=data_va + 16, bss_last=data_va + 16 + PAGE - 8)
    displacements = []
    for at, rd, name, link in c.fixups:
        c.words[at // 4:at // 4 + 2] = pcrel_words(rd, symbols[name], TEXT + at, link)
        displacements.append(symbols[name] - (TEXT + at))
    text = struct.pack(f"<{len(c.words)}I", *c.words)
    data = struct.pack("<QQ", symbols["message"], 123)  # An ABS64 symbol fixup.

    segments = []
    file_end = PAGE
    for payload, va, memory, flags in [(text, TEXT, len(text), 5),
                                      (rodata, ro_va, len(rodata), 4),
                                      (data, data_va, len(data) + PAGE, 6)]:
        off = align(file_end)
        segments.append((off, va, len(payload), memory, flags, payload))
        file_end = off + len(payload)
    image = bytearray(file_end)
    ident = b"\x7fELF\x02\x01\x01" + bytes(9)
    struct.pack_into("<16sHHIQQQIHHHHHH", image, 0, ident, 2, 243, 1,
                     TEXT, 64, 0, 0, 64, 56, 3, 0, 0, 0)
    for index, (off, va, size, memory, flags, payload) in enumerate(segments):
        struct.pack_into("<IIQQQQQQ", image, 64 + 56 * index,
                         1, flags, off, va, va, size, memory, PAGE)
        image[off:off + size] = payload

    c.asm += [".section .rodata", f".zero {PREFIX}", "message:",
              ".byte " + ",".join(map(str, MESSAGE)), ".data", "global_pointer:",
              ".dword message", ".dword 123", ".bss", "bss_begin:",
              f".zero {PAGE - 8}", "bss_last:", ".zero 8"]
    return bytes(image), "\n".join(c.asm) + "\n", (text, rodata, data), displacements


def oracle_sections(blob):
    """Read only the named section payloads needed from Clang/LLD's oracle ELF."""
    shoff = struct.unpack_from("<Q", blob, 40)[0]
    entsize, count, names_index = struct.unpack_from("<HHH", blob, 58)
    entries = [struct.unpack_from("<IIQQQQIIQQ", blob, shoff + i * entsize)
               for i in range(count)]
    names = entries[names_index]
    strings = blob[names[4]:names[4] + names[5]]
    result = {}
    for entry in entries:
        name = strings[entry[0]:].split(b"\0", 1)[0].decode()
        if name in [".text", ".rodata", ".data"]:
            result[name] = blob[entry[4]:entry[4] + entry[5]]
    return tuple(result[name] for name in [".text", ".rodata", ".data"])


def run(args):
    return subprocess.run(args, check=True, capture_output=True, timeout=20)


def main():
    out = Path(__file__).resolve().parent / "out"
    out.mkdir(exist_ok=True)
    (out / "report.json").unlink(missing_ok=True)
    image, assembly, sections, deltas = make_image()
    # Include the exact high/low rounding edges, including the positive limit.
    for delta in [-0x80000000, -4097, -2049, -2048, -1, 0, 2047, 2048, 4095, 0x7ffff7ff]:
        upper, lower = pcrel_words(5, 0x10000 + delta, 0x10000)
        hi = upper >> 12
        lo = lower >> 20
        hi = hi - (1 << 20) if hi & (1 << 19) else hi
        lo = lo - (1 << 12) if lo & (1 << 11) else lo
        assert hi * PAGE + lo == delta
    for delta in [-0x80000801, 0x7ffff800]:
        try:
            pcrel_words(5, delta, 0)
        except ValueError:
            pass
        else:
            raise AssertionError("out-of-range fixup accepted")
    elf = out / "direct.elf"
    elf.write_bytes(image)
    elf.chmod(0o755)
    observed = run(["qemu-riscv64", str(elf)])
    assert observed.stdout == MESSAGE, observed.stdout
    info = run(["readelf", "-h", "-l", str(elf)]).stdout
    (out / "readelf.txt").write_bytes(info)
    (out / "oracle.S").write_text(assembly)
    (out / "oracle.ld").write_text(
        "ENTRY(_start)\nSECTIONS { . = 0x10000; .text : { *(.text) } "
        ". = ALIGN(4096); .rodata : { *(.rodata) } "
        ". = ALIGN(4096); .data : { *(.data) } .bss : { *(.bss) } }\n")
    run(["clang", "--target=riscv64-unknown-elf", "-march=rv64i", "-mabi=lp64",
         "-mno-relax", "-c", str(out / "oracle.S"), "-o", str(out / "oracle.o")])
    run(["ld.lld", "--no-relax", "-T", str(out / "oracle.ld"),
         str(out / "oracle.o"), "-o", str(out / "oracle.elf")])
    assert sections == oracle_sections((out / "oracle.elf").read_bytes())
    assert min(deltas) < -4096 and max(deltas) > 4096
    report = {"status": "passed", "image_bytes": len(image),
              "section_bytes": dict(zip(["text", "rodata", "data"], map(len, sections))),
              "bss_bytes": PAGE, "pcrel_fixups": len(deltas), "abs64_fixups": 1,
              "min_pcrel": min(deltas), "max_pcrel": max(deltas),
              "clang_lld_section_comparison": "identical",
              "qemu_user_exit": observed.returncode, "stdout": observed.stdout.decode(),
              "sha256": hashlib.sha256(image).hexdigest(),
              "scope": "Host Python emitter, hand-built program, Linux syscall adapter; not self-hosting."}
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
