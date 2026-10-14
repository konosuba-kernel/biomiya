#!/usr/bin/env python3
"""Ensure target faults and failures cannot masquerade as successful exit."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

HERE = Path(__file__).resolve().parent
CORE = HERE.parents[1]
CASES = {
    'reserved-slot': ('p5_install(64, pte)', 'p5_install(0, pte)', 250),
    'invalid-flags': ('p5_install(64, pte)', 'p5_install(64, pte | 8_usize)', 250),
    'misaligned-page': ('p5_install(64, pte)', 'p5_install(64, pte | 1024_usize)', 250),
    'reserved-high-bit': ('p5_install(64, pte)', 'p5_install(64, pte | 0x8000000000000000_usize)', 250),
    'reserved-low-bit': ('p5_install(64, pte)', 'p5_install(64, pte | 0x0040000000000000_usize)', 250),
    'ppn-high-bit': ('p5_install(64, pte)', 'p5_install(64, pte | 0x0020000000000000_usize)', 250),
    'below-ram': ('p5_install(64, pte)', 'p5_install(64, ((0x84e00000_usize >> 12_usize) << 10_usize) | 199_usize)', 250),
    'ram-upper-bound': ('p5_install(64, pte)', 'p5_install(64, ((0x87800000_usize >> 12_usize) << 10_usize) | 199_usize)', 250),
    'unmapped-read': ('fn run() -> u32 {', 'fn run() -> u32 { unsafe { let value:usize = *core::ptr::<usize>(0x08000000); if value == 0_usize { return 77; } }', 250),
    'explicit-failure': ('fn run() -> u32 {', 'fn run() -> u32 { return 7;', 7),
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    compiler = args.compiler.resolve()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    qemu = shutil.which('qemu-riscv64')
    probe = (HERE/'probe.vt').read_text()
    record = dict(accepted=False, compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(), cases=[])
    try:
        for name, (old, new, exit_code) in CASES.items():
            case = out/name
            case.mkdir()
            relative = case.relative_to(CORE)
            source = case/'probe.vt'
            if probe.count(old) != 1:
                raise RuntimeError(f'probe edit is no longer unique: {old}')
            source.write_text(probe.replace(old, new))
            (case/'probe.build').write_text(f'format 1\nprofile linux-user\nentry system::probe::run\nsource {relative}/probe.vt\nassembly experiments/system/machine.s0\noutput {relative}/probe.elf\n')
            command = [qemu, '-s', '1048576', str(compiler), 'build', str(relative/'probe.build')]
            result = subprocess.run(command, cwd=CORE, env={**os.environ, 'PATH': ''}, capture_output=True, text=True, timeout=60)
            (case/'compile.stdout').write_text(result.stdout)
            (case/'compile.stderr').write_text(result.stderr)
            if result.returncode:
                raise RuntimeError(result.stderr)
            subprocess.run([sys.executable, str(HERE/'run_system.py'), str(case/'probe.elf'), '--out', str(case/'machine'), '--expect', '', '--expect-exit', str(exit_code)], cwd=CORE, check=True, timeout=45)
            record['cases'].append(dict(name=name, source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(), expected_exit=exit_code, compile_command=command))
        record['accepted'] = True
    finally:
        (out/'verification.json').write_text(json.dumps(record, indent=2)+'\n')
    print(out/'verification.json')


if __name__ == '__main__':
    main()
