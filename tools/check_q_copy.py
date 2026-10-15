#!/usr/bin/env python3
"""Run a real Q manifest/lexer/copy application, including explicit syscall fault injection."""

# Assertions are verification checks, including in imported helpers.
if not __debug__:
    raise SystemExit("verification requires assertions; unset PYTHONOPTIMIZE and omit -O/-OO")

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess

CORE = Path(__file__).resolve().parents[1]
SOURCES = ['compiler/model.vt', 'compiler/target.vt', 'compiler/runtime/host_linux.vt', 'compiler/input.vt', 'compiler/lex.vt', 'compiler/runtime/linux.vt', 'compiler/runtime/bytes.vt', 'tests/compiler/copy.vt']


def sha(data):
    return hashlib.sha256(data).hexdigest()


def instrument(source, mode):
    # Only the syscall boundary is replaced in a saved test copy. The production
    # read/retry/write/close implementations below it remain byte-for-byte intact.
    original = 'extern abi(C) fn __b2_syscall6(n:usize,a:usize,b:usize,c:usize,d:usize,e:usize,f:usize) -> usize;'
    assert source.count(original) == 1
    wrapper = '''extern abi(C) fn __test_real_syscall6(n:usize,a:usize,b:usize,c:usize,d:usize,e:usize,f:usize) -> usize;
static mut test_opened:usize = 0;
static mut test_output:usize = 0xffffffffffffffff;
static mut test_read_retry:bool = false;
static mut test_write_retry:bool = false;
fn test_event(message:[]const u8) -> () {
    unsafe { let ignored:usize = __test_real_syscall6(64,2,core::addr(message.data),message.len,0,0,0); }
    return ();
}
fn __b2_syscall6(n:usize,a:usize,b:usize,c:usize,d:usize,e:usize,f:usize) -> usize {
    unsafe {
        var amount:usize = c;
        INJECTION
        let result:usize = __test_real_syscall6(n,a,b,amount,d,e,f);
        if n == 56_usize && c == 0_usize { test_opened = test_opened+1_usize; }
        if n == 56_usize && c == 193_usize { test_output = result; }
        AFTER
        return result;
    }
}
'''
    injection, after = '', ''
    if mode == 'short-eintr':
        injection = '''if n == 63_usize && !test_read_retry { test_read_retry = true; test_event("INJECT read EINTR\\n"); return 0xfffffffffffffffc; }
        if n == 64_usize && a == test_output && !test_write_retry { test_write_retry = true; test_event("INJECT write EINTR\\n"); return 0xfffffffffffffffc; }
        if (n == 63_usize || n == 64_usize && a == test_output) && amount > 7_usize { amount = 7; }'''
    elif mode == 'read-error':
        injection = 'if n == 63_usize && test_opened == 2_usize { test_event("INJECT read EIO\\n"); return 0xfffffffffffffffb; }'
    elif mode == 'zero-write':
        injection = 'if n == 64_usize && a == test_output { test_event("INJECT zero write\\n"); return 0; }'
    elif mode == 'write-error':
        injection = 'if n == 64_usize && a == test_output { test_event("INJECT write EIO\\n"); return 0xfffffffffffffffb; }'
    elif mode == 'close-error':
        after = 'if n == 57_usize && a == test_output { test_event("INJECT close EIO\\n"); return 0xfffffffffffffffb; }'
    else:
        raise ValueError(mode)
    return source.replace(original, wrapper.replace('INJECTION', injection).replace('AFTER', after))


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
    inputs = {name: (CORE / name).read_bytes() for name in SOURCES + ['compiler/runtime/linux.s0']}
    inputs['tools/check_q_copy.py'] = Path(__file__).read_bytes()
    for name, data in inputs.items():
        destination = out / 'inputs' / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)
    report = {'compiler_sha256': sha(compiler.read_bytes()), 'inputs': {n: sha(d) for n, d in inputs.items()},
              'builds': [], 'cases': [], 'passed': False, 'guest_stack_bytes': 1048576,
              'fault_injection': 'Saved test copy replaces only the Linux syscall boundary; original production files stay unchanged.'}

    def run(directory, arguments, trace=False):
        command = [qemu, '-s', '1048576'] + (['-strace'] if trace else []) + list(map(str, arguments))
        proc = subprocess.run(command, cwd=directory, env={'PATH': ''}, capture_output=True, text=True, timeout=180)
        return {'command': command, 'exit': proc.returncode, 'stdout': proc.stdout, 'stderr': proc.stderr}

    def build(mode):
        work = out / ('build-' + mode)
        work.mkdir()
        manifest = 'format 1\nprofile linux-user\nentry copy_probe::run\n'
        for name in SOURCES:
            data = inputs[name].decode()
            if mode != 'normal' and name == 'compiler/runtime/linux.vt':
                data = instrument(data, mode)
            dest = work / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(data)
            manifest += 'source ' + name + '\n'
        assembly = inputs['compiler/runtime/linux.s0'].decode()
        if mode != 'normal':
            assembly = assembly.replace('__b2_syscall6:', '__test_real_syscall6:\n__b2_syscall6:')
        (work / 'linux.s0').write_text(assembly)
        (work / 'copy.build').write_text(manifest + 'assembly linux.s0\noutput copy.elf\n')
        compiled_inputs = {str(p.relative_to(work)): sha(p.read_bytes()) for p in work.rglob('*') if p.is_file()}
        result = run(work, [compiler, 'build', 'copy.build'])
        report['builds'].append({'mode': mode, **result})
        assert result['exit'] == 0, result
        report['builds'][-1]['elf_sha256'] = sha((work / 'copy.elf').read_bytes())
        assert {str(p.relative_to(work)): sha(p.read_bytes()) for p in work.rglob('*') if p.is_file() and p.name != 'copy.elf'} == compiled_inputs
        report['builds'][-1]['compiled_inputs'] = compiled_inputs
        return work / 'copy.elf'

    def execute(name, image, files, expected=0, existing=False, error=None):
        work = out / name
        work.mkdir()
        manifest = 'format 1\nprofile linux-user\nentry copy_probe::run\n'
        for filename, data in files.items():
            path = work / filename
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            manifest += 'source ' + filename + '\n'
        (work / 'project').write_text(manifest + 'output copied.vt\n')
        if existing:
            (work / 'copied.vt').write_bytes(b'keep existing')
        result = run(work, [image, 'project'], trace=True)
        item = {'name': name, **result, 'passed': False}
        report['cases'].append(item)
        assert result['exit'] == expected, item
        if error:
            assert error in result['stderr'], item
        for filename, data in files.items():
            assert (work / filename).read_bytes() == data
        if existing:
            assert (work / 'copied.vt').read_bytes() == b'keep existing'
            assert 'unlinkat(' not in result['stderr']
        elif expected == 0:
            assert (work / 'copied.vt').read_bytes() == b''.join(files.values())
        else:
            assert not (work / 'copied.vt').exists()
        # Every successfully opened descriptor must be successfully closed.
        live = set()
        opens = closes = 0
        for line in result['stderr'].splitlines():
            opened = re.match(r'\d+ openat\(.*\) = (\d+)', line)
            closed = re.match(r'\d+ close\((\d+)\) = 0', line)
            if opened:
                live.add(int(opened[1])); opens += 1
            if closed:
                assert int(closed[1]) in live, line
                live.remove(int(closed[1])); closes += 1
        assert not live and opens > 0 and opens == closes, item
        if name in ('zero-write', 'write-error', 'close-error'):
            assert 'unlinkat(' in result['stderr'], item
        if name == 'read-error':
            assert 'O_WRONLY' not in result['stderr'], item
        events = {'short-eintr': ['INJECT read EINTR\n', 'INJECT write EINTR\n'],
                  'read-error': ['INJECT read EIO\n'], 'zero-write': ['INJECT zero write\n'],
                  'write-error': ['INJECT write EIO\n'], 'close-error': ['INJECT close EIO\n']}
        for event in events.get(name, []):
            assert result['stderr'].count(event) == 1, item
        item['closed_opened_fds'] = closes
        item['passed'] = True

    try:
        images = {mode: build(mode) for mode in ['normal', 'short-eintr', 'read-error', 'zero-write', 'write-error', 'close-error']}
        long = b'/* nested /* comment */ ok */\n' * 400
        files = {'empty.vt': b'', 'one.vt': long, '源码/二.vt': b'fn f()->u32{return 42;}\n'}
        execute('three-files', images['normal'], files)
        execute('all-empty', images['normal'], {'empty.vt': b'', 'other.vt': b''})
        execute('short-eintr', images['short-eintr'], {'input.vt': b'fn f()->u32{return 42;}\n' * 250})
        execute('existing', images['normal'], files, expected=12, existing=True)
        execute('read-error', images['read-error'], {'input.vt': b'fn f()->u32{return 0;}'}, expected=1, error='E_IO')
        for mode, expected in [('zero-write', 16), ('write-error', 16), ('close-error', 13)]:
            execute(mode, images[mode], {'input.vt': b'fn f()->u32{return 0;}'}, expected=expected)
        report['changed_inputs'] = [n for n, data in inputs.items() if (CORE / n).read_bytes() != data]
        assert report['changed_inputs'] == []
        report['passed'] = True
    finally:
        (out / 'verification.json').write_text(json.dumps(report, indent=2) + '\n')
    print('Copy and I/O scenarios passed:', len(report['cases']))


if __name__ == '__main__':
    main()
