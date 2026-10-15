#!/usr/bin/env python3
"""Check Q save/path behavior with real compiled modules and bounded syscall faults."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

CORE = Path(__file__).resolve().parents[1]
SOURCES = ['compiler/model.vt', 'compiler/target.vt', 'compiler/runtime/host_linux.vt', 'compiler/input.vt', 'compiler/runtime/linux.vt', 'compiler/runtime/bytes.vt']
FIXTURES = ['tests/compiler/behavior-io-save.vt', 'tests/compiler/behavior-io-shim.s0', 'compiler/runtime/linux.s0']
PAYLOAD = b'0123456789abcdef0123456789abcdef'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def invoke(command, directory):
    result = subprocess.run(command, cwd=directory, capture_output=True, text=True,
                            env={'PATH': ''}, timeout=180)
    return {'command': list(map(str, command)), 'exit': result.returncode,
            'stdout': result.stdout, 'stderr': result.stderr}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', type=Path, required=True)
    parser.add_argument('--record-dir', type=Path)
    args = parser.parse_args()
    qemu = shutil.which('qemu-riscv64')
    require(qemu is not None, 'qemu-riscv64 is required')
    compiler = args.compiler.resolve().read_bytes()
    if args.record_dir:
        record = args.record_dir.resolve()
        record.mkdir(parents=True, exist_ok=False)
    else:
        record = Path(tempfile.mkdtemp(prefix='q-io-failures-'))
    inputs = {name: (CORE / name).read_bytes()
              for name in SOURCES + FIXTURES}
    inputs['tools/check_q_io_failures.py'] = Path(__file__).read_bytes()
    for name, data in inputs.items():
        path = record / 'source' / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    image = record / 'compiler.elf'
    image.write_bytes(compiler)
    image.chmod(0o755)
    report = {'compiler_sha256': digest(compiler),
              'input_sha256': {name: digest(data) for name, data in inputs.items()},
              'builds': [], 'tests': [], 'passed': False}

    def build(entry, shim):
        work = record / ('build-' + entry)
        work.mkdir()
        for name in SOURCES:
            target = work / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(inputs[name])
        (work / 'probe.vt').write_bytes(inputs['tests/compiler/behavior-io-save.vt'])
        assembly = inputs['compiler/runtime/linux.s0'].decode()
        if shim:
            assembly = inputs['tests/compiler/behavior-io-shim.s0'].decode() + assembly.replace(
                '__b2_syscall6:', '__b2_real:', 1)
        (work / 'linux.s0').write_text(assembly)
        (work / 'probe.build').write_text(
            f'format 1\nprofile linux-user\nentry behavior_io::{entry}\n' +
            ''.join(f'source {name}\n' for name in SOURCES) +
            'source probe.vt\nassembly linux.s0\noutput probe.elf\n')
        outcome = invoke([qemu, '-s', '1048576', str(image), 'build', 'probe.build'], work)
        report['builds'].append({'entry': entry, **outcome})
        require(outcome['exit'] == 0, f'build failed: {outcome}')
        executable = work / 'probe.elf'
        report['builds'][-1]['elf_sha256'] = digest(executable.read_bytes())
        return executable

    def save_case(executable, name, mode, events, phase=None, code=0, closed=0,
                  cleanup=0, residual=None, existing=False):
        work = record / ('case-' + name)
        work.mkdir()
        (work / 'payload').write_bytes(PAYLOAD)
        (work / 'input.build').write_text(
            'format 1\nprofile linux-user\nentry unused\nsource payload\noutput result.elf\n')
        output = work / 'result.elf'
        if existing:
            output.write_bytes(b'protected output')
            output.chmod(0o640)
        outcome = invoke([qemu, '-s', '1048576', str(executable), mode], work)
        actual_events = ''.join(re.findall(r'\[([OWMCU])\]', outcome['stderr']))
        diagnostic = re.sub(r'\[[OWMCU]\]', '', outcome['stderr'])
        case = {'name': name, **outcome, 'events': actual_events, 'diagnostic': diagnostic,
                'output_exists': output.exists(),
                'output_hex': output.read_bytes().hex() if output.exists() else None,
                'output_mode': oct(output.stat().st_mode & 0o777) if output.exists() else None,
                'passed': False}
        report['tests'].append(case)
        require(outcome['exit'] == (1 if phase else 0), f'{name}: exit {outcome}')
        require(actual_events == events, f'{name}: events {actual_events!r}, expected {events!r}')
        require((work / 'payload').read_bytes() == PAYLOAD, f'{name}: source changed')
        expected = ''
        if phase:
            expected = (f'save path=result.elf phase={phase} errno={code} '
                        f'close_errno={closed} cleanup_errno={cleanup}')
            if cleanup:
                expected += ' residual=result.elf'
            expected += ' E_IO file=0 byte=0\n'
        require(diagnostic == expected, f'{name}: diagnostic {diagnostic!r}, expected {expected!r}')
        if existing:
            require(output.read_bytes() == b'protected output', f'{name}: existing output damaged')
            require(case['output_mode'] == '0o640', f'{name}: existing permissions changed')
        elif residual is not None:
            require(output.exists() and output.read_bytes() == PAYLOAD[:residual],
                    f'{name}: wrong residual contents')
            expected_mode = '0o755' if phase == 'close' else '0o600'
            require(case['output_mode'] == expected_mode, f'{name}: wrong residual permissions')
        elif phase:
            require(not output.exists(), f'{name}: failed output was not removed')
        else:
            require(output.read_bytes() == PAYLOAD, f'{name}: successful output differs')
            require(case['output_mode'] == '0o755', f'{name}: successful output is not executable')
        case['passed'] = True

    try:
        executable = build('save', True)
        save_case(executable, 'normal', 'n', 'OWMC')
        save_case(executable, 'existing', 'n', 'O', 'create', 17, existing=True)
        save_case(executable, 'short', 's', 'OWWWWWMC')
        save_case(executable, 'write', 'w', 'OWWCU', 'write', 5)
        save_case(executable, 'zero-progress', 'z', 'OWCU', 'write', 5)
        save_case(executable, 'chmod', 'm', 'OWMCU', 'chmod', 13)
        save_case(executable, 'close', 'c', 'OWMCU', 'close', 5, closed=5)
        save_case(executable, 'write-unlink', 'u', 'OWWCU', 'write', 5, cleanup=13, residual=7)
        save_case(executable, 'write-close-unlink', 'v', 'OWWCU', 'write', 5,
                  closed=5, cleanup=13, residual=7)
        save_case(executable, 'chmod-unlink', 'h', 'OWMCU', 'chmod', 13, cleanup=13, residual=32)
        save_case(executable, 'close-unlink', 'l', 'OWMCU', 'close', 5,
                  closed=5, cleanup=13, residual=32)
        # This entry never arms the shim; open/remove exercise the unmodified real adapter.
        nul = build('nul', True)
        work = record / 'case-nul'
        work.mkdir()
        (work / 'victim').write_bytes(b'untouched sentinel')
        outcome = invoke([qemu, '-s', '1048576', str(nul), 'n'], work)
        case = {'name': 'nul-open-remove', **outcome, 'passed': False}
        report['tests'].append(case)
        require(outcome['exit'] == 0, f'NUL was not rejected: {outcome}')
        require((work / 'victim').read_bytes() == b'untouched sentinel', 'NUL prefix was modified')
        require(outcome['stderr'] == '', 'unexpected NUL diagnostic')
        case['passed'] = True
        require(all((CORE / name).read_bytes() == data for name, data in inputs.items()),
                'inputs changed during validation')
        report['passed'] = True
    except Exception as error:
        report['error'] = str(error)
        raise
    finally:
        (record / 'verification.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'passed': report['passed'], 'cases': len(report['tests']), 'record': str(record)}))


if __name__ == '__main__':
    main()
