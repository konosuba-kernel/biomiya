#!/usr/bin/env python3
"""Compare bounded S0 machine instructions with Clang, reject bad operands, run sh.

Clang is an independent test oracle only; Q compiles every tested ELF itself.
This verifier uses explicit checks and remains effective under Python -O/-OO.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess

CORE = Path(__file__).resolve().parents[1]
FIXTURES = CORE / 'tests/compiler'
NAMES = ('k0-s0-encoding.s0', 'k0-s0-invalid.json', 'k0-s0-run.vt', 'k0-s0-run.s0')
PREFIX = 'addi x29, x0, 1234\nxori x28, x29, -1234\n'
SUFFIX = 'addi x28, x0, 1235\nxori x29, x28, -1235\n'


def sha(data):
    return hashlib.sha256(data).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def text_segment(elf):
    require(elf[:7] == b'\x7fELF\x02\x01\x01', 'not RV64 little-endian ELF')
    require(struct.unpack_from('<H', elf, 18)[0] == 243, 'wrong ELF machine')
    start = struct.unpack_from('<Q', elf, 32)[0]
    width, count = struct.unpack_from('<HH', elf, 54)
    matches = []
    for index in range(count):
        kind, flags, offset, address, _, size, _, _ = struct.unpack_from(
            '<IIQQQQQQ', elf, start + index * width)
        if kind == 1 and flags & 1:
            matches.append((address, elf[offset:offset + size]))
    require(len(matches) == 1, 'expected one executable load segment')
    return matches[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', type=Path, required=True)
    parser.add_argument('--record-dir', type=Path, required=True)
    args = parser.parse_args()
    out = args.record_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    executables = {}
    for name in ('qemu-riscv64', 'clang'):
        path = shutil.which(name)
        require(path is not None, f'{name} is required')
        executables[name] = str(Path(path).resolve())
    snapshots = {name: (FIXTURES / name).read_bytes() for name in NAMES}
    snapshots['tool.py'] = Path(__file__).read_bytes()
    snapshots['compiler.elf'] = args.compiler.read_bytes()
    for name, data in snapshots.items():
        (out / name).write_bytes(data)
    compiler = out / 'compiler.elf'
    compiler.chmod(0o755)
    report = {'schema': 'q-s0-system-v1', 'passed': False,
              'input_sha256': {name: sha(data) for name, data in snapshots.items()},
              'tools': {name: {'path': path, 'sha256': sha(Path(path).read_bytes())}
                        for name, path in executables.items()}, 'cases': []}

    def invoke(directory, command, guest=False):
        run = subprocess.run(command, cwd=directory, capture_output=True, text=True,
                             timeout=300, env={'PATH': ''} if guest else None)
        return {'command': command, 'exit': run.returncode,
                'stdout': run.stdout, 'stderr': run.stderr}

    def oracle(directory, assembly, name, result):
        source = directory / (name + '.s')
        source.write_text('.option norvc\n.option norelax\n.text\n' + assembly)
        obj = directory / (name + '.o')
        command = [executables['clang'], '--target=riscv64',
                   '-march=rv64im_zicsr_zifencei', '-mabi=lp64', '-c',
                   '-o', str(obj), str(source)]
        run = invoke(directory, command)
        result['commands'].append(run)
        require(run['exit'] == 0, 'Clang oracle command failed')
        data = obj.read_bytes()
        table = struct.unpack_from('<Q', data, 40)[0]
        width, count, names_index = struct.unpack_from('<HHH', data, 58)
        sections = [struct.unpack_from('<IIQQQQIIQQ', data, table + index * width)
                    for index in range(count)]
        names = sections[names_index]
        strings = data[names[4]:names[4] + names[5]]
        found = []
        for section in sections:
            name_offset = section[0]
            name = strings[name_offset:strings.index(b'\0', name_offset)]
            if name == b'.text':
                found.append(data[section[4]:section[4] + section[5]])
            if name in (b'.rela.text', b'.rel.text'):
                require(section[5] == 0, 'oracle text contains unresolved relocations')
        require(len(found) == 1, 'oracle has no unique .text section')
        return found[0]

    def compile_case(directory, assembly, result, source=None):
        (directory / 'main.vt').write_text(source or 'module app; pub fn run() -> u32 { return 0; }\n')
        (directory / 'code.s0').write_text(assembly)
        entry = 'k0_s0::run' if source else 'app::run'
        (directory / 'program.build').write_text(
            f'format 1\nprofile linux-user\nentry {entry}\nsource main.vt\n'
            'assembly code.s0\noutput program.elf\n')
        run = invoke(directory, [executables['qemu-riscv64'], '-s', '1048576',
                                str(compiler), 'build', 'program.build'], True)
        result['commands'].append(run)
        return run

    def encoding(directory, result, body):
        assembly = PREFIX + body + SUFFIX
        run = compile_case(directory, assembly, result)
        require(run['exit'] == 0, 'Q rejected valid S0')
        prefix = oracle(directory, PREFIX, 'prefix', result)
        suffix = oracle(directory, SUFFIX, 'suffix', result)
        address, text = text_segment((directory / 'program.elf').read_bytes())
        require(text.count(prefix) == 1 and text.count(suffix) == 1, 'ambiguous markers')
        first = text.index(prefix)
        last = text.index(suffix) + len(suffix)
        # Match the real VA residue so Clang, independently, decides alignment padding.
        residue = (address + first) % 4096
        oracle_source = f'.space {residue}, 0\n'
        for line in assembly.splitlines():
            if line.startswith('align '):
                line = '.balign ' + line.split()[1]
            elif line == 'fence':
                line = 'fence iorw, iorw'
            oracle_source += line + '\n'
        expected = oracle(directory, oracle_source, 'oracle', result)
        require(expected.count(suffix) == 1, 'ambiguous oracle end')
        expected = expected[residue:expected.index(suffix) + len(suffix)]
        actual = text[first:last]
        result.update({'start_address': address + first, 'compared_bytes': len(actual),
                       'oracle_sha256': sha(expected), 'q_sha256': sha(actual)})
        require(actual == expected, 'instruction/padding bytes differ from Clang')
        # Source-generated fixture has no relocation; whole marked region is compared.

    def run_case(name, action):
        directory = out / name
        directory.mkdir()
        result = {'name': name, 'commands': [], 'passed': False}
        report['cases'].append(result)
        try:
            action(directory, result)
            result['passed'] = True
        except Exception as error:
            result['failure'] = str(error)

    def negative(directory, result, assembly):
        result['assembly'] = assembly
        run = compile_case(directory, assembly + '\n', result)
        require(run['exit'] == 1 and 'E_PARSE' in run['stderr'], 'invalid operand accepted/wrong error')
        require(not (directory / 'program.elf').exists(), 'invalid source wrote output ELF')

    def runnable(directory, result):
        run = compile_case(directory, snapshots['k0-s0-run.s0'].decode(), result,
                           snapshots['k0-s0-run.vt'].decode())
        require(run['exit'] == 0, 'sh execution fixture failed to compile')
        run = invoke(directory, [executables['qemu-riscv64'], str(directory / 'program.elf')], True)
        result['commands'].append(run)
        require(run['exit'] == 0 and run['stdout'] == '' and run['stderr'] == '',
                'sh/fence/aligned branch execution failed')

    try:
        run_case('encodings', lambda d, r: encoding(d, r, snapshots['k0-s0-encoding.s0'].decode()))
        for alignment in (4, 8, 16, 256, 4096):
            body = f'addi x7, x0, 9\nalign {alignment}\naddi x8, x0, 10\n'
            run_case(f'align_{alignment}', lambda d, r, b=body: encoding(d, r, b))
        invalid = json.loads(snapshots['k0-s0-invalid.json'])
        require(invalid and len(invalid) == len(set(invalid)), 'invalid corpus empty/duplicated')
        for index, assembly in enumerate(invalid):
            run_case(f'invalid_{index:03}', lambda d, r, a=assembly: negative(d, r, a))
        run_case('halfword_execution', runnable)
        require(args.compiler.read_bytes() == snapshots['compiler.elf'], 'compiler changed')
        require(Path(__file__).read_bytes() == snapshots['tool.py'], 'tool changed')
        for name in NAMES:
            require((FIXTURES / name).read_bytes() == snapshots[name], 'fixture changed: ' + name)
        report['passed'] = all(case['passed'] for case in report['cases'])
    except Exception as error:
        report['failure'] = str(error)
    (out / 'verification.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f"{sum(case['passed'] for case in report['cases'])}/{len(report['cases'])} S0 checks passed")
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
