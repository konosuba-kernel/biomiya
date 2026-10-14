#!/usr/bin/env python3
"""Retained frontend recursion and layout limits must fail before target panic/stack overflow."""

# Assertions are verification checks, including in imported helpers.
if not __debug__:
    raise SystemExit("verification requires assertions; unset PYTHONOPTIMIZE and omit -O/-OO")

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

CORE = Path(__file__).resolve().parents[1]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', type=Path, required=True)
    parser.add_argument('--record-dir', type=Path, required=True)
    args = parser.parse_args()
    out = args.record_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    compiler = out / 'compiler.elf'
    compiler.write_bytes(args.compiler.read_bytes())
    compiler.chmod(0o755)
    qemu = shutil.which('qemu-riscv64')
    assert qemu
    cases = [
        ('expression-depth', 'fn run()->u32{return ' + '(' * 100 + '0' + ')' * 100 + ';}', 'I_LIMIT', True),
        ('block-depth', 'fn run()->u32{' + '{' * 80 + 'return 0;' + '}' * 80 + '}', 'I_LIMIT', True),
        ('pointer-depth', 'static A:' + '*' * 100 + 'u8=null;fn run()->u32{return 0;}', 'I_LIMIT', True),
        ('type-chain', ''.join(f'struct A{i}{{x:A{i+1}}}' for i in range(40)) + 'struct A40{x:u8}fn run()->u32{return 0;}', 'I_LIMIT', True),
        ('array-count', 'static mut A:[4294967296_usize]u8=zero;fn run()->u32{return 0;}', 'actual=4294967296 limit=4294967295 I_LIMIT', False),
        ('repeat-count', 'static A:[1_usize]u8=[0;4294967296_usize];fn run()->u32{return 0;}', 'actual=4294967296 limit=4294967295 I_LIMIT', False),
        ('zero-repeat-type', 'static A:[0_usize]u32=["x";0_usize];fn run()->u32{return 0;}', 'E_TYPE', False),
        ('zero-repeat-name', 'static A:[0_usize]*const u8=[core::addr_of(missing);0_usize];fn run()->u32{return 0;}', 'E_NAME', False),
        ('mutable-static-permission', 'static mut A:[2_usize]u8=zero;fn run()->u32{return A.len as u32;}', 'E_PERMISSION', False),
    ]
    hierarchy = 'struct A0{' + ''.join(f'f{i}:[67108864_usize]u8,' for i in range(256)) + '}'
    for level in range(1, 6):
        hierarchy += f'struct A{level}{{' + ''.join(f'f{i}:A{level-1},' for i in range(256)) + '}'
    cases.append(('layout-overflow', hierarchy + 'fn run()->u32{return 0;}', 'I_LIMIT', True))
    big = 'struct Big{' + ''.join(f'f{i}:[67108864_usize]u8,' for i in range(256)) + '}'
    cases.append(('array-layout-overflow', big + 'static mut A:[4294967295_usize]Big=zero;fn run()->u32{return 0;}', 'I_LIMIT', True))
    for name, body in [
        ('field-chain', 'fn run()->u32{var a:u32=0;return a' + '.x' * 3000 + ';}'),
        ('index-chain', 'fn run()->u32{var a:u32=0;return a' + '[0]' * 3000 + ';}'),
        ('else-chain', 'fn run()->u32{' + 'if false {} else ' * 3000 + '{}return 0;}'),
        ('constant-chain', 'static N:u32=' + '1_u32+' * 3000 + '1_u32;fn run()->u32{return 0;}'),
    ]:
        cases.append((name, body, ('I_LIMIT', 'E_TYPE'), False))
    inputs = {'tools/check_q_limits.py': sha(Path(__file__).read_bytes())}
    (out / 'tool.py').write_bytes(Path(__file__).read_bytes())
    report = {'compiler_sha256': sha(compiler.read_bytes()), 'inputs': inputs,
              'guest_stack_bytes': 1048576, 'cases': [], 'passed': False,
              'identities': ['frontend_depth_limits_and_type_shadowing_fail_before_guest_stack_overflow',
                             'frontend_reports_permission_and_capacity_errors_without_target_panics']}
    try:
        for name, body, diagnostic, bounded in cases:
            work = out / name
            work.mkdir()
            source = 'module app;' + body
            (work / 'main.vt').write_text(source)
            (work / 'case.build').write_text('format 1\nprofile linux-user\nentry app::run\nsource main.vt\noutput out.elf\n')
            command = [qemu, '-s', '1048576', str(compiler), 'build', 'case.build']
            proc = subprocess.run(command, cwd=work, env={'PATH': ''}, capture_output=True, text=True, timeout=120)
            item = {'name': name, 'command': command, 'exit': proc.returncode,
                    'stdout': proc.stdout, 'stderr': proc.stderr, 'source_sha256': sha(source.encode()), 'passed': False}
            report['cases'].append(item)
            diagnostics = diagnostic if isinstance(diagnostic, tuple) else (diagnostic,)
            assert proc.returncode == 1 and any(code in proc.stderr for code in diagnostics), item
            if bounded:
                assert all(text in proc.stderr for text in ['actual=', 'limit=', 'file=1 byte=']), item
            assert not (work / 'out.elf').exists()
            item['passed'] = True
        report['passed'] = True
    finally:
        (out / 'verification.json').write_text(json.dumps(report, indent=2) + '\n')
    print('Frontend limit scenarios passed:', len(report['cases']))


if __name__ == '__main__':
    main()
