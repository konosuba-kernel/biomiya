#!/usr/bin/env python3
"""Compile both system slices with an identified target compiler, then execute.

This produces candidate-specific system evidence. P5 acceptance additionally
requires the caller to tie this compiler hash to accepted P4 fixed-point proof.
"""
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


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    compiler = args.compiler.resolve()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    relative = out.relative_to(CORE)
    qemu = shutil.which('qemu-riscv64')
    if qemu is None:
        raise RuntimeError('qemu-riscv64 unavailable')
    record = dict(accepted=False, requires_p4_proof=True, compiler_sha256=sha(compiler),
                  sources={str(p.relative_to(CORE)): sha(p) for p in sorted(HERE.iterdir()) if p.is_file()}, commands=[])

    def run(command, expected=0, env=None):
        result = subprocess.run(command, cwd=CORE, env=env, text=True, capture_output=True, timeout=60)
        record['commands'].append(dict(argv=command, exit=result.returncode, stdout=result.stdout, stderr=result.stderr))
        if result.returncode != expected:
            raise RuntimeError(f'unexpected exit {result.returncode}: {result.stderr}')
        return result

    try:
        for name in ('system', 'file_rollback'):
            lines = (HERE/f'{name}.build').read_text().splitlines()
            lines = [line for line in lines if not line.startswith('output ')]
            lines.append(f'output {relative}/{name}.elf')
            (out/f'{name}.build').write_text('\n'.join(lines)+'\n')
            run([qemu, '-s', '1048576', str(compiler), 'build', str(relative/f'{name}.build')], env={**os.environ, 'PATH': ''})
            record[f'{name}_sha256'] = sha(out/f'{name}.elf')
        result = run([qemu, '-s', '1048576', str(out/'file_rollback.elf')], env={**os.environ, 'PATH': ''})
        if result.stdout:
            raise RuntimeError('unexpected file rollback stdout')
        run([sys.executable, str(HERE/'run_system.py'), str(out/'system.elf'), '--out', str(out/'machine')])
        machine = json.loads((out/'machine/verification.json').read_text())
        run([sys.executable, str(HERE/'check_monitor.py'), '--compiler', str(compiler), '--out', str(out/'negative')])
        if not machine['accepted']:
            raise RuntimeError('machine checks failed')
        if record['sources'] != {str(p.relative_to(CORE)): sha(p) for p in sorted(HERE.iterdir()) if p.is_file()}:
            raise RuntimeError('system sources changed while tests ran')
        if record['compiler_sha256'] != sha(compiler):
            raise RuntimeError('compiler changed while tests ran')
        record['accepted'] = True
    finally:
        (out/'verification.json').write_text(json.dumps(record, indent=2)+'\n')
    print(out/'verification.json')


if __name__ == '__main__':
    main()
