#!/usr/bin/env python3
"""Small black-box N2 probes plus two discriminating temporary mutants.
Only temporary copies are modified. No full self-host or clean build claim.
"""
from pathlib import Path
import hashlib,json,re,shutil,subprocess,tempfile
root=Path(__file__).resolve().parents[3]
core=root/'core-s'
files=['bootstrap/'+n+'.vt' for n in ['model','lex','input','types','parse','ir','check']]+['runtime/bytes.vt','runtime/linux.vt']
probe=re.search(r'const PROBE: &str = r#"(.*?)"#;', (core/'tests/b3_frontend.rs').read_text(),re.S)[1]
results={}
for variant in ['current','missing_diagnostic_restore','checker_moves_token']:
 with tempfile.TemporaryDirectory(prefix='b31-n2-probe-') as tmp:
  p=Path(tmp)
  for name in files+['runtime/linux.s0']:
   dest=p/name;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(core/name,dest)
  if variant=='missing_diagnostic_restore':
   q=p/'bootstrap/types.vt';q.write_text(q.read_text().replace('        diagnostic = saved_diagnostic;',''))
  if variant=='checker_moves_token':
   q=p/'bootstrap/check.vt';q.write_text(q.read_text().replace('    types::locate(at);','    types::locate(at);\n    unsafe { types::token.file = at.file; types::token.start = at.start; }'))
  (p/'probe.vt').write_text(probe)
  (p/'probe.build').write_text('format 1\nprofile linux-user\nentry probe::run\noutput probe.elf\n'+''.join('source '+n+'\n' for n in files+['probe.vt'])+'assembly runtime/linux.s0\n')
  built=subprocess.run([str(core/'target/debug/core-s'),'project','probe.build'],cwd=p,capture_output=True,text=True,timeout=30)
  assert built.returncode==0,built.stderr
  cases={
   'cursor':'pub fn run()->u32{return 0;}',
   'layout':'struct Outer{bad:[67108865]Inner}struct Inner{v:u8}pub fn run()->u32{return 0;}',
   'constant':'const A:u32=B; const B:bool=true;pub fn run()->u32{return 0;}'
  }
  records={}
  for name,source in cases.items():
   (p/'main.vt').write_text(source)
   (p/'test.build').write_text('format 1\nprofile linux-user\nentry run\noutput unused.elf\nsource main.vt\n')
   r=subprocess.run(['qemu-riscv64','-s','1048576','probe.elf','test.build'],cwd=p,capture_output=True,text=True,timeout=30)
   records[name]={'exit':r.returncode,'stderr':r.stderr}
  results[variant]=records
assert results['current']['cursor']['exit']==0
assert results['checker_moves_token']['cursor']['exit']==9
for case,marker,code in [('layout','}struct','I_LIMIT'),('constant','; const B','E_TYPE')]:
 expect=f'{code} file=1 byte={cases[case].index(marker)}'
 assert expect in results['current'][case]['stderr']
 assert expect not in results['missing_diagnostic_restore'][case]['stderr']
print(json.dumps({'input_hashes':{n:hashlib.sha256((core/n).read_bytes()).hexdigest() for n in files},'results':results,'mutants_discriminated':True},indent=2))
