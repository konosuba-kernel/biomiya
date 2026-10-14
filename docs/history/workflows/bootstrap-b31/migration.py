#!/usr/bin/env python3
"""Verify B3 -> current source -> D1 -> D2 -> D3 in isolated target directories."""
import hashlib,json,os,pathlib,re,shutil,struct,subprocess,sys,tempfile
root=pathlib.Path(sys.argv[1]).resolve()
seed=pathlib.Path(sys.argv[2]).resolve()
evidence=pathlib.Path(sys.argv[3]).resolve(); evidence.mkdir(parents=True,exist_ok=False)
expected='8444edaad7dd0911dc9c0363965067c13e3b02209fbe6f8f70e0f1457296b33a'
sha=lambda b:hashlib.sha256(b).hexdigest()
assert sha(seed.read_bytes())==expected,'not the preserved B3 C2'
qemu=shutil.which('qemu-riscv64'); assert qemu
manifest=(root/'bootstrap/c1.build').read_text()
inputs=[line.split(maxsplit=1)[1] for line in manifest.splitlines() if line.startswith(('source ','assembly '))]
manifest=re.sub(r'^output .*$', 'output next.elf',manifest,flags=re.M)
results=[]; images=[]
with tempfile.TemporaryDirectory(prefix='b31-migration-') as base:
 compiler=seed.read_bytes()
 for generation in range(1,4):
  stage=pathlib.Path(base)/f'd{generation}';stage.mkdir()
  for name in inputs:
   dst=stage/name; dst.parent.mkdir(parents=True,exist_ok=True); shutil.copyfile(root/name,dst)
  (stage/'self.build').write_text(manifest)
  policy='bootstrap/pure.blocking'
  if generation==3: shutil.copyfile(root/policy,stage/policy)
  (stage/'compiler.elf').write_bytes(compiler); (stage/'compiler.elf').chmod(0o755)
  command=[qemu,'-s','1048576','-strace','compiler.elf','build','self.build']
  if generation==3: command+=['--blocking',policy]
  p=subprocess.run(command,cwd=stage,env={'PATH':''},capture_output=True,timeout=600)
  log=p.stderr.decode();(evidence/f'N5-d{generation}.log').write_text(log)
  assert p.returncode==0,log[-3000:]
  assert 'execve(' not in log and '"compiler.elf"' not in log
  opens=[line.split('"')[1] for line in log.splitlines() if 'openat(' in line]
  assert set(opens)<=set(inputs+['self.build','next.elf']+([policy] if generation==3 else [])),opens
  compiler=(stage/'next.elf').read_bytes();images.append(compiler)
  ph=struct.unpack_from('<Q',compiler,32)[0]; n=struct.unpack_from('<H',compiler,56)[0]
  rw=0;span=0
  for i in range(n):
   at=ph+56*i;flags=struct.unpack_from('<I',compiler,at+4)[0]; addr=struct.unpack_from('<Q',compiler,at+16)[0]; mem=struct.unpack_from('<Q',compiler,at+40)[0]
   if flags&2: rw+=mem
   span=max(span,addr+mem-0x10000)
  assert max(rw,span)<=64*1024*1024
  chunks=[]
  for match in re.finditer(r'\d+ write\(2,0x[0-9a-f]+,(\d+)\)',log):
   length=int(match[1]);payload=log[match.end():match.end()+length]
   assert log[match.end()+length:].startswith(f' = {length}\n')
   chunks.append(payload)
  statistics=''.join(chunks)
  assert int(re.search(r'max_frame=(\d+)',statistics)[1])<=262144
  if generation==3:
   assert statistics.count(': Verified\n')==6 and ': Violation' not in statistics and ': Unknown' not in statistics
  results.append({'blocking_enabled':generation==3,'generation':generation,'sha256':sha(compiler),'elf_bytes':len(compiler),'rw_bytes':rw,'image_span':span,'allowed_opens':sorted(set(opens)),'statistics':statistics,'max_frame':int(re.search(r'max_frame=(\d+)',statistics)[1])})
  print(json.dumps(results[-1]),flush=True)
 assert images[1]==images[2],'D2 != D3'
 (evidence/'N5-migration.json').write_text(json.dumps({'seed_sha256':expected,'source_hashes':{n:sha((root/n).read_bytes()) for n in inputs+['bootstrap/pure.blocking']},'generations':results,'d1_equals_d2':images[0]==images[1],'d2_equals_d3':True,'guest_stack':1048576,'environment':{'PATH':''},'host_delegation':False},indent=2)+'\n')
 # Leave the verified final executable in ignored build storage, outside source/evidence.
 (root/'build').mkdir(exist_ok=True)
 (root/'build/b31-final.elf').write_bytes(images[2]);(root/'build/b31-final.elf').chmod(0o755)
