#!/usr/bin/env python3
"""Rebuild the seed and exercise B3 from only declared source inputs."""
import argparse,hashlib,json,os,re,shutil,struct,subprocess,tempfile,time
from pathlib import Path
root=Path(__file__).resolve().parents[3]
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--record-dir',type=Path,help='new evidence directory; default is a fresh persistent /tmp directory')
args=parser.parse_args()
if args.record_dir:
    record=args.record_dir.resolve();record.mkdir(parents=True,exist_ok=False)
else:
    record=Path(tempfile.mkdtemp(prefix='core-s-b3-evidence-'))
print('Evidence directory:',record,flush=True)
def run(args,**kwargs):
    start=time.monotonic()
    p=subprocess.run(args,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=900,**kwargs)
    if p.returncode:raise RuntimeError(f'{args}: exit {p.returncode}\n{p.stdout}')
    return p.stdout,time.monotonic()-start
def elf_metrics(data):
    phoff=struct.unpack_from('<Q',data,32)[0]
    stride,count=struct.unpack_from('<HH',data,54)
    segments=[]
    for n in range(count):
        kind,flags,offset,vaddr,paddr,filesz,memsz,align=struct.unpack_from('<IIQQQQQQ',data,phoff+n*stride)
        if kind==1:
            segments.append(dict(flags=flags,file_bytes=filesz,memory_bytes=memsz,address=vaddr))
    rw=sum(s['memory_bytes'] for s in segments if s['flags'] & 2)
    span=max(s['address']+s['memory_bytes'] for s in segments)-min(s['address'] for s in segments)
    assert rw<=64*1024*1024 and span<=64*1024*1024
    return dict(segments=segments,writable_memory_bytes=rw,image_span_bytes=span,
                bss_bytes=sum(s['memory_bytes']-s['file_bytes'] for s in segments))
with tempfile.TemporaryDirectory(prefix='core-s-b3-clean-') as directory:
    dest=Path(directory)
    for name in ['src/source.rs','src/diagnostic.rs','src/syntax/lexer.rs','src/syntax/unicode.rs']:
        target=dest/name;target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(root/name,target)
    for name in ['src','tests','fixtures','runtime','compiler','app','bootstrap']:
        shutil.copytree(root/'core-s'/name,dest/'core-s'/name)
    for name in ['Cargo.toml','Cargo.lock']:shutil.copyfile(root/'core-s'/name,dest/'core-s'/name)
    env=os.environ.copy()
    for key in ['CARGO_TARGET_DIR','RUSTC_WRAPPER','RUSTC_WORKSPACE_WRAPPER']:env.pop(key,None)
    output,elapsed=run(['cargo','test','--offline','--locked','--manifest-path',str(dest/'core-s/Cargo.toml'),'--all-features'],env=env)
    (record/'I-clean-tests.txt').write_text(output)
    count=sum(map(int,re.findall(r'test result: ok\. (\d+) passed',output)))
    assert count==81,count
    project=dest/'core-s';(project/'build').mkdir(exist_ok=True)
    seed=project/'target/debug/core-s';empty_path=dict(env,PATH='')
    run([str(seed),'project','bootstrap/c1.build'],cwd=project,env=empty_path)
    c1=(project/'build/c1.elf').read_bytes()
    # Independent of the test harness: run the checked-in manifests with a 1MiB guest stack.
    qemu=shutil.which('qemu-riscv64');assert qemu
    stats=[];timings=[]
    for compiler,manifest in [('c1.elf','bootstrap/c2.build'),('c2.elf','bootstrap/c3.build')]:
        output,seconds=run([qemu,'-s','1048576',str(project/'build'/compiler),'build',manifest],cwd=project,env={'PATH':''})
        stats.append(output);timings.append(seconds)
    c2=(project/'build/c2.elf').read_bytes();c3=(project/'build/c3.elf').read_bytes();assert c2==c3
    result={'node':'I','source_only_clean_build':True,'offline_locked_tests_passed':count,'test_seconds':elapsed,'target_build_PATH':'empty','guest_stack_bytes':1048576,'c2_c3_byte_equal':True,'hashes':{name:hashlib.sha256(data).hexdigest() for name,data in [('c1',c1),('c2',c2),('c3',c3)]},'elf_file_bytes':{'c1':len(c1),'c2':len(c2),'c3':len(c3)},'generation_seconds':timings,'compiler_statistics':stats,'elf_memory':{name:elf_metrics(data) for name,data in [('c1',c1),('c2',c2),('c3',c3)]},'source_sha256':{str(p.relative_to(project)):hashlib.sha256(p.read_bytes()).hexdigest() for folder in ['bootstrap','runtime'] for p in sorted((project/folder).glob('*')) if p.is_file() and p.suffix in ['.vt','.s0','.build']},'source_input_policy':'Only core-s source/tests/fixtures/runtime/compiler/app/bootstrap and Cargo files plus source.rs,diagnostic.rs,lexer.rs,unicode.rs; no target or old semantic compiler copied.'}
    (record/'I-clean-build.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
