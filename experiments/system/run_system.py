#!/usr/bin/env python3
"""Run an unchanged Core S ELF on two QEMU harts behind an Sv39 mapping.

The host builds only the counted machine monitor. It never compiles guest code.
Retain this tool's JSON record together with the identified guest build log.
"""
import argparse
import hashlib
import json
import pathlib
import shutil
import struct
import subprocess
import time

HERE = pathlib.Path(__file__).resolve().parent


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def stage_guest(elf, output):
    data = elf.read_bytes()
    if data[:7] != b'\x7fELF\x02\x01\x01' or struct.unpack_from('<H', data, 18)[0] != 243:
        raise ValueError('expected little-endian RISC-V ELF64')
    entry, phoff = struct.unpack_from('<QQ', data, 24)
    phsize, phcount = struct.unpack_from('<HH', data, 54)
    if phsize != 56 or phcount > 16 or phoff + phsize*phcount > len(data):
        raise ValueError('invalid program header table')
    segments = []
    for i in range(phcount):
        kind, flags, off, va, pa, filesz, memsz, align = struct.unpack_from('<IIQQQQQQ', data, phoff+i*phsize)
        if kind != 1:
            continue
        if filesz > memsz or off+filesz > len(data) or va < 0x10000 or va+memsz > 0x4000000:
            raise ValueError('segment outside the supported low 64 MiB guest window')
        if any(va < b['virtual']+b['memory_bytes'] and b['virtual'] < va+memsz for b in segments):
            raise ValueError('overlapping load segments')
        segments.append(dict(virtual=va, file_offset=off, file_bytes=filesz, memory_bytes=memsz, flags=flags))
    if not any(s['virtual'] <= entry < s['virtual']+s['file_bytes'] and s['flags'] & 1 for s in segments):
        raise ValueError('entry outside executable bytes')
    ram = bytearray(max(s['virtual']+s['memory_bytes'] for s in segments))
    for s in segments:
        ram[s['virtual']:s['virtual']+s['file_bytes']] = data[s['file_offset']:s['file_offset']+s['file_bytes']]
    (output/'guest.bin').write_bytes(ram)
    (output/'guest.inc').write_text(f'.equ GUEST_ENTRY, {entry:#x}\n')
    return dict(entry=entry, physical_bias=0x81000000, segments=segments, elf_sha256=digest(elf), staged_sha256=digest(output/'guest.bin'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('elf', type=pathlib.Path)
    parser.add_argument('--out', type=pathlib.Path, required=True)
    parser.add_argument('--expect', default='P5 OK\n')
    parser.add_argument('--timeout', type=int, default=30)
    parser.add_argument('--expect-exit', type=int, default=0)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    record = dict(status='preliminary', accepted=False, expected_exit=args.expect_exit, expected_stdout=args.expect, host_tools={}, commands=[])
    for name in ('clang', 'ld.lld', 'qemu-system-riscv64'):
        path = shutil.which(name)
        if not path:
            raise RuntimeError(f'missing tool: {name}')
        record['host_tools'][name] = dict(path=path, version=subprocess.check_output([path, '--version'], text=True).splitlines()[0])
    record['guest'] = stage_guest(args.elf.resolve(), out)
    record['platform_sources'] = {p.name: digest(p) for p in (HERE/'monitor.S', HERE/'monitor.ld', HERE/'machine.s0', pathlib.Path(__file__))}
    commands = [
        [shutil.which('clang'), '--target=riscv64-unknown-elf', '-march=rv64ima_zicsr', '-mabi=lp64', '-mno-relax', '-I', str(out), '-c', str(HERE/'monitor.S'), '-o', str(out/'monitor.o')],
        [shutil.which('ld.lld'), '--no-relax', '-T', str(HERE/'monitor.ld'), str(out/'monitor.o'), '-o', str(out/'monitor.elf')],
        [shutil.which('qemu-system-riscv64'), '-machine', 'virt', '-cpu', 'rv64', '-m', '128M', '-smp', '2', '-bios', 'none', '-kernel', str(out/'monitor.elf'), '-device', f'loader,file={out}/guest.bin,addr=0x81000000,force-raw=on', '-nographic', '-monitor', 'none', '-serial', 'stdio'],
    ]
    try:
        for index, command in enumerate(commands):
            start = time.monotonic()
            result = subprocess.run(command, text=True, capture_output=True, timeout=args.timeout)
            record['commands'].append(dict(argv=command, exit=result.returncode, stdout=result.stdout, stderr=result.stderr, seconds=time.monotonic()-start))
            (out/f'command-{index}.stdout').write_text(result.stdout)
            (out/f'command-{index}.stderr').write_text(result.stderr)
            expected_exit = args.expect_exit if index == 2 else 0
            if result.returncode != expected_exit:
                raise RuntimeError(f'command {index} failed with {result.returncode}')
        if result.stdout != args.expect:
            raise RuntimeError(f'output mismatch: {result.stdout!r}')
        record['accepted'] = True
        record['monitor_sha256'] = digest(out/'monitor.elf')
    finally:
        (out/'verification.json').write_text(json.dumps(record, indent=2)+'\n')
    print(out/'verification.json')


if __name__ == '__main__':
    main()
