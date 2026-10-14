#!/usr/bin/env python3
"""Build Q's IR/backend and AST fixtures with an explicit compiler and replay them."""

# Assertions are verification checks, including in imported helpers.
if not __debug__:
    raise SystemExit("verification requires assertions; unset PYTHONOPTIMIZE and omit -O/-OO")

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import struct
import tarfile
import time
from verify import memory

CORE = Path(__file__).resolve().parents[1]
SPECS = ('tests/compiler/machine-cases.json', 'tests/compiler/ast-cases.json',
         'tests/compiler/owner-cases.json')


def sha(data):
    return hashlib.sha256(data).hexdigest()


def relative(name):
    path = Path(name)
    if path.is_absolute() or '..' in path.parts:
        raise ValueError(f'unsafe input path: {name}')
    return path


def matches(run, case, prefix=''):
    return (run['exit'] == case[prefix+'exit']
            and (prefix+'stdout' not in case or run['stdout'] == case[prefix+'stdout'])
            and (prefix+'stderr' not in case or run['stderr'] == case[prefix+'stderr'])
            and (prefix+'stdout_hex' not in case or run['stdout_hex'] == case[prefix+'stdout_hex'])
            and (prefix+'stderr_contains' not in case
                 or case[prefix+'stderr_contains'] in run['stderr']))


def padding_layout(data, padding):
    """Independent ELF contract for three exit instructions plus unreachable words."""
    text = struct.pack('<III', 0x00000513, 0x05d00893, 0x00000073)
    text += struct.pack('<I', 0xc0001073) * padding
    align = lambda n: (n + 4095) & ~4095
    ro_offset = align(4096 + len(text))
    rw_offset = align(ro_offset + 1)
    ro_address = align(65536 + len(text))
    rw_address = align(ro_address + 1)
    expected = bytearray(rw_offset + 1)
    struct.pack_into('<16sHHIQQQIHHHHHH', expected, 0,
                     b'\x7fELF\x02\x01\x01', 2, 243, 1, 65536, 64, 0, 0,
                     64, 56, 3, 0, 0, 0)
    headers = [(5, 4096, 65536, len(text)),
               (4, ro_offset, ro_address, 1), (6, rw_offset, rw_address, 1)]
    for index, (flags, offset, address, size) in enumerate(headers):
        struct.pack_into('<IIQQQQQQ', expected, 64 + index * 56,
                         1, flags, offset, address, address, size, size, 4096)
    expected[4096:4096 + len(text)] = text
    return {'passed': data == expected, 'padding_words': padding,
            'text_bytes': len(text), 'expected_file_bytes': len(expected),
            'expected_sha256': sha(expected), 'actual_sha256': sha(data),
            'checks': ['all ELF header fields', 'all three program headers',
                       'exact exit and padding instructions', 'zero intersegment padding',
                       'zero empty data segments', 'exact file end']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', required=True, type=Path)
    parser.add_argument('--record-dir', required=True, type=Path)
    args = parser.parse_args()
    out = args.record_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    tree = out/'tree'
    tree.mkdir()
    compiler = out/'compiler.elf'
    compiler.write_bytes(args.compiler.read_bytes())
    compiler.chmod(0o755)
    qemu = shutil.which('qemu-riscv64')
    if not qemu:
        raise RuntimeError('qemu-riscv64 is required')
    qemu = str(Path(qemu).resolve())
    record = {'schema': 'q-machine-v1', 'independently_accepted': False,
              'compiler_sha256': sha(compiler.read_bytes()),
              'compiler_memory': memory(compiler.read_bytes()),
              'qemu': qemu, 'qemu_sha256': sha(Path(qemu).read_bytes()),
              'guest_stack_bytes': 1048576, 'environment': {'PATH': ''},
              'inputs': {}, 'builds': [], 'cases': [], 'passed': False}

    def save():
        (out/'verification.json').write_text(json.dumps(record, indent=2)+'\n')

    def copy(name):
        data = (CORE/relative(name)).read_bytes()
        digest = sha(data)
        if name in record['inputs'] and record['inputs'][name] != digest:
            raise RuntimeError(f'input changed while collecting: {name}')
        record['inputs'][name] = digest
        dest = tree/name
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(data)
        return data

    def execute(command, cwd, timeout=180, host=False):
        start = time.monotonic()
        result = subprocess.run(command, cwd=cwd, env=os.environ if host else {'PATH': ''},
                                stdin=subprocess.DEVNULL, capture_output=True,
                                timeout=timeout)
        return {'command': command, 'cwd': str(cwd), 'host_oracle_build': host, 'exit': result.returncode,
                'stdout': result.stdout.decode(errors='replace'),
                'stdout_hex': result.stdout.hex(),
                'stderr': result.stderr.decode(errors='replace'),
                'elapsed_seconds': time.monotonic()-start}

    def elf(path):
        data = path.read_bytes()
        if data[:5] != b'\x7fELF\x02' or data[18:20] != b'\xf3\x00':
            raise RuntimeError(f'not an RV64 ELF: {path}')
        path.chmod(0o755)
        headers = segments(data)
        return {'sha256': sha(data), 'bytes': len(data), 'memory': memory(data),
                'rw_file_bytes': sum(h[5] for h in headers if h[0] == 1 and h[1] & 2),
                'rw_memory_bytes': sum(h[6] for h in headers if h[0] == 1 and h[1] & 2)}

    def segments(data):
        offset = struct.unpack_from('<Q', data, 32)[0]
        stride, count = struct.unpack_from('<HH', data, 54)
        return [struct.unpack_from('<IIQQQQQQ', data, offset+i*stride)
                for i in range(count)]

    def oracle(case, item, directory, target_path):
        spec = case['oracle']
        clang, linker = shutil.which('clang'), shutil.which('ld.lld')
        if not clang or not linker:
            raise RuntimeError('clang and ld.lld are required for independent assembly oracles')
        clang, linker = str(Path(clang).resolve()), str(Path(linker).resolve())
        item['oracle'] = evidence = {'kind': spec['kind'],
            'tools': {clang: sha(Path(clang).read_bytes()), linker: sha(Path(linker).read_bytes())},
            'commands': []}
        source, obj = tree/spec['assembly'], directory/'reference.o'
        command = [clang, '--target=riscv64-unknown-elf', '-march=rv64im_zicsr',
                   '-mabi=lp64', '-mno-relax', '-c', str(source), '-o', str(obj)]
        compiled = execute(command, directory)
        evidence['commands'].append(compiled)
        if compiled['exit'] != 0:
            return False
        reference = directory/('reference.bin' if spec['kind'] == 'text_bytes' else 'reference.elf')
        command = [linker, '--no-relax', '-Ttext=0x10000', '-e', 'entry']
        if spec['kind'] == 'text_bytes':
            command.append('--oformat=binary')
        linked = execute(command+[str(obj), '-o', str(reference)], directory)
        evidence['commands'].append(linked)
        if linked['exit'] != 0:
            return False
        evidence['reference_sha256'] = sha(reference.read_bytes())
        rust_reference = directory/'rust-reference.elf'
        rust = execute([str(rust_binary), 's0' if spec['kind'] == 'text_bytes' else 'constants',
                        str(directory/'input.s0'), str(rust_reference)], directory)
        evidence['commands'].append(rust)
        if rust['exit'] != 0:
            return False
        evidence['rust_reference_sha256'] = sha(rust_reference.read_bytes())
        evidence['rust_exact_elf'] = rust_reference.read_bytes() == target_path.read_bytes()
        if not evidence['rust_exact_elf']:
            return False
        if spec['kind'] == 'text_bytes':
            data = target_path.read_bytes()
            headers = [h for h in segments(data) if h[0] == 1 and h[1] & 1]
            if len(headers) != 1:
                return False
            h = headers[0]
            actual = data[h[2]:h[2]+h[5]]
            evidence['target_text_sha256'] = sha(actual)
            evidence['text_bytes'] = len(actual)
            return actual == reference.read_bytes()
        evidence['elf'] = elf(reference)
        run = execute([qemu, '-s', '1048576', str(reference)], directory, 60)
        evidence['commands'].append(run)
        return matches(run, case['target']) and run['stdout_hex'] == item['target_run']['stdout_hex']

    try:
        copy('tools/check_q_machine.py')
        copy('tools/verify.py')
        copy('bootstrap/frozen/core-s/tools/verify.py')
        for name in ['tools/q_machine_oracle.rs', 'bootstrap/frozen/core-s/src/machine.rs', 'bootstrap/frozen/core-s/src/encode.rs', 'bootstrap/frozen/core-s/src/image.rs', 'bootstrap/frozen/core-s/src/s0.rs']:
            copy(name)
        cases = []
        for name in SPECS:
            cases.extend(json.loads(copy(name)))
        names = [case['name'] for case in cases]
        if len(names) != len(set(names)):
            raise RuntimeError('duplicate case names')
        for case in cases:
            for value in case.get('files', {}).values():
                if 'source' in value:
                    copy(value['source'])
            if 'oracle' in case:
                copy(case['oracle']['assembly'])
        manifests = {}
        for case in cases:
            name = case['manifest']
            if name not in manifests:
                text = copy(name).decode()
                for line in text.splitlines():
                    if line.startswith(('source ', 'assembly ')):
                        copy(line.split()[1])
                if len(re.findall(r'^output .+$', text, re.M)) != 1:
                    raise RuntimeError(f'manifest must have one output: {name}')
                manifests[name] = text
        capsule = out/'source.tar'
        with tarfile.open(capsule, 'w', format=tarfile.USTAR_FORMAT) as archive:
            for name in sorted(record['inputs']):
                data = (tree/name).read_bytes()
                info = tarfile.TarInfo(name)
                info.size = len(data)
                info.mode = 0o644
                archive.addfile(info, io.BytesIO(data))
        record['capsule_sha256'] = sha(capsule.read_bytes())
        rustc = shutil.which('rustc')
        if not rustc:
            raise RuntimeError('rustc is required solely for the frozen machine oracle')
        rustc = str(Path(rustc).absolute())
        sysroot = execute([rustc, '--print', 'sysroot'], tree, host=True)
        if sysroot['exit'] != 0:
            raise RuntimeError('cannot identify the Rust oracle compiler')
        rustc = str(Path(sysroot['stdout'].strip())/'bin/rustc')
        rust_binary = out/'rust-machine-oracle'
        compiled = execute([rustc, '--edition=2024', str(tree/'tools/q_machine_oracle.rs'),
                            '-o', str(rust_binary)], tree, host=True)
        record['rust_oracle'] = {'rustc': rustc, 'rustc_sha256': sha(Path(rustc).read_bytes()),
                                 'build': compiled, 'sysroot_command': sysroot, 'reads_vt': False}
        if compiled['exit'] != 0:
            raise RuntimeError('failed to build the independent Rust machine oracle')
        record['rust_oracle']['sha256'] = sha(rust_binary.read_bytes())
        builds = {}
        for number, (manifest, text) in enumerate(manifests.items()):
            directory = tree/'builds'/str(number)
            directory.mkdir(parents=True)
            binary = directory/'probe.elf'
            normalized = directory/'probe.build'
            normalized.write_text(re.sub(r'^output .+$',
                                        'output '+str(binary.relative_to(tree)),
                                        text, flags=re.M))
            result = execute([qemu, '-s', '1048576', str(compiler), 'build',
                              str(normalized.relative_to(tree))], tree)
            result.update(manifest=manifest,
                          normalized_manifest_sha256=sha(normalized.read_bytes()))
            if result['exit'] == 0:
                result['elf'] = elf(binary)
            else:
                result['output_absent'] = not binary.exists()
            builds[manifest] = (result, binary)
            record['builds'].append(result)
            save()
        for case in cases:
            build, binary = builds[case['manifest']]
            item = {'case': case, 'passed': matches(build, case, 'compile_')}
            record['cases'].append(item)
            if case['compile_exit'] != 0:
                item['passed'] = item['passed'] and build.get('output_absent', False)
            elif item['passed']:
                directory = out/'runs'/case['name']
                directory.mkdir(parents=True)
                for name, value in case.get('files', {}).items():
                    dest = directory/relative(name)
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    dest.write_bytes((tree/value['source']).read_bytes() if 'source' in value
                                     else value['text'].encode())
                run = execute([qemu, '-s', '1048576', str(binary),
                               *case.get('arguments', [])], directory, 60)
                item['run'] = run
                item['passed'] = matches(run, case)
                if 'output_absent' in case:
                    item['passed'] &= not (directory/relative(case['output_absent'])).exists()
                if 'target' in case and item['passed']:
                    target = case['target']
                    path = directory/relative(target['path'])
                    item['target_elf'] = elf(path)
                    item['target_run'] = execute([qemu, '-s', '1048576', str(path)], directory, 60)
                    item['passed'] &= matches(item['target_run'], target)
                    for name in ['rw_file_bytes', 'rw_memory_bytes']:
                        if name in target:
                            item['passed'] &= item['target_elf'][name] == target[name]
                    if 'elf_padding_words' in case:
                        item['layout'] = padding_layout(path.read_bytes(), case['elf_padding_words'])
                        item['passed'] &= item['layout']['passed']
                    if case.get('repeat_generator'):
                        repeated = directory/'repeat'
                        repeated.mkdir()
                        replay = execute([qemu, '-s', '1048576', str(binary),
                                          *case.get('arguments', [])], repeated, 60)
                        repeated_path = repeated/relative(target['path'])
                        replay['passed'] = matches(replay, case) and repeated_path.exists()
                        if replay['passed']:
                            replay['sha256'] = sha(repeated_path.read_bytes())
                            replay['passed'] &= repeated_path.read_bytes() == path.read_bytes()
                        item['repeat_generator'] = replay
                        item['passed'] &= replay['passed']
                    if 'oracle' in case and item['passed']:
                        item['passed'] &= oracle(case, item, directory, path)
            print(case['name'], item['passed'], flush=True)
            save()
        changed = [name for name, digest in record['inputs'].items()
                   if sha((CORE/name).read_bytes()) != digest]
        record['changed_inputs'] = changed
        record['passed'] = not changed and all(item['passed'] for item in record['cases'])
        save()
        if not record['passed']:
            raise RuntimeError(f'Q machine/AST checks failed: {out}')
    except Exception as error:
        record['passed'] = False
        record['error'] = str(error)
        save()
        raise


if __name__ == '__main__':
    main()
