#!/usr/bin/env python3
"""Rebuild and run B2 from source only, without the archived semantic compiler."""
import hashlib,json,os,re,shutil,subprocess,tempfile
from pathlib import Path
root=Path(__file__).resolve().parents[3]
record=root/'docs/workflows/bootstrap-b2'
def run(args,**kwargs):
    p=subprocess.run(args,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=180,**kwargs)
    if p.returncode:raise RuntimeError(f'{args}: exit {p.returncode}\n{p.stdout}')
    return p.stdout
with tempfile.TemporaryDirectory(prefix='core-s-b2-clean-') as directory:
    dest=Path(directory)
    for name in ['src/source.rs','src/diagnostic.rs','src/syntax/lexer.rs','src/syntax/unicode.rs']:
        target=dest/name;target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(root/name,target)
    for name in ['src','tests','fixtures','runtime','compiler','app']:
        shutil.copytree(root/'core-s'/name,dest/'core-s'/name)
    for name in ['Cargo.toml','Cargo.lock']:shutil.copyfile(root/'core-s'/name,dest/'core-s'/name)
    # Do not inherit a shared target directory or compiler wrapper into this check.
    env=os.environ.copy()
    for key in ['CARGO_TARGET_DIR','RUSTC_WRAPPER','RUSTC_WORKSPACE_WRAPPER']:env.pop(key,None)
    output=run(['cargo','test','--offline','--locked','--manifest-path',str(dest/'core-s/Cargo.toml'),'--all-features'],env=env)
    (record/'I-r1-clean-tests.txt').write_text(output)
    count=sum(map(int,re.findall(r'test result: ok\. (\d+) passed',output)))
    assert count==61,count
    project=dest/'core-s';(project/'build').mkdir()
    seed=project/'target/debug/core-s';empty_path=dict(env,PATH='')
    run([str(seed),'project','app/copy.build'],cwd=project,env=empty_path)
    first=(project/'build/copy.elf').read_bytes();shutil.copyfile(project/'build/copy.elf',project/'build/clean.elf')
    run([str(root/'core-s/target/debug/core-s'),'project','app/copy.build'],cwd=project,env=empty_path)
    second=(project/'build/copy.elf').read_bytes();assert first==second
    run(['qemu-riscv64',str(project/'build/copy.elf'),'app/copy.manifest'],cwd=project)
    expected=b''.join((project/name).read_bytes() for name in ['compiler/source.vt','compiler/scan.vt','compiler/manifest.vt'])
    actual=(project/'build/compiler-sources.vt').read_bytes();assert actual==expected
    result={'node':'I','revision':1,'source_only_clean_build':True,'offline_locked_tests_passed':count,'production_build_PATH':'empty','workspace_and_clean_seed_elf_identical':True,'elf_sha256':hashlib.sha256(first).hexdigest(),'elf_file_bytes':len(first),'actual_target_copied_bytes':len(actual),'copied_sha256':hashlib.sha256(actual).hexdigest(),'source_input_policy':'Only core-s source/tests/fixtures/runtime/compiler/app and Cargo files plus source.rs,diagnostic.rs,lexer.rs,unicode.rs; no target or old semantic compiler copied.'}
    (record/'I-r1-clean-build.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
