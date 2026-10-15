#!/usr/bin/env python3
"""Check record diagnostic ownership and child instruction positions in a Q compiler."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

CORE = Path(__file__).resolve().parents[1]
CASES = CORE / 'tests/compiler/behavior-diagnostic-cases.json'


def sha(data):
    return hashlib.sha256(data).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', type=Path, required=True)
    parser.add_argument('--record-dir', type=Path, required=True)
    parser.add_argument('--source-root', type=Path, default=CORE,
                        help='repository source snapshot used to compile the IR ownership probe')
    args = parser.parse_args()
    qemu = shutil.which('qemu-riscv64')
    require(qemu is not None, 'qemu-riscv64 is required')
    record = args.record_dir.resolve()
    record.mkdir(parents=True, exist_ok=False)
    inputs = {'tool.py': Path(__file__).read_bytes(), 'cases.json': CASES.read_bytes(),
              'compiler.elf': args.compiler.read_bytes()}
    for name, data in inputs.items():
        (record / name).write_bytes(data)
    compiler = record / 'compiler.elf'
    compiler.chmod(0o755)
    report = {'input_sha256': {name: sha(data) for name, data in inputs.items()},
              'guest_stack_bytes': 1048576, 'cases': [], 'passed': False}

    def invoke(directory, arguments):
        command = [qemu, '-s', '1048576', *map(str, arguments)]
        process = subprocess.run(command, cwd=directory, env={'PATH': ''},
                                 capture_output=True, text=True, timeout=300)
        return {'command': command, 'exit': process.returncode,
                'stdout': process.stdout, 'stderr': process.stderr}

    def case(spec):
        directory = record / spec['name']
        directory.mkdir()
        files = spec['files']
        for name, source in files.items():
            (directory / name).write_text(source)
        manifest = 'format 1\nprofile linux-user\nentry app::run\n'
        manifest += ''.join(f'source {name}\n' for name in files)
        manifest += 'output program.elf\n'
        (directory / 'program.build').write_text(manifest)
        result = {'name': spec['name'], 'commands': [], 'passed': False}
        report['cases'].append(result)
        source = files['main.vt'].encode()
        diagnostic = None
        if 'error' in spec:
            offset = source.index(spec['marker'].encode())
            diagnostic = f"{spec['error']} file={spec['file']} byte={offset}\n"
            result['expected_diagnostic'] = diagnostic
        for mode in ('check', 'build'):
            output = invoke(directory, [compiler, mode, 'program.build'])
            result['commands'].append(output)
            require(output['exit'] == (1 if diagnostic else 0), f'{mode}: exit mismatch')
            require(output['stdout'] == '', f'{mode}: unexpected stdout')
            if diagnostic:
                require(output['stderr'] == diagnostic, f'{mode}: diagnostic mismatch')
            else:
                require(output['stderr'].startswith('S-next-0.1 '), f'{mode}: missing summary')
            if diagnostic or mode == 'check':
                require(not (directory / 'program.elf').exists(), f'{mode}: unexpected output')
        if not diagnostic:
            output = invoke(directory, [directory / 'program.elf'])
            result['commands'].append(output)
            require(output['exit'] == spec['exit'], 'program: exit mismatch')
            require(output['stdout'] == '', 'program: unexpected stdout')
            require(output['stderr'] == '', 'program: unexpected stderr')
        result['passed'] = True

    def ir_case():
        directory = record / 'ir_ownership'
        directory.mkdir()
        result = {'name': 'ir_ownership', 'commands': [], 'input_sha256': {}, 'passed': False}
        report['cases'].append(result)
        manifest = 'format 1\nprofile linux-user\nentry diagnostic_ir::run\n'
        names = [f'compiler/{name}.vt' for name in
                 ('model', 'target', 'runtime/host_linux', 'input', 'image', 'rules', 'lex', 'parse', 'types', 'init', 'ir', 'check',
                  'runtime/bytes', 'runtime/linux')]
        names += ['compiler/runtime/linux.s0', 'tests/compiler/behavior-diagnostic-ir.vt']
        for name in names:
            source = CORE if name.endswith('behavior-diagnostic-ir.vt') else args.source_root
            data = (source / name).read_bytes()
            target = directory / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
            result['input_sha256'][name] = sha(data)
            manifest += ('assembly ' if name.endswith('.s0') else 'source ') + name + '\n'
        manifest += 'output probe.elf\n'
        (directory / 'probe.build').write_text(manifest)
        output = invoke(directory, [compiler, 'build', 'probe.build'])
        result['commands'].append(output)
        require(output['exit'] == 0, 'IR probe build failed')
        output = invoke(directory, [directory / 'probe.elf'])
        result['commands'].append(output)
        require(output['exit'] == 0 and output['stdout'] == '' and output['stderr'] == '',
                'IR probe failed: child Bounds and record Store spans must stay distinct')
        for name, digest in result['input_sha256'].items():
            source = CORE if name.endswith('behavior-diagnostic-ir.vt') else args.source_root
            require(sha((source / name).read_bytes()) == digest, 'IR probe source changed: ' + name)
        result['passed'] = True

    try:
        specifications = json.loads(inputs['cases.json'])
        require(len(specifications) > 0, 'no diagnostic cases')
        require(len({item['name'] for item in specifications}) == len(specifications),
                'duplicate case name')
        for spec in specifications:
            try:
                case(spec)
            except Exception as error:
                report['cases'][-1]['failure'] = str(error)
        try:
            ir_case()
        except Exception as error:
            report['cases'][-1]['failure'] = str(error)
        require(Path(__file__).read_bytes() == inputs['tool.py'], 'tool changed during run')
        require(CASES.read_bytes() == inputs['cases.json'], 'cases changed during run')
        require(args.compiler.read_bytes() == inputs['compiler.elf'], 'compiler changed during run')
        report['passed'] = all(item['passed'] for item in report['cases'])
        require(report['passed'], 'one or more diagnostic cases failed')
    except Exception as error:
        report['failure'] = str(error)
        report['passed'] = False
    finally:
        (record / 'verification.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f"{sum(item['passed'] for item in report['cases'])}/{len(report['cases'])} diagnostic cases passed")
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
