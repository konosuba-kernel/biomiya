#!/usr/bin/env python3
"""Audit final N1 changes: verify each IR wrapper and invert its call sites,
then expand newly named constants and compare old B3 lexical tokens.
This complements execution tests; it is not independent node acceptance.
"""
import hashlib, importlib.util, json, re, subprocess
from pathlib import Path
root=Path(__file__).resolve().parents[3]
spec=importlib.util.spec_from_file_location('format_vt',Path(__file__).with_name('N1-format.py'))
f=importlib.util.module_from_spec(spec);spec.loader.exec_module(f)
model=(root/'core-s/bootstrap/model.vt').read_text()
base_model=subprocess.check_output(['git','show','1c0f143:core-s/bootstrap/model.vt'],cwd=root,text=True)
pattern=r'pub const ([A-Z_0-9]+):u32 = (\d+);'
old_names={k for k,v in re.findall(pattern,base_model)}
constants={k:v for k,v in re.findall(pattern,model) if k not in old_names}
def code_tokens(s):return [t for t in f.tokens(s) if not t.startswith('//') and not t.startswith('/*')]
def normalize(s,expand):
 if expand:
  s=re.sub(pattern,lambda m:'' if m[1] in constants else m[0],s)
  s=re.sub(r'\b(?:model::)?([A-Z][A-Z_0-9]+)\b',lambda m:constants.get(m[1],m[0]),s)
 return [re.sub(r'_(?:u8|u32|usize)$','',t) if t[0].isdigit() else t for t in code_tokens(s)]
expected='''
pub fn copy_to(destination:u32, source:u32) -> () {
 emit(model::IR_COPY, destination, source, 0, 0, 0, 0); return ();
}
pub fn bounds(index:u32, length:u32) -> () {
 emit(model::IR_BOUNDS, 0, index, length, 0, 0, 0); return ();
}
pub fn call(destination:u32, declaration:u32, first:u32, count:u32) -> () {
 emit(model::IR_CALL, destination, declaration, first, count, 0, 0); return ();
}
pub fn return_value(value:u32) -> () {
 emit(model::IR_RETURN, 0, value, 0, 0, 0, 0); return ();
}
pub fn panic_() -> () {
 emit(model::IR_PANIC, 0, 0, 0, 0, 0, 0); return ();
}
pub fn unary(op:u32, operand:u32, result_type:u32) -> u32 {
 let result:u32 = slot(result_type);
 emit(model::IR_UNARY, result, operand, 0, 0, op, 0); return result;
}
'''
report={}
for p in sorted((root/'core-s/bootstrap').glob('*.vt')):
 s=p.read_text();rel=str(p.relative_to(root));source_hash=f.digest(s)
 if p.name=='ir.vt':
  cut=s.index('pub fn copy_to(')
  assert code_tokens(s[cut:])==code_tokens(expected),'IR helper bodies changed'
  s=s[:cut].replace('copy_to(s, source);','emit(model::IR_COPY, s, source, 0, 0, 0, 0);')
 if p.name=='check.vt':
  s=re.sub(r'ir::copy_to\((\w+), (.*?)\);',r'ir::emit(model::IR_COPY, \1, \2, 0, 0, 0, 0);',s)
  s=s.replace('ir::bounds(ix, length);','ir::emit(model::IR_BOUNDS, 0, ix, length, 0, 0, 0);')
  s=re.sub(r'ir::return_value\((\w+)\);',r'ir::emit(model::IR_RETURN, 0, \1, 0, 0, 0, 0);',s)
  s=s.replace('ir::call(result, id, begin, d.count);','ir::emit(model::IR_CALL, result, id, begin, d.count, 0, 0);')
  s=s.replace('ir::panic_();','ir::emit(model::IR_PANIC, 0, 0, 0, 0, 0, 0);')
  s=s.replace('value = ir::unary(n.a, operand, ty);','value = ir::slot(ty); ir::emit(model::IR_UNARY, value, operand, 0, 0, n.a, 0);')
 baseline=subprocess.check_output(['git','show','1c0f143:'+rel],cwd=root,text=True)
 a=normalize(baseline,False);b=normalize(s,True)
 if a!=b:
  for i,(x,y) in enumerate(zip(a,b)):
   if x!=y:print(rel,i,a[max(0,i-8):i+8],b[max(0,i-8):i+8]);break
  raise SystemExit('Unexpected change after expansion')
 report[rel]={'final_sha256':source_hash,'expanded_tokens_equal':True,'token_count':len(a)}
print(json.dumps({'helper_bodies_verified':True,'files':report},indent=2))
