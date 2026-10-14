"""Independent final CLI probes; use a built seed and temporary inputs only."""
from pathlib import Path
import os
import subprocess
import tempfile
seed=(Path.cwd()/'core-s/target/debug/core-s').resolve()
with tempfile.TemporaryDirectory(prefix='b2-i-review-') as tmp:
    root=Path(tmp)
    (root/'main.vt').write_text('module app; import lib; pub fn run()->u32{return lib::value();}')
    (root/'library.vt').write_text('module lib; extern abi(C) fn platform()->u32; pub fn value()->u32{unsafe{return platform();}}')
    (root/'provider.s0').write_text('platform:\naddi x10,x0,17\njalr x0,0(x1)\n')
    manifest='format 1\nprofile linux-user\nentry root::app::run\nsource main.vt\nsource library.vt\nassembly provider.s0\noutput result.elf\n'
    (root/'project').write_text(manifest)
    def build():
        return subprocess.run([str(seed),'project',str(root/'project'),'--root',str(root)],cwd='/',env=dict(os.environ,PATH=''),capture_output=True,text=True,timeout=10)
    out=build();assert out.returncode==0,out.stderr
    assert subprocess.run(['qemu-riscv64',str(root/'result.elf')],timeout=10).returncode==17
    (root/'result.elf').unlink()
    for input in ['main.vt','library.vt','provider.s0','project']:
        previous=(root/input).read_bytes()
        (root/'result.elf').symlink_to(input)
        out=build();assert out.returncode!=0 and 'output aliases input' in out.stderr,(input,out.stderr)
        assert (root/input).read_bytes()==previous and (root/'result.elf').is_symlink()
        (root/'result.elf').unlink()
    original=(root/'library.vt').read_text()
    (root/'result.elf').write_bytes(b'old artifact')
    (root/'library.vt').write_text(original+'\nextern abi(C) fn missing_provider()->u32;\n')
    out=build();assert out.returncode!=0 and 'I_LINK' in out.stderr and 'missing_provider' in out.stderr,out.stderr
    assert (root/'result.elf').read_bytes()==b'old artifact'
    (root/'library.vt').write_text('module lib;\npub fn value()->u32{\nreturn false;\n}')
    out=build();assert out.returncode!=0 and 'library.vt:3:' in out.stderr,out.stderr
    assert (root/'result.elf').read_bytes()==b'old artifact'
    (root/'library.vt').write_text(original)
    (root/'other.s0').write_text((root/'provider.s0').read_text())
    (root/'project').write_text(manifest+'assembly other.s0\n')
    out=build();assert out.returncode!=0 and 'duplicate symbol platform' in out.stderr,out.stderr
    assert (root/'result.elf').read_bytes()==b'old artifact'
    assert not list(root.glob('.core-s-*.tmp'))
print('Independent I CLI: root-qualified entry, empty PATH, four symlink aliases, unused extern, source-line diagnostic, duplicate providers and artifact preservation passed.')
