#!/usr/bin/env python3
"""Exercise retained manifest, source diagnostic, constant and publication contracts."""

# Assertions are verification checks, including in imported helpers.
if not __debug__:
    raise SystemExit("verification requires assertions; unset PYTHONOPTIMIZE and omit -O/-OO")

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess


def sha(data):
    return hashlib.sha256(data).hexdigest()


BASE = 'format 1\nprofile linux-user\nentry app::run\nsource main.vt\noutput out.elf\n'
MAIN = 'module app;fn run()->u32{return 42;}'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', type=Path, required=True)
    parser.add_argument('--record-dir', type=Path, required=True)
    parser.add_argument('--baseline-compiler', type=Path, help='optional accepted 32-source negative control')
    args = parser.parse_args()
    out = args.record_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    compiler = out / 'compiler.elf'
    compiler.write_bytes(args.compiler.read_bytes())
    compiler.chmod(0o755)
    baseline = None
    if args.baseline_compiler:
        baseline = out / 'baseline.elf'
        baseline.write_bytes(args.baseline_compiler.read_bytes())
        baseline.chmod(0o755)
    qemu = shutil.which('qemu-riscv64')
    assert qemu
    (out / 'tool.py').write_bytes(Path(__file__).read_bytes())
    report = {'compiler_sha256': sha(compiler.read_bytes()), 'tool_sha256': sha(Path(__file__).read_bytes()),
              'guest_stack_bytes': 1048576, 'cases': [], 'passed': False}

    def invoke(directory, arguments):
        command = [qemu, '-s', '1048576', *map(str, arguments)]
        p = subprocess.run(command, cwd=directory, env={'PATH': ''}, capture_output=True, text=True, timeout=120)
        return {'command': command, 'exit': p.returncode, 'stdout': p.stdout, 'stderr': p.stderr}

    def case(name, files=None, manifest=BASE, error=None, run_exit=None, existing=False, driver=None):
        driver = driver or compiler
        directory = out / name
        directory.mkdir()
        for filename, value in (files or {'main.vt': MAIN}).items():
            path = directory / filename
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(value if isinstance(value, bytes) else value.encode())
        (directory / 'project').write_text(manifest)
        if existing:
            (directory / 'out.elf').write_bytes(b'keep old artifact')
        before = {str(p.relative_to(directory)): p.read_bytes() for p in directory.rglob('*') if p.is_file()}
        result = {'name': name, 'compiler_sha256': sha(driver.read_bytes()), 'commands': [], 'inputs': {n: sha(v) for n, v in before.items()}, 'passed': False}
        report['cases'].append(result)
        mode = 'build' if error or existing or run_exit is not None else 'check'
        actual = invoke(directory, [driver, mode, 'project'])
        result['commands'].append(actual)
        assert actual['exit'] == (1 if error else 0), result
        if error:
            assert error in actual['stderr'], result
        if error or mode == 'check':
            assert {str(p.relative_to(directory)): p.read_bytes() for p in directory.rglob('*') if p.is_file()} == before, result
        else:
            for name, data in before.items():
                assert (directory / name).read_bytes() == data
        if run_exit is not None:
            execution = invoke(directory, [directory / 'out.elf'])
            result['commands'].append(execution)
            assert execution['exit'] == run_exit and execution['stdout'] == '', result
        result['passed'] = True

    try:
        case('ordinary-builtin-name', {'main.vt': 'module app;fn addr_of()->u32{return 42;}fn run()->u32{return addr_of();}'}, run_exit=42)
        case('independent-type-namespace', {'main.vt': 'module app;struct A{x:u32}fn run()->u32{let A:u32=7;let x:A=make A{x:A};return x.x;}'}, run_exit=7)
        for name, declaration in [('extern-slice', 'extern abi(C) fn os(s:[]const u8)->u32;'),
                                  ('extern-aggregate', 'extern abi(C) fn os()->[2_usize]u8;'),
                                  ('extern-unit', 'extern abi(C) fn os(x:())->u32;')]:
            case(name, {'main.vt': 'module app;' + declaration + 'fn run()->u32{return 0;}'}, error='I_UNSUPPORTED')

        exact = [
            ('name', 'fn run()->u32{return missing;}', 'E_NAME', 'missing'),
            ('type', 'fn run()->u32{return true;}', 'E_TYPE', 'true'),
            ('permission', 'fn run()->u32{let n:u32=0;return *core::addr_of(n);}', 'E_PERMISSION', '*core'),
            ('initializer', 'static bad:[2_usize]u32=[1_u32,true];fn run()->u32{return 0;}', 'E_TYPE', 'true'),
            ('forward-const', 'const A:u32=B;const B:bool=true;fn run()->u32{return 0;}', 'E_TYPE', 'B;'),
            ('recursive-header', 'struct A{b:B} struct B{a:A} fn run()->u32{return 0;}', 'E_TYPE', 'A} fn'),
            ('recursive-array-layout', 'struct Outer{bad:[67108865_usize]Inner}struct Inner{v:u8}fn run()->u32{return 0;}', 'I_LIMIT', '[67108865'),
        ]
        for name, body, diagnostic, marker in exact:
            source = 'module app;' + body
            case('span-' + name, {'prefix.vt': 'module prefix;pub const N:u32=0;', 'main.vt': source},
                 BASE.replace('source main.vt', 'source prefix.vt\nsource main.vt'),
                 error=f'{diagnostic} file=2 byte={source.index(marker)}\n')

        values = [('u8', '255_u8+%1_u8', '0'), ('u8', '~0_u8', '255'), ('u8', '-0_u8', '0'),
                  ('u8', '256_u32 as u8', '0'), ('u8', '1_u8<<7_u8', '128'),
                  ('bool', '1_u8==1_u8', 'true'), ('bool', '!false', 'true'),
                  ('usize', '18446744073709551615_usize+%1_usize', '0')]
        for index, (ty, expression, expected) in enumerate(values):
            source = f'module app;const C:{ty}={expression};static S:{ty}={expression};fn run()->u32{{if C!={expected} || S!={expected}{{return 1;}}return 0;}}'
            case('constant-value-' + str(index), {'main.vt': source}, run_exit=0)
        invalid = [('u8', '255_u8+1_u8'), ('u8', '-1_u8'), ('u8', '1_u8<<8_u8'),
                   ('u8', '1_u8/0_u8'), ('u8', 'true as u8'), ('usize', '18446744073709551615_usize+1_usize')]
        for index, (ty, expression) in enumerate(invalid):
            for form in ['const', 'static']:
                case(f'constant-invalid-{index}-{form}', {'main.vt': f'module app;{form} C:{ty}={expression};fn run()->u32{{return 0;}}'}, error='E_TYPE')

        failures = [('parse', 'fn run()->u32{return ;}', 'E_PARSE file=2'),
                    ('name', 'fn run()->u32{return unknown;}', 'E_NAME file=2'),
                    ('type', 'fn run()->u32{return false;}', 'E_TYPE file=2'),
                    ('entry-missing', 'fn other()->u32{return 0;}', 'E_NAME file=0'),
                    ('entry-args', 'fn run(x:u32)->u32{return x;}', 'E_TYPE file=2'),
                    ('entry-unsafe', 'unsafe fn run()->u32{return 0;}', 'E_TYPE file=2'),
                    ('entry-result', 'fn run()->bool{return false;}', 'E_TYPE file=2'),
                    ('link', 'extern abi(C) fn missing()->u32;fn run()->u32{return 0;}', 'E_LINK')]
        for name, source, diagnostic in failures:
            case('existing-' + name, {'prefix.vt': 'module prefix;', 'main.vt': 'module app;' + source},
                 BASE.replace('source main.vt', 'source prefix.vt\nsource main.vt'), error=diagnostic, existing=True)
        case('existing-invalid-utf8', {'main.vt': b'\xff'}, error='E_PARSE file=1 byte=0', existing=True)

        for index, path in enumerate(['/etc/passwd', 'a//b', 'a/./b', 'a/../b', 'a/', '../a', 'a\\b', 'a b', 'a\0b',
                                      'a' * 97, '源' * 33, 'a\u00a0b', 'a\u3000b']):
            case('path-invalid-' + str(index), manifest=BASE + 'source ' + path + '\n', error='E_PARSE file=0')
        case('path-96', {'main.vt': MAIN, 'a' * 96: 'module long;'}, BASE + 'source ' + 'a' * 96 + '\n')
        case('unicode-path', {'main.vt': MAIN, '源码/二.vt': 'module extra;'}, BASE + 'source 源码/二.vt\n')
        for index, extra in enumerate(['source main.vt', 'assembly main.vt', 'entry foo', 'format 1',
                                      'profile linux-user', 'output foo', 'unknown bar', 'source', 'source a b']):
            case('directive-invalid-' + str(index), manifest=BASE + extra + '\n', error='E_PARSE file=0')
        for field in ['format', 'profile', 'entry', 'source', 'output']:
            case('missing-' + field, manifest='\n'.join(line for line in BASE.splitlines() if not line.startswith(field)) + '\n', error='E_PARSE file=0')
        case('profile-invalid', manifest=BASE.replace('linux-user', 'sos'), error='I_UNSUPPORTED file=0')
        manifest = BASE
        files = {'main.vt': MAIN}
        for i in range(1, 64):
            files[f's{i}.vt'] = f'module extra{i};'
            manifest += f'source s{i}.vt\n'
        for i in range(8):
            files[f'a{i}.s0'] = '# empty adapter\n'
            manifest += f'assembly a{i}.s0\n'
        case('input-capacities-exact', files, manifest, run_exit=42)
        case('source-capacity-over', files, manifest + 'source overflow.vt\n',
             error=f'actual=65 limit=64 I_LIMIT file=0 byte={len(manifest)}\n')
        case('assembly-capacity-over', files, manifest + 'assembly overflow.s0\n', error='I_LIMIT')
        # Old boundary, first newly supported file, and the last source all run.
        for count in [32, 33, 64]:
            files = {'main.vt': 'module app;import tail;fn run()->u32{return tail::answer();}'}
            manifest = BASE
            for i in range(1, count):
                files[f's{i}.vt'] = ('module tail;pub fn answer()->u32{return 42;}'
                                     if i == count-1 else f'module extra{i};')
                manifest += f'source s{i}.vt\n'
            case(f'sources-{count}-last-entry', files, manifest, run_exit=42)
            if count == 33 and baseline:
                offset = manifest.index('source s32.vt')
                case('source-33-old-capacity', files, manifest,
                     error=f'actual=33 limit=32 I_LIMIT file=0 byte={offset}\n', driver=baseline)
        tail = 'module tail;pub fn answer()->u32{return missing;}'
        invalid = {**files, 's63.vt': tail}
        case('source-64-diagnostic', invalid, manifest,
             error=f'E_NAME file=64 byte={tail.index("missing")}\n')
        # Complete assembly numbering and file-local label namespaces at 65..72.
        files['s63.vt'] = 'module tail;extern abi(C) fn asm7()->u32;pub fn answer()->u32{unsafe{return asm7();}}'
        for i in range(8):
            files[f'a{i}.s0'] = f'asm{i}:\njump .Ldone\n.Ldone:\naddi x10, x0, 42\njalr x0, 0(x1)\n'
            manifest += f'assembly a{i}.s0\n'
        case('sources-64-assembly-8-last-call', files, manifest, run_exit=42)
        invalid = {**files, 'a7.s0': 'invalid_opcode\n'}
        case('assembly-72-diagnostic', invalid, manifest, error='I_UNSUPPORTED file=72 byte=0\n')
        # Traverse all source-indexed semantic states, not just empty headers.
        chain = {}
        manifest = BASE.replace('source main.vt\n', '')
        for i in range(64):
            name = 'app' if i == 0 else f'm{i}'
            body = f'import m{i+1};pub const N:u32=m{i+1}::N+1_u32;' if i < 63 else 'pub const N:u32=0;'
            chain[f'm{i}.vt'] = f'module {name};' + body + ('fn run()->u32{return N;}' if i == 0 else '')
            manifest += f'source m{i}.vt\n'
        case('modules-64-chain', chain, manifest, run_exit=63)
        cycle = {**chain, 'm63.vt': 'module m63;import app;pub const N:u32=0;'}
        case('modules-64-cycle', cycle, manifest, error='E_NAME file=64')
        shared = {'m0.vt': 'module app;fn run()->u32{return last();}'}
        shared.update({f'm{i}.vt': 'module app;' for i in range(1, 63)})
        shared['m63.vt'] = 'module app;fn last()->u32{return 42;}'
        case('same-module-64-files', shared, manifest, run_exit=42)
        manifest = BASE + '#' + 'x' * (16384-len(BASE)-1)
        case('manifest-exact', manifest=manifest)
        case('manifest-over', manifest=manifest + 'x', error='I_LIMIT')
        report['passed'] = True
    finally:
        (out / 'verification.json').write_text(json.dumps(report, indent=2) + '\n')
    print('Interface scenarios passed:', len(report['cases']))


if __name__ == '__main__':
    main()
