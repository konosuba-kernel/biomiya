#!/usr/bin/env python3
"""Independent C64 boundary/identity exercise; no author test model is imported."""
import argparse
import hashlib
import json
from pathlib import Path
import random
import subprocess


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--compiler', type=Path, required=True)
    p.add_argument('--baseline', type=Path, required=True)
    p.add_argument('--out', type=Path, required=True)
    a = p.parse_args()
    compiler, baseline = a.compiler.resolve(), a.baseline.resolve()
    out = a.out.resolve(); out.mkdir(parents=True, exist_ok=False)
    result = {'scope': 'independent native 64-source semantic/header coverage and rejection ordering',
              'compiler_sha256': sha(compiler.read_bytes()), 'baseline_sha256': sha(baseline.read_bytes()),
              'cases': [], 'passed': False}

    def case(name, files, manifest, driver=compiler, expect=None, existing=False):
        folder = out / name; folder.mkdir()
        for path, text in files.items():
            (folder/path).write_text(text)
        (folder/'test.build').write_text(manifest)
        before = {str(x.relative_to(folder)): sha(x.read_bytes()) for x in folder.iterdir() if x.is_file()}
        if existing:
            (folder/'result.elf').write_bytes(b'independent sentinel\x00\xff')
        command = ['qemu-riscv64', '-s', '1048576', str(driver), 'build', 'test.build']
        run = subprocess.run(command, cwd=folder, capture_output=True, timeout=30)
        item = {'name': name, 'command': command, 'exit': run.returncode,
                'stdout': run.stdout.decode(errors='replace'), 'stderr': run.stderr.decode(errors='replace'),
                'input_sha256': before}
        result['cases'].append(item)
        for path, digest in before.items():
            assert sha((folder/path).read_bytes()) == digest, (name, 'input mutation', path)
        if expect is not None:
            assert run.returncode != 0 and expect in run.stderr.decode(), item
            if existing:
                assert (folder/'result.elf').read_bytes() == b'independent sentinel\x00\xff'
            else:
                assert not (folder/'result.elf').exists()
        else:
            assert run.returncode == 0 and not run.stdout, item
            native = subprocess.run(['qemu-riscv64', '-s', '1048576', str(folder/'result.elf')],
                                    capture_output=True, timeout=10)
            item['native_exit'] = native.returncode
            item['elf_sha256'] = sha((folder/'result.elf').read_bytes())
            assert native.returncode == 42 and not native.stdout and not native.stderr, item
        item['passed'] = True

    try:
        # A field, local array length, type identity and function body in every source.
        files = {}
        checks = []
        for i in range(63):
            files[f'm{i:02}.vt'] = (f'module m{i:02};const N:usize={i%3+1};'
                'pub struct Opaque { value:u32 }'
                f'pub fn value()->u32{{var a:[N]u32=zero;a[N-1_usize]={i}_u32;'
                'let x:Opaque=make Opaque{value:a[N-1_usize]};return x.value;}')
            checks.append(f'if m{i:02}::value()!={i}_u32{{return 1;}}')
        files['entry.vt'] = ('module app;' + ''.join(f'import m{i:02};' for i in range(63)) +
                            'fn run()->u32{' + ''.join(checks) + 'return 42;}')
        order = list(range(63)); random.Random(64).shuffle(order)
        manifest = ('format 1\nprofile linux-user\nentry app::run\n' +
                    ''.join(f'source m{i:02}.vt\n' for i in order) +
                    'source entry.vt\noutput result.elf\n')
        case('all-64-shuffled-semantic-sources', files, manifest)
        case('old-compiler-rejects-same-program', files, manifest, driver=baseline,
             expect='actual=33 limit=32 I_LIMIT file=0')
        case('65-before-missing-file-preserves-existing-output', files,
             manifest+'source missing.vt\n', expect=f'actual=65 limit=64 I_LIMIT file=0 byte={len(manifest)}\n', existing=True)
        invalid = dict(files)
        invalid['entry.vt'] = 'module app;import m00;fn run()->u32{var x:m00::Opaque=zero;return 42;}'
        case('file-64-private-zero-still-rejected', invalid, manifest, expect='E_NAME file=64')
        # Every split-file header contributes a private constant used from the final file.
        shared = {f's{i:02}.vt': f'module shared;const V{i}:u32={i};' for i in range(63)}
        shared['entry.vt'] = ('module shared;fn run()->u32{' +
                             ''.join(f'if V{i}!={i}_u32{{return 1;}}' for i in range(63)) + 'return 42;}')
        shared_manifest = ('format 1\nprofile linux-user\nentry shared::run\n' +
                           ''.join(f'source s{i:02}.vt\n' for i in range(63)) +
                           'source entry.vt\noutput result.elf\n')
        case('64-split-module-private-constants', shared, shared_manifest)
        result['passed'] = True
    finally:
        (out/'verification.json').write_text(json.dumps(result, indent=2)+'\n')
    print('independent C64 cases passed:', len(result['cases']))


if __name__ == '__main__':
    main()
