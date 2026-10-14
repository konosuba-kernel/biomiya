#!/usr/bin/env python3
"""Execute the P3 representation comparison with a named compiler candidate."""

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
from verify import memory

CORE = Path(__file__).resolve().parents[1]


def digest(data):
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
    record = {'compiler_sha256': digest(compiler.read_bytes()), 'inputs': {}, 'cases': []}

    def copy(name):
        data = (CORE / name).read_bytes()
        record['inputs'][name] = digest(data)
        path = out / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        return data

    copy('tools/check_next_trials.py')
    spec = json.loads(copy('tests/trials/cases.json'))
    cases = spec['cases'] + [{'manifest': 'experiments/system/file_rollback.build', 'exit': 0}]
    for case in cases:
        manifest = copy(case['manifest']).decode()
        for line in manifest.splitlines():
            if line.startswith(('source ', 'assembly ')):
                copy(line.split()[1])
        name = Path(case['manifest']).stem
        output = name + '.elf'
        (out / (name + '.build')).write_text(re.sub(r'^output .*$', 'output ' + output, manifest, flags=re.M))
        command = [qemu, '-s', '1048576', str(compiler), 'build', name + '.build']
        result = subprocess.run(command, cwd=out, env={'PATH': ''}, capture_output=True, text=True, timeout=180)
        item = {'case': case, 'command': command, 'exit': result.returncode,
                'stdout': result.stdout, 'stderr': result.stderr}
        record['cases'].append(item)
        if case.get('reject'):
            expected = 'E_NAME' if case['reject'] is True else case['reject']
            item['passed'] = result.returncode != 0 and expected in result.stderr and not (out / output).exists()
        elif result.returncode == 0:
            binary = (out / output).read_bytes()
            item['elf_sha256'] = digest(binary)
            item['elf_bytes'] = len(binary)
            item['memory'] = memory(binary)
            run = subprocess.run([qemu, '-s', '1048576', output], cwd=out, env={'PATH': ''}, capture_output=True, text=True, timeout=60)
            item['run'] = {'exit': run.returncode, 'stdout': run.stdout, 'stderr': run.stderr}
            item['passed'] = run.returncode == case['exit'] and run.stdout == ''
        else:
            item['passed'] = False
        print(name, item['passed'], flush=True)
    record['passed'] = all(case['passed'] for case in record['cases'])
    (out / 'verification.json').write_text(json.dumps(record, indent=2) + '\n')
    assert record['passed'], out


if __name__ == '__main__':
    main()
