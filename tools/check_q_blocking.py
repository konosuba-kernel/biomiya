#!/usr/bin/env python3
"""Replay retained Blocking contracts on the specified Q compiler and actual Q analysis."""

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
IDENTITIES = [
    'blocking_contracts_propagate_without_changing_generated_elf',
    'blocking_policy_is_strict_and_failure_preserves_existing_files',
    'blocking_long_chains_are_order_independent_and_do_not_skip_core_errors',
    'blocking_missing_capture_invalid_ir_and_summary_capacity_fail_closed',
]
PROVIDER = 'poll:\naddi x10,x0,7\njalr x0,0(x1)\nwait:\naddi x10,x0,9\njalr x0,0(x1)\n'
EXTERNS = 'extern abi(C) fn poll()->u32; extern abi(C) fn wait()->u32;\n'


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', type=Path, required=True)
    parser.add_argument('--record-dir', type=Path, required=True)
    parser.add_argument('--external-only', action='store_true', help='development probe; does not accept internal contracts')
    args = parser.parse_args()
    out = args.record_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    compiler = out / 'compiler.elf'
    compiler.write_bytes(args.compiler.read_bytes())
    compiler.chmod(0o755)
    qemu = shutil.which('qemu-riscv64')
    assert qemu
    qemu = str(Path(qemu).resolve())
    inputs = {'tools/check_q_blocking.py': Path(__file__).read_bytes()}
    if not args.external_only:
        inputs['compiler/compiler.build'] = (CORE / 'compiler/compiler.build').read_bytes()
        for line in inputs['compiler/compiler.build'].decode().splitlines():
            if line.startswith(('source ', 'assembly ')):
                name = line.split()[1]
                inputs[name] = (CORE / name).read_bytes()
        inputs['tests/compiler/blocking.vt'] = (CORE / 'tests/compiler/blocking.vt').read_bytes()
    for name, data in inputs.items():
        saved = out / 'inputs' / name
        saved.parent.mkdir(parents=True, exist_ok=True)
        saved.write_bytes(data)
    report = {'compiler_sha256': sha(compiler.read_bytes()), 'qemu': qemu,
              'qemu_sha256': sha(Path(qemu).read_bytes()), 'guest_stack_bytes': 1048576,
              'inputs': {name: sha(data) for name, data in inputs.items()}, 'cases': [],
              'passed': False, 'external_only': args.external_only, 'independently_accepted': False}

    def run(work, argv):
        proc = subprocess.run([qemu, '-s', '1048576', *map(str, argv)], cwd=work,
                              env={'PATH': ''}, capture_output=True, text=True, timeout=240)
        item = {'command': [qemu, '-s', '1048576', *map(str, argv)],
                'exit': proc.returncode, 'stdout': proc.stdout, 'stderr': proc.stderr}
        return item

    def work(name, source, policy, extra=None, assembly=PROVIDER, output='out.elf'):
        directory = out / name
        directory.mkdir()
        files = {'main.vt': source, **(extra or {})}
        for filename, contents in files.items():
            (directory / filename).write_text(contents)
        if assembly:
            (directory / 'provider.s0').write_text(assembly)
        if policy is not None:
            (directory / 'policy').write_text(policy)
        manifest = 'format 1\nprofile linux-user\nentry app::run\n'
        manifest += ''.join('source ' + filename + '\n' for filename in files)
        if assembly:
            manifest += 'assembly provider.s0\n'
        (directory / 'case.build').write_text(manifest + 'output ' + output + '\n')
        return directory

    def case(identity, name):
        result = {'identity': identity, 'name': name, 'commands': [], 'passed': False}
        report['cases'].append(result)
        return result

    def checked(result, directory, mode, expected, diagnostic=None, enabled=True):
        command = [compiler, mode, 'case.build']
        if enabled:
            command += ['--blocking', 'policy']
        result['commands'].append(run(directory, command))
        actual = result['commands'][-1]
        assert actual['exit'] == expected, actual
        if diagnostic:
            assert diagnostic in actual['stderr'], actual
        return actual

    try:
        propagation = [
            ('recursive-verified', 'fn a()->u32{return b();}fn b()->u32{return a();}fn run()->u32{return a();}', '', 'Verified'),
            ('direct', 'fn run()->u32{unsafe{return poll();}}', '', 'Unknown'),
            ('contract', 'fn run()->u32{unsafe{return poll();}}', 'nonblocking app::poll\n', 'Verified'),
            ('blocked', 'fn run()->u32{unsafe{return poll();}}', 'may_block app::poll\n', 'Violation'),
            ('mixed-forward', 'fn run()->u32{unsafe{return poll()+wait();}}', 'may_block app::wait\n', 'Violation'),
            ('mixed-backward', 'fn run()->u32{unsafe{return wait()+poll();}}', 'may_block app::wait\n', 'Violation'),
            ('recursive-unknown', 'fn a()->u32{return b();}fn b()->u32{unsafe{let x:u32=poll();}return a();}fn run()->u32{return a();}', '', 'Unknown'),
            ('recursive-blocked', 'fn a()->u32{return b();}fn b()->u32{unsafe{let x:u32=poll();}return a();}fn run()->u32{return a();}', 'may_block app::poll\n', 'Violation'),
            ('unused-blocked', 'fn run()->u32{return 7;}', 'may_block app::poll\n', 'Verified'),
            ('dead-branch', 'fn run()->u32{if false{unsafe{return poll();}}return 7;}', 'may_block app::poll\n', 'Violation'),
            ('defer', 'fn cleanup()->(){unsafe{let n:u32=poll();}return ();}fn run()->u32{defer cleanup();return 7;}', 'may_block app::poll\n', 'Violation'),
            ('defer-loop', 'fn cleanup()->(){unsafe{let x:u32=poll();}return ();}fn run()->u32{var n:u32=0;while n<2_u32{defer cleanup();n=n+1_u32;if n==1_u32{continue;}break;}return 7;}', 'may_block app::poll\n', 'Violation'),
        ]
        for name, body, contracts, outcome in propagation:
            result = case(IDENTITIES[0], name)
            source = 'module app;' + EXTERNS + body
            policy = 'format 1\nrequire app::run\n' + contracts
            directory = work(name, source, policy)
            checked(result, directory, 'build', 0, enabled=False)
            elf = (directory / 'out.elf').read_bytes()
            (directory / 'out.elf').unlink()
            for mode in ['check', 'build']:
                checked(result, directory, mode, 0 if outcome == 'Verified' else 1,
                        'Blocking app::run: ' + outcome)
                if mode == 'build' and outcome == 'Verified':
                    assert (directory / 'out.elf').read_bytes() == elf
                    result['analysis_elf_equal'] = True
                    (directory / 'out.elf').unlink()
                else:
                    assert not (directory / 'out.elf').exists()
                assert (directory / 'main.vt').read_text() == source
                assert (directory / 'policy').read_text() == policy
            result['passed'] = True

        source = 'module app;extern abi(C) fn poll()->u32;const n:u32=1;fn safe()->u32{return 7;}fn run()->u32{unsafe{return poll();}}'
        extra = {'alias.vt': 'module alias;extern abi(C) fn poll()->u32;'}
        invalid = ['', 'format 2\nrequire app::run\n', 'format 1\n', 'require app::run\nformat 1\n',
                   'format 1\nformat 1\nrequire app::run\n', 'format 1\nrequire app::missing\n',
                   'format 1\nrequire app::n\n', 'format 1\nrequire app::poll\n',
                   'format 1\nrequire app::run\nrequire root::app::run\n',
                   'format 1\nrequire app::run\nnonblocking app::run\n',
                   'format 1\nrequire app::run\nnonblocking app::missing\n',
                   'format 1\nrequire app::run\nnonblocking app::poll\nnonblocking app::poll\n',
                   'format 1\nrequire app::run\nnonblocking app::poll\nmay_block alias::poll\n',
                   'format 1\nrequire app::run\nnonblocking app::poll\nnonblocking alias::poll\n',
                   'format 1\nrequire app::run extra\n', 'format 1\nrequire\n',
                   'format 1\nrequire app::run\nmagic app::poll\n',
                   '#' + 'x' * 16384 + '\nformat 1\nrequire app::safe\n', None]
        for index, policy in enumerate(invalid):
            name = 'invalid-policy-' + str(index)
            result = case(IDENTITIES[1], name)
            directory = work(name, source, policy, extra)
            checked(result, directory, 'build', 1, 'E_BLOCKING_CONFIG')
            assert not (directory / 'out.elf').exists()
            result['passed'] = True
        for index, requirements in enumerate(['require app::safe\nrequire app::run\n', 'require app::run\nrequire app::safe\n']):
            name = 'requirement-order-' + str(index)
            result = case(IDENTITIES[1], name)
            directory = work(name, source, 'format 1\n' + requirements, extra)
            actual = checked(result, directory, 'build', 1, 'Blocking app::safe: Verified')
            assert 'Blocking app::run: Unknown' in actual['stderr']
            assert not (directory / 'out.elf').exists()
            result['passed'] = True
        result = case(IDENTITIES[1], 'symbol-alias-contract')
        directory = work(result['name'], source, '# policy\r\nformat 1\r\nrequire root::app::run\r\nnonblocking alias::poll\r\n', extra)
        checked(result, directory, 'check', 0, 'Blocking app::run: Verified')
        assert not (directory / 'out.elf').exists()
        result['passed'] = True
        for index, policy in enumerate(['format 1\nrequire app::run\n', 'format 1\nrequire app::run\nnonblocking app::poll\n']):
            for output in ['out.elf', 'main.vt', 'policy']:
                name = 'preserve-' + str(index) + '-' + output
                result = case(IDENTITIES[1], name)
                directory = work(name, source, policy, extra, output=output)
                if output == 'out.elf':
                    (directory / output).write_bytes(b'existing output')
                before = {p.name: p.read_bytes() for p in directory.iterdir()}
                checked(result, directory, 'build', 1)
                assert {p.name: p.read_bytes() for p in directory.iterdir()} == before
                result['passed'] = True

        functions = [f'fn f{i}()->u32{{return f{i+1}();}}' for i in range(300)]
        functions.append('fn f300()->u32{unsafe{return poll();}}')
        for index in range(2):
            name = 'long-chain-' + str(index)
            result = case(IDENTITIES[2], name)
            directory = work(name, 'module app;' + EXTERNS + 'fn run()->u32{return f0();}' + '\n'.join(functions),
                             'format 1\nrequire app::run\nmay_block app::poll\n')
            checked(result, directory, 'check', 1, 'Blocking app::run: Violation')
            assert not (directory / 'out.elf').exists()
            result['passed'] = True
            functions.reverse()
        for source, error in [('module app;fn run()->u32{return true;}', 'E_TYPE'),
                              ('module app;extern abi(C) fn missing()->u32;fn run()->u32{return 0;}', 'E_LINK')]:
            result = case(IDENTITIES[2], 'core-' + error)
            directory = work(result['name'], source, 'format 1\nrequire app::run\n', assembly='')
            checked(result, directory, 'build', 1, error)
            assert not (directory / 'out.elf').exists()
            result['passed'] = True

        if not args.external_only:
            directory = out / 'internal'
            directory.mkdir()
            manifest = 'format 1\nprofile linux-user\nentry blocking_probe::run\n'
            for name, data in inputs.items():
                if name.endswith(('.vt', '.s0')) and name != 'compiler/main.vt':
                    dest = directory / name
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    dest.write_bytes(data)
                    manifest += ('source ' if name.endswith('.vt') else 'assembly ') + name + '\n'
            (directory / 'probe.build').write_text(manifest + 'output probe.elf\n')
            result = case(IDENTITIES[3], 'build-real-analysis')
            result['commands'].append(run(directory, [compiler, 'build', 'probe.build']))
            assert result['commands'][-1]['exit'] == 0, result
            result['elf_sha256'] = sha((directory / 'probe.elf').read_bytes())
            result['passed'] = True
            (directory / 'sample.vt').write_text('module sample;fn run()->(){return ();}fn other()->(){return ();}fn sink()->(){return ();}const n:u32=7;')
            (directory / 'sample.build').write_text('format 1\nprofile linux-user\nentry sample::run\nsource sample.vt\noutput should-not-exist.elf\n')
            (directory / 'sample.policy').write_text('format 1\nrequire sample::run\n')
            for mode, code, message in [('missing', 0, 'Blocking sample::run: Unknown'),
                                        ('bad', 1, 'I_INTERNAL'), ('edges', 1, 'actual=65537 limit=65536')]:
                result = case(IDENTITIES[3], mode)
                result['commands'].append(run(directory, [directory / 'probe.elf', mode]))
                actual = result['commands'][-1]
                assert actual['exit'] == code and message in actual['stderr'], actual
                assert not (directory / 'should-not-exist.elf').exists()
                result['passed'] = True
        report['changed_inputs'] = [name for name, data in inputs.items() if (CORE / name).read_bytes() != data]
        assert report['changed_inputs'] == [], report['changed_inputs']
        report['passed'] = all(case['passed'] for case in report['cases'])
        report['internal_contracts_passed'] = not args.external_only
    finally:
        (out / 'verification.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'passed': report['passed'], 'cases': len(report['cases']),
                      'internal_contracts_passed': report['internal_contracts_passed']}))


if __name__ == '__main__':
    main()
