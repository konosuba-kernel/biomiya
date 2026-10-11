#!/usr/bin/env python3
"""N3 focused C0 build, one target self-compile, and fresh behavior probes."""
import hashlib,json,os,shutil,struct,subprocess,tempfile,time
from pathlib import Path
root=Path(__file__).resolve().parents[3];core=root/'core-s';qemu=shutil.which('qemu-riscv64')
assert qemu
manifest=(core/'bootstrap/c1.build').read_text()
inputs=[line.split(' ',1)[1] for line in manifest.splitlines() if line.startswith(('source ','assembly '))]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def run(args,cwd):
 started=time.monotonic();r=subprocess.run(args,cwd=cwd,env={**os.environ,'PATH':''},capture_output=True,text=True,timeout=180)
 return r,{'args':args,'exit':r.returncode,'seconds':round(time.monotonic()-started,3),'stderr':r.stderr}
with tempfile.TemporaryDirectory(prefix='b31-n3-integration-') as tmp:
 p=Path(tmp)
 for name in inputs:
  dest=p/name;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(core/name,dest)
 (p/'c1.build').write_text(manifest.replace('output build/c1.elf','output c1.elf'))
 (p/'c2.build').write_text(manifest.replace('output build/c1.elf','output c2.elf'))
 r,c0=run([str(core/'target/debug/core-s'),'project','c1.build'],p);assert r.returncode==0,r.stderr
 r,selfbuild=run([qemu,'-s','1048576','c1.elf','build','c2.build'],p);assert r.returncode==0,r.stderr
 results={}
 cases={
  'cleanup':('static mut n:u32=0;fn f()->u32{var x:u32=13;defer{unsafe{n=5;}x=90;}return x;}pub fn run()->u32{let x:u32=f();unsafe{return x+n;}}',18),
  'rules':('const C:u8=256_u32 as u8;static S:u8=1_u8<<7_u8;pub fn run()->u32{return C as u32+S as u32;}',128),
  'invalid_defer':('pub fn run()->u32{defer{return 0;}return 0;}',None),
  'invalid_static_tail':('static S:[2]u32=[1_u32,true];pub fn run()->u32{return 0;}',None),
  'invalid_zero_repeat':('static S:[0]u32=[1_u32/0_u32;0];pub fn run()->u32{return 0;}',None)
 }
 for name,(source,expected) in cases.items():
  (p/(name+'.vt')).write_text(source)
  (p/(name+'.build')).write_text(f'format 1\nprofile linux-user\nentry run\nsource {name}.vt\noutput {name}.elf\n')
  r,built=run([qemu,'-s','1048576','c2.elf','build',name+'.build'],p)
  if expected is None:
   assert r.returncode==1 and 'E_TYPE' in r.stderr and not (p/(name+'.elf')).exists(),built
   results[name]={'compile':built,'no_output':True}
  else:
   assert r.returncode==0,built
   r,executed=run([qemu,'-s','1048576',name+'.elf'],p);assert r.returncode==expected,executed
   results[name]={'compile':built,'execute':executed,'expected':expected}
 data=(p/'c2.elf').read_bytes();phoff=struct.unpack_from('<Q',data,32)[0];entsize,count=struct.unpack_from('<HH',data,54)
 loads=[struct.unpack_from('<IIQQQQQQ',data,phoff+i*entsize) for i in range(count)]
 loads=[s for s in loads if s[0]==1]
 report={'input_hashes':{name:sha(p/name) for name in inputs},'c0':c0,'target_self_compile':selfbuild,'binaries':{'c1':sha(p/'c1.elf'),'c2':sha(p/'c2.elf')},'metrics':{'elf_bytes':len(data),'rw_memory_bytes':sum(s[6] for s in loads if s[1]&2),'image_span':max(s[3]+s[6] for s in loads)-min(s[3] for s in loads)},'probes':results,'scope':'One target generation only; full fixed point/old seed/clean build are N5.'}
 print(json.dumps(report,indent=2))
