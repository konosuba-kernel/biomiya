#!/usr/bin/env python3
"""Compile and execute every complete example in the S-next 0.1 reference."""

# Assertions are verification checks, including in imported helpers.
if not __debug__:
    raise SystemExit("verification requires assertions; unset PYTHONOPTIMIZE and omit -O/-OO")

import argparse
import hashlib
import json
from pathlib import Path
import shutil
from check_reference import extract
from check_next import invoke

CORE = Path(__file__).resolve().parents[1]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', type=Path, required=True)
    parser.add_argument('--record-dir', type=Path, required=True)
    args = parser.parse_args()
    spec = json.loads((CORE / 'docs/reference/s-next-0.1.json').read_text())
    compiler_bytes = args.compiler.read_bytes()
    compiler_hash = sha(compiler_bytes)
    if compiler_hash not in spec['compiler_sha256']:
        raise SystemExit('Compiler identity is not registered for S-next 0.1; rebuild the documented candidate.')
    capacities = spec['source_capacity_by_compiler']
    assert set(capacities) == set(spec['compiler_sha256']), 'incomplete compiler capacity identity map'
    assert capacities[compiler_hash] in (32, 64), 'unknown source capacity'
    out = args.record_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    compiler = out / 'compiler.elf'
    compiler.write_bytes(compiler_bytes)
    compiler.chmod(0o755)
    qemu = shutil.which('qemu-riscv64')
    assert qemu
    inputs = {}
    for name in ['docs/reference/s-next-0.1.md', 'docs/reference/s-next-0.1.json',
                 'tools/check_next_reference.py', 'tools/check_reference.py', 'tools/check_next.py']:
        data = (CORE / name).read_bytes()
        inputs[name] = sha(data)
        dest = out / 'inputs' / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(data)
    examples = extract((CORE / 'docs/reference/s-next-0.1.md').read_text())
    assert set(examples) == {case['name'] for case in spec['cases']}
    record = {'version': spec['version'], 'compiler_sha256': compiler_hash,
              'source_capacity': capacities[compiler_hash], 'inputs': inputs, 'cases': []}
    for case in spec['cases']:
        work = out / case['name']
        work.mkdir()
        for name, source in examples[case['name']].items():
            (work / name).write_text(source)
        if not (work / 'program.build').exists():
            manifest = 'format 1\nprofile linux-user\nentry ' + case['entry'] + '\n'
            manifest += ''.join('source ' + name + '\n' for name in examples[case['name']] if name.endswith('.vt'))
            (work / 'program.build').write_text(manifest + 'output program.elf\n')
        result = {'name': case['name'], 'commands': []}
        for mode in ['check', 'build']:
            command = invoke([qemu, '-s', '1048576', str(compiler), mode, 'program.build'], work)
            result['commands'].append(command)
            assert command['exit'] == 0, result
            if mode == 'check':
                assert not (work / 'program.elf').exists()
        run = invoke([qemu, '-s', '1048576', 'program.elf'], work)
        result['commands'].append(run)
        assert run['exit'] == case['exit'] and run['stdout'] == '', result
        result['passed'] = True
        record['cases'].append(result)
    record['passed'] = len(record['cases'])
    (out / 'verification.json').write_text(json.dumps(record, indent=2) + '\n')
    print('Reference examples passed:', record['passed'])


if __name__ == '__main__':
    main()
