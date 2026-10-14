#!/usr/bin/env python3
# Independent review variant: alternate EINTR and one-byte writes, max-width diagnostics.
import pathlib,tempfile,subprocess,re,json,sys
root=pathlib.Path(__file__).resolve().parents[4]/'core-s'
model=(root/'bootstrap/model.vt').read_text()
index=len(re.findall(r'^pub static mut ',model,re.M))
results={}
with tempfile.TemporaryDirectory(prefix='b3-h2-') as d:
 p=pathlib.Path(d)
 (p/'model.vt').write_text(model)
 call='model::limit_at(18446744073709551615,4194304,42,4096)' if len(sys.argv)>1 and sys.argv[1]=='limit' else 'model::fail(6,4294967295,18446744073709551615)'
 expected=b'actual=18446744073709551615 limit=4194304 I_LIMIT file=42 byte=4096\n' if 'limit_at' in call else b'I_LIMIT file=4294967295 byte=18446744073709551615\n'
 (p/'probe.vt').write_text('module probe; import bootstrap::model; static mut counter:usize=0; pub fn start()->u32 { '+call+'; return 0; }')
 for mode in ['short_eintr','zero','error','oversize']:
  if mode=='short_eintr':
   body=f'''address x5,__core_s_g{index}
ld x6,0(x5)
addi x6,x6,1
sd x6,0(x5)
addi x7,x0,1
and x6,x6,x7
beq x6,x0,.Lshort
addi x10,x0,-4
jalr x0,0(x1)
.Lshort:
addi x13,x0,1
jump .Lreal
'''
  elif mode=='zero': body='addi x10,x0,0\njalr x0,0(x1)\n'
  elif mode=='error': body='addi x10,x0,-5\njalr x0,0(x1)\n'
  else: body='addi x10,x13,1\njalr x0,0(x1)\n'
  shim='''__b2_syscall6:
addi x5,x0,64
bne x10,x5,.Lreal
'''+body+'''.Lreal:
addi x17,x10,0
addi x10,x11,0
addi x11,x12,0
addi x12,x13,0
addi x13,x14,0
addi x14,x15,0
addi x15,x16,0
ecall
jalr x0,0(x1)
'''
  (p/'shim.s0').write_text(shim)
  (p/'test.build').write_text('format 1\nprofile linux-user\nentry probe::start\nsource model.vt\nsource probe.vt\nassembly shim.s0\noutput '+mode+'.elf\n')
  subprocess.run([str(root/'target/debug/core-s'),'project','test.build'],cwd=p,check=True)
  run=subprocess.run(['qemu-riscv64',str(p/(mode+'.elf'))],capture_output=True,timeout=10)
  assert run.returncode==1,(mode,run.returncode,run.stderr)
  if mode=='short_eintr':assert run.stderr==expected,run.stderr
  else:assert run.stderr==b'',run.stderr
  results[mode]={'exit':run.returncode,'stderr':run.stderr.decode()}
print(json.dumps(results,indent=2))
