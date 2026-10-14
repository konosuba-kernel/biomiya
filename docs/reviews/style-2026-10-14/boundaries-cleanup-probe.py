#!/usr/bin/env python3
"""Inject write then unlink failures into copied syscall adapter; never modify production."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[3]
Q = ROOT / 'core-s/next/q'
COMPILER = ROOT / 'core-s/build/snext-p4-r2/e3.elf'
SOURCE = '''module cleanup_probe;
import bootstrap::model;
import bootstrap::input;
pub fn run() -> u32 {
    model::reset_all();
    input::load("input.build");
    input::save(input::source(1).bytes);
    return 0;
}
'''
SHIM = '''# Test only: deny unlink; first 32-byte write writes seven bytes, next write fails.
__b2_syscall6:
addi x5,x0,35
beq x10,x5,.Ldeny
addi x5,x0,64
bne x10,x5,.Lreal
sltiu x5,x11,3
bne x5,x0,.Lreal
addi x5,x0,32
bne x13,x5,.Lfail
addi x13,x0,7
jump __b2_real
.Lfail:
addi x10,x0,-5
jalr x0,0(x1)
.Ldeny:
addi x10,x0,-13
jalr x0,0(x1)
.Lreal:
jump __b2_real
'''
def digest(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def run(args,cwd):
    p = subprocess.run(args,cwd=cwd,capture_output=True,text=True,timeout=30)
    return {'argv':args,'returncode':p.returncode,'stdout':p.stdout,'stderr':p.stderr}
with tempfile.TemporaryDirectory(prefix='suba-style-cleanup-') as name:
    work=Path(name)
    sources=['model.vt','input.vt','runtime/linux.vt','runtime/bytes.vt']
    for source in sources:
        shutil.copy2(Q/source,work/Path(source).name)
    (work/'shim.s0').write_text(SHIM+(Q/'runtime/linux.s0').read_text().replace('__b2_syscall6:', '__b2_real:', 1))
    (work/'probe.vt').write_text(SOURCE)
    (work/'probe.build').write_text('format 1\nprofile linux-user\nentry cleanup_probe::run\nsource probe.vt\n'+''.join('source '+Path(s).name+'\n' for s in sources)+'assembly shim.s0\noutput probe.elf\n')
    (work/'input.build').write_text('format 1\nprofile linux-user\nentry cleanup_probe::run\nsource payload\noutput partial.elf\n')
    (work/'payload').write_bytes(b'0123456789abcdef0123456789abcdef')
    result={'compiler_sha256':digest(COMPILER),'inputs':{str((Q/s).relative_to(ROOT)):digest(Q/s) for s in sources+['runtime/linux.s0']}, 'source':SOURCE,'shim':SHIM}
    result['compile']=run(['qemu-riscv64',str(COMPILER),'build','probe.build'],work)
    assert result['compile']['returncode']==0,result
    result['probe_sha256']=digest(work/'probe.elf')
    result['run']=run(['qemu-riscv64','probe.elf'],work)
    partial=work/'partial.elf'
    result['partial_exists']=partial.exists()
    result['partial_bytes']=partial.read_bytes().decode() if partial.exists() else None
    result['partial_mode']=oct(partial.stat().st_mode & 0o777) if partial.exists() else None
    assert result['run']['returncode']==1 and result['partial_bytes']=='0123456'
    assert result['run']['stderr']=='E_IO file=0 byte=0\n'
    result['scope']='Injected external failures confirm cleanup-result diagnostic loss; does not demand impossible cleanup when filesystem rejects unlink.'
    print(json.dumps(result,indent=2))
