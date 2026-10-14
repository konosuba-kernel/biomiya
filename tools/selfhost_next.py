#!/usr/bin/env python3
"""Produce P4 bootstrap evidence; independent language acceptance remains external.

B(P)=E0 must reproduce an accepted P3 candidate. E0(Q)=E1, E1(Q)=E2,
E2(Q)=E3 must stabilize as complete ELF files. A separate Q source mutation
must change the resulting compiler's observable statistics prefix.
"""

# Assertions are verification checks, including in imported helpers.
if not __debug__:
    raise SystemExit("verification requires assertions; unset PYTHONOPTIMIZE and omit -O/-OO")

import argparse
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import re
import shutil
import struct
import subprocess
import tarfile
import tempfile
import time

from verify import memory

CORE = Path(__file__).resolve().parents[1]
REPO = CORE / "bootstrap/frozen"
STACK = 1048576
LIMIT = 64 * 1024 * 1024
RESERVED = {'compiler.build', 'compiler.elf', 'driver.elf'}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def hashes(files):
    return {name: sha(data) for name, data in sorted(files.items())}


def relative(name):
    path = PurePosixPath(name)
    require(name and not path.is_absolute() and '\\' not in name and
            all(part not in ('', '.', '..') for part in name.split('/')),
            f'unsafe capsule/manifest path: {name!r}')
    return name


def current_files(root, expected):
    result = {}
    for name, digest in expected.items():
        path = root / relative(name)
        require(path.resolve().is_relative_to(root.resolve()), f'input escapes tree: {name}')
        data = path.read_bytes()
        require(sha(data) == digest, f'current input differs from accepted evidence: {name}')
        result[name] = data
    return result


def capsule(path):
    files = {}
    with tarfile.open(path, 'r:') as archive:
        for member in archive:
            name = relative(member.name)
            require(member.isfile() and name not in files, f'non-regular or duplicate capsule entry: {name}')
            require(0 <= member.size <= LIMIT, f'oversized capsule member: {name}')
            files[name] = archive.extractfile(member).read()
            require(sum(map(len, files.values())) <= LIMIT * 4, 'oversized source capsule')
    return files


def write_capsule(path, files):
    with tarfile.open(path, 'w') as archive:
        for name, data in sorted(files.items()):
            info = tarfile.TarInfo(relative(name))
            info.size, info.mode, info.mtime = len(data), 0o644, 0
            archive.addfile(info, io.BytesIO(data))


def manifest(data):
    fields, inputs, lines = {}, [], []
    for raw in data.decode('utf-8').splitlines():
        line = raw.split('#', 1)[0].strip()
        if not line:
            continue
        parts = line.split()
        require(len(parts) == 2, f'unsupported manifest line: {raw}')
        key, value = parts
        if key in ('source', 'assembly'):
            relative(value)
            require(value not in RESERVED and value not in inputs, f'duplicate/reserved input: {value}')
            require(value.endswith('.vt' if key == 'source' else '.s0'), f'unexpected input kind: {value}')
            require(len(value.encode()) <= 96, f'input path exceeds current target limit: {value}')
            inputs.append(value)
        else:
            require(key in ('format', 'profile', 'entry', 'output') and key not in fields,
                    f'unknown/duplicate manifest field: {key}')
            fields[key] = value
        lines.append((key, value))
    require(set(fields) == {'format', 'profile', 'entry', 'output'}, 'incomplete manifest')
    require(fields['format'] == '1' and fields['profile'] == 'linux-user', 'unsupported manifest format/profile')
    require(inputs, 'manifest contains no inputs')
    relative(fields['output'])
    require(fields['output'] not in inputs, 'manifest output overwrites input')
    normalized = ''.join(f'{key} {"compiler.elf" if key == "output" else value}\n' for key, value in lines).encode()
    return inputs, normalized, fields['entry']


def trace_contract(text, read_names, output_name=None):
    """Check every guest syscall, path, and accessed file descriptor.

    QEMU itself loads driver.elf outside guest syscalls. Guest opens of that
    file, /proc/self/exe, host tools, sockets and unexpected inherited fds fail.
    """
    opened, read_opened, descriptors, syscall_counts = [], set(), {}, {}
    for line in text.splitlines():
        match = re.fullmatch(r'\d+ ([A-Za-z_][A-Za-z_0-9]*)\((.*)\)(?:\s+=\s+(-?\d+).*)?', line)
        require(match is not None, f'unrecognized guest trace line: {line}')
        call, arguments, result = match.groups()
        syscall_counts[call] = syscall_counts.get(call, 0) + 1
        require(call in {'openat', 'read', 'write', 'close', 'fchmod', 'exit', 'exit_group', 'unlinkat'},
                f'forbidden guest syscall: {call}')
        if call == 'openat':
            opened_call = re.fullmatch(r'AT_FDCWD,"([^"\\]+)",([^,]+)(?:,[^,]+)?', arguments)
            require(opened_call is not None, f'unsupported open form: {line}')
            name, flags = opened_call.groups()
            if name in read_names:
                require(flags == 'O_RDONLY', f'input is not read-only: {line}')
                mode = 'read'
            else:
                require(output_name is not None and name == output_name and
                        set(flags.split('|')) == {'O_WRONLY', 'O_CREAT', 'O_EXCL'},
                        f'open outside declared input/output contract: {line}')
                mode = 'write'
            opened.append(dict(path=name, flags=flags, result=int(result)))
            if int(result) >= 0:
                descriptors[int(result)] = (name, mode)
                if mode == 'read':
                    read_opened.add(name)
        elif call in ('read', 'write', 'close', 'fchmod'):
            fd = int(arguments.split(',', 1)[0])
            if call == 'write' and fd in (1, 2):
                continue
            require(fd in descriptors, f'guest used undeclared file descriptor: {line}')
            name, mode = descriptors[fd]
            if call == 'read':
                require(mode == 'read', f'guest read its output: {line}')
            elif call in ('write', 'fchmod'):
                require(mode == 'write', f'guest changed its input: {line}')
            elif int(result) == 0:
                del descriptors[fd]
        elif call == 'unlinkat':
            require(output_name is not None and arguments == f'AT_FDCWD,"{output_name}",0',
                    f'guest unlinked undeclared path: {line}')
    require(read_opened == set(read_names), f'compiler did not open all declared inputs: {set(read_names)-read_opened}')
    require(syscall_counts.get('exit', 0) + syscall_counts.get('exit_group', 0) == 1,
            'guest did not exit normally')
    return dict(opens=opened, syscalls=syscall_counts, all_inputs_opened=True, no_guest_execve=True,
                no_guest_self_elf_reads=True)


def image_info(data):
    require(len(data) >= 64 and data[:7] == b'\x7fELF\x02\x01\x01', 'invalid ELF64 image')
    require(struct.unpack_from('<H', data, 18)[0] == 243, 'image is not RISC-V')
    offset = struct.unpack_from('<Q', data, 32)[0]
    stride, count = struct.unpack_from('<HH', data, 54)
    require(stride == 56 and 0 < count <= 16 and offset+stride*count <= len(data), 'invalid ELF headers')
    for i in range(count):
        kind, flags, off, va, pa, filesz, memsz, align = struct.unpack_from('<IIQQQQQQ', data, offset+i*stride)
        if kind == 1:
            require(filesz <= memsz and off+filesz <= len(data), 'invalid ELF load segment')
    usage = memory(data)
    require(usage['rw_bytes'] <= LIMIT and usage['image_span_bytes'] <= LIMIT, '64 MiB image budget exceeded')
    return dict(sha256=sha(data), elf_bytes=len(data), memory=usage)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--p3-record', type=Path, required=True)
    parser.add_argument('--seed', type=Path, required=True)
    parser.add_argument('--q-manifest', type=Path, required=True)
    parser.add_argument('--record-dir', type=Path, required=True)
    args = parser.parse_args()
    record = args.record_dir.resolve()
    record.mkdir(parents=True, exist_ok=False)
    evidence = dict(format=1, stage='P4', mechanical_checks_passed=False,
                    independently_accepted=False, language_cases_required=True,
                    guest_stack_bytes=STACK, stages=[])

    def save():
        (record/'verification.json').write_text(json.dumps(evidence, indent=2)+'\n')

    try:
        p3_dir = args.p3_record.resolve()
        p3_bytes = (p3_dir/'verification.json').read_bytes()
        p3 = json.loads(p3_bytes)
        require(p3.get('stage') == 'P3' and p3.get('failed') == [], 'upstream is not a completed P3 verification')
        cases = p3.get('cases', [])
        require(cases and all(case.get('passed') is True for case in cases) and p3.get('passed') == len(cases),
                'P3 case evidence is incomplete')
        require(len(p3.get('retained_target_tests', [])) == 11 and
                len(set(p3['retained_target_tests'])) == 11, 'P3 retained target checks are incomplete')
        require(len({case['name'] for case in cases}) == len(cases), 'P3 case names are duplicated')
        candidate = capsule(p3_dir/'candidate-source.tar')
        expected = {**p3['source_sha256'], **p3['verification_inputs']}
        require(all(expected[name] == digest for name,digest in p3['source_sha256'].items()),
                'P3 source/verification hashes disagree')
        require(hashes(candidate) == expected, 'P3 candidate capsule identity mismatch')
        current_files(CORE, expected)
        require(sha((p3_dir/'compiler.elf').read_bytes()) == p3['compiler_sha256'], 'P3 ELF identity mismatch')
        baseline_bytes = candidate['bootstrap/transition/seed.json']
        baseline = json.loads(baseline_bytes)
        identity = sha(json.dumps(baseline['source_sha256'], sort_keys=True, separators=(',', ':')).encode())
        require(identity == baseline['source_identity'] == p3['seed_source_identity'], 'frozen source identity mismatch')
        seed = args.seed.resolve().read_bytes()
        require(sha(seed) == baseline['compiler_sha256'] == p3['seed_sha256'], 'wrong frozen B seed')
        frozen = current_files(REPO, baseline['source_sha256'])
        require(hashes(capsule(p3_dir/'seed-source.tar')) == hashes(frozen), 'P3 seed capsule identity mismatch')
        p_names, p_manifest, _ = manifest(candidate['bootstrap/transition/compiler.build'])
        require(set(p_names) == set(p3['source_sha256']), 'P3 source list differs from its manifest')
        q_manifest_path = args.q_manifest.resolve()
        require(q_manifest_path.is_relative_to(CORE), 'Q manifest must be in the repository')
        q_manifest_name = str(q_manifest_path.relative_to(CORE))
        q_original = q_manifest_path.read_bytes()
        q_names, q_manifest, q_entry = manifest(q_original)
        require(q_manifest_name not in q_names, 'Q manifest is also a source')
        q_sources = {}
        for name in q_names:
            path = CORE/name
            require(path.resolve().is_relative_to(CORE), f'Q input escapes the repository: {name}')
            q_sources[name] = path.read_bytes()
        q_snapshot = {**q_sources, q_manifest_name:q_original}
        module = q_entry.rsplit('::', 1)[0]
        mains = [(name, data) for name, data in q_sources.items()
                 if re.search(rb'(?m)^\s*module\s+'+re.escape(module.encode())+rb'\s*;', data)]
        require(len(mains) == 1, 'Q entry module must identify one source for behavior perturbation')
        main_name, main_source = mains[0]
        prefixes = re.findall(rb'\btext\("(S-next[^"\\\r\n]*)"\)', main_source)
        require(len(prefixes) == 1 and main_source.count(b'"'+prefixes[0]+b'"') == 1,
                'Q main must contain one literal text("S-next...") statistics prefix')
        prefix = prefixes[0]
        marker = b'S-next-P4-PERTURBED-'+sha(main_source)[:12].encode()
        require(all(marker not in data for data in q_sources.values()), 'perturbation marker already exists')
        mutated = {**q_sources, main_name:main_source.replace(b'"'+prefix+b'"', b'"'+marker+b'"', 1)}
        qemu = shutil.which('qemu-riscv64')
        require(qemu is not None, 'qemu-riscv64 is required')
        qemu = str(Path(qemu).resolve())
        evidence.update(seed_sha256=sha(seed), seed_source_identity=identity,
                        p3_record_sha256=sha(p3_bytes), p3_candidate_sha256=p3['compiler_sha256'],
                        p_source_sha256=p3['source_sha256'], q_source_sha256=hashes(q_snapshot),
                        qemu=dict(path=qemu, version=subprocess.check_output([qemu, '--version'], text=True).splitlines()[0]),
                        tool_sha256=hashes({'tools/selfhost_next.py':Path(__file__).read_bytes(),
                                           'tools/verify.py':(CORE/'tools/verify.py').read_bytes(),
                                           'bootstrap/frozen/core-s/tools/verify.py':(REPO/'core-s/tools/verify.py').read_bytes()}))
        (record/'p3-verification.json').write_bytes(p3_bytes)
        (record/'seed.json').write_bytes(baseline_bytes)
        (record/'b.elf').write_bytes(seed)
        (record/'b.elf').chmod(0o755)
        write_capsule(record/'tool-source.tar', {name:(CORE/name).read_bytes() for name in evidence['tool_sha256']})
        write_capsule(record/'p3-candidate-source.tar', candidate)
        write_capsule(record/'seed-source.tar', frozen)
        write_capsule(record/'q-source.tar', q_snapshot)
        write_capsule(record/'perturbed-source.tar', {**mutated, q_manifest_name:q_original})
        (record/'logs').mkdir()
        save()

        def run_stage(label, driver, files, build_manifest, mode='build'):
            print(f'{label}: {mode} with {sha(driver)[:16]}', flush=True)
            state = dict(name=label, compiler_sha256=sha(driver), mode=mode,
                         input_sha256=hashes({**files, 'compiler.build':build_manifest}))
            evidence['stages'].append(state)
            save()
            with tempfile.TemporaryDirectory(prefix='snext-p4-') as directory:
                work = Path(directory)
                for name, data in files.items():
                    target = work/relative(name)
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(data)
                (work/'compiler.build').write_bytes(build_manifest)
                (work/'driver.elf').write_bytes(driver)
                (work/'driver.elf').chmod(0o755)
                trace_path = record/'logs'/f'{label}.strace'
                command = [qemu, '-s', str(STACK), '-strace', '-D', str(trace_path), 'driver.elf', mode, 'compiler.build']
                state.update(command=command, environment={'PATH':''}, cwd=str(work))
                save()
                started = time.monotonic()
                try:
                    result = subprocess.run(command, cwd=work, env={'PATH':''}, stdin=subprocess.DEVNULL,
                                            capture_output=True, text=True, timeout=900)
                except subprocess.TimeoutExpired as error:
                    state.update(timed_out=True, elapsed_seconds=time.monotonic()-started)
                    for suffix, data in [('stdout',error.stdout), ('stderr',error.stderr)]:
                        (record/'logs'/f'{label}.{suffix}').write_bytes(data.encode() if isinstance(data,str) else data or b'')
                    save()
                    raise
                state.update(exit=result.returncode, elapsed_seconds=time.monotonic()-started,
                             stdout=result.stdout, stderr=result.stderr,
                             statistics={key:int(value) for key,value in re.findall(r'\b([a-z_]+)=(\d+)\b', result.stderr)})
                (record/'logs'/f'{label}.stdout').write_text(result.stdout)
                (record/'logs'/f'{label}.stderr').write_text(result.stderr)
                save()
                require(result.returncode == 0, f'{label} failed: {result.stderr}')
                state['access'] = trace_contract(trace_path.read_text(), set(files)|{'compiler.build'},
                                                 'compiler.elf' if mode == 'build' else None)
                require('max_frame' in state['statistics'], f'{label} emitted no frame statistics')
                require(state['statistics']['max_frame'] <= 262144, f'{label} exceeded existing maximum frame gate')
                require(hashes({name:(work/name).read_bytes() for name in files}) == hashes(files), 'guest changed source inputs')
                require((work/'driver.elf').read_bytes() == driver, 'guest changed its executable')
                require((work/'compiler.build').read_bytes() == build_manifest, 'guest changed manifest')
                if mode == 'check':
                    require(not (work/'compiler.elf').exists(), 'check published an ELF')
                    save()
                    return result.stderr
                binary = (work/'compiler.elf').read_bytes()
                state['output'] = image_info(binary)
                (record/f'{label}.elf').write_bytes(binary)
                (record/f'{label}.elf').chmod(0o755)
                save()
                return binary

        e0 = run_stage('e0', seed, {name:candidate[name] for name in p_names}, p_manifest)
        require(sha(e0) == p3['compiler_sha256'], 'B(P) did not reproduce accepted P3 E0')
        e1 = run_stage('e1', e0, q_sources, q_manifest)
        e2 = run_stage('e2', e1, q_sources, q_manifest)
        e3 = run_stage('e3', e2, q_sources, q_manifest)
        require(e2 == e3, 'E2 and E3 differ as complete ELF files')
        evidence.update(e2_equals_e3=True, sha256={name:sha(data) for name,data in [('e0',e0),('e1',e1),('e2',e2),('e3',e3)]})
        sample = {'sample.vt':b'module proof::sample;\nfn run() -> u32 { return 42; }\n'}
        sample_manifest = b'format 1\nprofile linux-user\nentry proof::sample::run\nsource sample.vt\noutput compiler.elf\n'
        original_behavior = run_stage('baseline-check', e3, sample, sample_manifest, 'check')
        require(prefix.decode()+' sources=' in original_behavior and marker.decode() not in original_behavior,
                'unperturbed compiler did not report its declared statistics prefix')
        changed = run_stage('perturbed', e3, mutated, q_manifest)
        require(changed != e3, 'source mutation did not change compiler image')
        changed_behavior = run_stage('perturbed-check', changed, sample, sample_manifest, 'check')
        require(marker.decode()+' sources=' in changed_behavior and prefix.decode()+' sources=' not in changed_behavior,
                'source mutation did not cause the expected observable behavior')
        evidence['perturbation'] = dict(source=main_name, before_sha256=sha(main_source),
                                        after_sha256=sha(mutated[main_name]), old_prefix=prefix.decode(),
                                        new_prefix=marker.decode(), behavior_observed=True,
                                        sample_sha256=hashes(sample), sample_manifest=sample_manifest.decode())
        current_files(CORE, expected)
        current_files(CORE, hashes(q_snapshot))
        current_files(REPO, baseline['source_sha256'])
        require(sha(args.seed.resolve().read_bytes()) == sha(seed), 'seed changed during verification')
        current_files(CORE, evidence['tool_sha256'])
        require((p3_dir/'verification.json').read_bytes() == p3_bytes, 'P3 record changed during verification')
        evidence['capsule_sha256'] = {name:sha((record/name).read_bytes()) for name in
                                     ['p3-candidate-source.tar','seed-source.tar','q-source.tar','perturbed-source.tar','tool-source.tar']}
        evidence['mechanical_checks_passed'] = True
        save()
        print(f'P4 mechanical evidence complete: {record}/verification.json\nIndependent language/architecture acceptance is still required.', flush=True)
    except Exception as error:
        evidence['failure'] = f'{type(error).__name__}: {error}'
        save()
        raise


if __name__ == '__main__':
    main()
