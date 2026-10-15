#!/usr/bin/env python3
"""Compile real model diagnostics against a replacement host, with a legacy control."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

CORE = Path(__file__).resolve().parents[1]


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', type=Path, required=True)
    parser.add_argument('--record-dir', type=Path, required=True)
    parser.add_argument('--source-root', type=Path, default=CORE)
    parser.add_argument('--baseline-model', type=Path, required=True,
                        help='pre-host model source for a discriminating negative control')
    args = parser.parse_args()
    qemu = shutil.which('qemu-riscv64')
    require(qemu is not None, 'qemu-riscv64 is required')
    record = args.record_dir.resolve()
    record.mkdir(parents=True, exist_ok=False)
    inputs = {'model.vt': (args.source_root / 'compiler/model.vt').read_bytes(),
              'legacy.vt': args.baseline_model.read_bytes(),
              'host.vt': (CORE / 'tests/compiler/k0-host-replacement.vt').read_bytes(),
              'probe.vt': (CORE / 'tests/compiler/k0-host-probe.vt').read_bytes(),
              'trap.s0': (CORE / 'tests/compiler/k0-host-trap.s0').read_bytes(),
              'compiler.elf': args.compiler.read_bytes(), 'tool.py': Path(__file__).read_bytes()}
    require(inputs['model.vt'] != inputs['legacy.vt'], 'control must use distinct pre-host source')
    for name, data in inputs.items():
        (record / name).write_bytes(data)
    compiler = record / 'compiler.elf'
    compiler.chmod(0o755)
    report = {'input_sha256': {name: digest(data) for name, data in inputs.items()},
              'cases': [], 'passed': False}

    def run(command, cwd):
        process = subprocess.run(command, cwd=cwd, env={'PATH': ''}, capture_output=True,
                                 text=True, timeout=180)
        return {'command': list(map(str, command)), 'exit': process.returncode,
                'stdout': process.stdout, 'stderr': process.stderr}

    try:
        for name, source, entry, expected in (
                ('capture', 'model.vt', 'capture', 77),
                ('sink-failure', 'model.vt', 'silent', 79),
                ('legacy-bypass-control', 'legacy.vt', 'capture', 85)):
            directory = record / name
            directory.mkdir()
            for target, origin in [('model.vt', source), ('host.vt', 'host.vt'),
                                   ('probe.vt', 'probe.vt'), ('trap.s0', 'trap.s0')]:
                (directory / target).write_bytes(inputs[origin])
            (directory / 'probe.build').write_text(
                f'format 1\nprofile linux-user\nentry k0_host::{entry}\n'
                'source model.vt\nsource host.vt\nsource probe.vt\nassembly trap.s0\n'
                'output probe.elf\n')
            case = {'name': name, 'expected_exit': expected, 'passed': False}
            report['cases'].append(case)
            case['build'] = run([qemu, '-s', '1048576', str(compiler), 'build', 'probe.build'], directory)
            require(case['build']['exit'] == 0, f'{name}: build failed')
            image = directory / 'probe.elf'
            case['elf_sha256'] = digest(image.read_bytes())
            case['run'] = run([qemu, '-s', '1048576', str(image)], directory)
            require(case['run']['exit'] == expected, f'{name}: wrong exit, host bypass or mismatch')
            require(case['run']['stdout'] == '' and case['run']['stderr'] == '',
                    f'{name}: diagnostic escaped replacement host')
            case['passed'] = True
        report['passed'] = True
    except Exception as error:
        report['error'] = str(error)
        raise
    finally:
        (record / 'verification.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'passed': True, 'cases': len(report['cases']), 'record': str(record)}))


if __name__ == '__main__':
    main()
