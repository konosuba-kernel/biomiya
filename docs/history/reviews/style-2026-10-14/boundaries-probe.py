#!/usr/bin/env python3
"""Bounded isolated probe of Q runtime pathname decoding. Writes only a fresh temp dir."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[3]
Q = ROOT / 'core-s/next/q'
COMPILER = ROOT / 'core-s/build/snext-p4-r2/e3.elf'
SOURCE = '''module boundary_probe;
import runtime::linux;
pub fn run() -> u32 {
    let path:[8]u8 = [118,105,99,116,105,109,0,120];
    if linux::argc() == 1_usize {
        return linux::remove(core::slice_of(path));
    }
    match linux::open_read(core::slice_of(path)) {
        linux::CountResult::Value(fd) => { return linux::close(fd); },
        linux::CountResult::Failed(error) => { return error; },
    }
}
'''
def digest(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def run(args, cwd):
    p = subprocess.run(args, cwd=cwd, capture_output=True, text=True, timeout=30)
    return {'argv': args, 'returncode': p.returncode, 'stdout': p.stdout, 'stderr': p.stderr}

with tempfile.TemporaryDirectory(prefix='suba-style-boundary-') as name:
    work = Path(name)
    for p in ('linux.vt', 'linux.s0'):
        shutil.copy2(Q / 'runtime' / p, work / p)
    (work / 'probe.vt').write_text(SOURCE)
    (work / 'probe.build').write_text('format 1\nprofile linux-user\nentry boundary_probe::run\nsource probe.vt\nsource linux.vt\nassembly linux.s0\noutput probe.elf\n')
    result = {'compiler_sha256': digest(COMPILER), 'inputs': {str(p.relative_to(ROOT)): digest(p) for p in [Q/'runtime/linux.vt', Q/'runtime/linux.s0']}, 'source': SOURCE}
    result['compile'] = run(['qemu-riscv64', str(COMPILER), 'build', 'probe.build'], work)
    assert result['compile']['returncode'] == 0, result
    result['probe_sha256'] = digest(work/'probe.elf')
    (work/'victim').write_text('temporary probe sentinel\n')
    result['open_embedded_nul'] = run(['qemu-riscv64', 'probe.elf', 'open'], work)
    result['exists_after_open'] = (work/'victim').exists()
    result['remove_embedded_nul'] = run(['qemu-riscv64', 'probe.elf'], work)
    result['exists_after_remove'] = (work/'victim').exists()
    assert result['open_embedded_nul']['returncode'] == 11 and result['exists_after_open']
    assert result['remove_embedded_nul']['returncode'] == 0 and not result['exists_after_remove']
    result['observed_defect'] = True
    result['scope'] = 'Only fresh temporary victim file; production sources untouched. Successful probe confirms rejection mismatch, not compiler pipeline reachability.'
    print(json.dumps(result, indent=2))
