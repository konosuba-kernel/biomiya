#!/usr/bin/env python3
"""Verify symbolic-only phase by expanding new u32 constants to old literals.
Ignores numeric type suffixes because named constants supply the same u32 type.
This check precedes IR helper extraction; its input snapshot is archived beside it.
"""
import hashlib, json, re, subprocess, sys, tarfile
from pathlib import Path
import importlib.util
root=Path(__file__).resolve().parents[3]
spec=importlib.util.spec_from_file_location('format_vt',Path(__file__).with_name('N1-format.py'))
f=importlib.util.module_from_spec(spec);spec.loader.exec_module(f)
archive=Path(__file__).with_name('N1-symbolic-snapshot.tar.gz')
with tarfile.open(archive) as tar:
 sources={m.name:tar.extractfile(m).read().decode() for m in tar.getmembers() if m.isfile()}
model=sources['core-s/bootstrap/model.vt']
base_model=subprocess.check_output(['git','show','1c0f143:core-s/bootstrap/model.vt'],cwd=root,text=True)
pattern=r'pub const ([A-Z_0-9]+):u32 = (\d+);'
old_names={m[0] for m in re.findall(pattern,base_model)}
constants={k:v for k,v in re.findall(pattern,model) if k not in old_names}
def normalize(text, expand):
 if expand:
  text=re.sub(pattern,lambda m:'' if m[1] in constants else m[0],text)
  text=re.sub(r'\b(?:model::)?([A-Z][A-Z_0-9]+)\b',lambda m:constants.get(m[1],m[0]),text)
 ts=[t for t in f.tokens(text) if not t.startswith('//') and not t.startswith('/*')]
 return [re.sub(r'_(?:u8|u32|usize)$','',t) if t[0].isdigit() else t for t in ts]
report={}
for path,source in sorted(sources.items()):
 baseline=subprocess.check_output(['git','show','1c0f143:'+path],cwd=root,text=True)
 a=normalize(baseline,False);b=normalize(source,True)
 if a!=b:
  for i,(x,y) in enumerate(zip(a,b)):
   if x!=y: print(path,i,a[max(0,i-8):i+8],b[max(0,i-8):i+8],file=sys.stderr);break
  raise SystemExit('Symbol normalization differs')
 report[path]={'expanded_tokens_equal':True,'tokens':len(a),'symbolic_sha256':f.digest(source)}
print(json.dumps({'constants':constants,'files':report},indent=2))
