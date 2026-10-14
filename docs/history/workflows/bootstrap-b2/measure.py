#!/usr/bin/env python3
"""Reproducible B2 source ledger; generated tables and inline tests stay separate."""
import hashlib, json, subprocess, tempfile
from pathlib import Path
root = Path(__file__).resolve().parents[3]
# When run from workflow, parents[3] is biomiya.

def piece(path, start=0, end=None):
    text=path.read_text();lines=text.splitlines(keepends=True);end=len(lines) if end is None else end
    return {'path':str(path.relative_to(root)), 'first_line':start+1, 'last_line':end,
            'source_sha256':hashlib.sha256(path.read_bytes()).hexdigest(), 'text':''.join(lines[start:end]), 'extension':path.suffix}

def measure(parts):
    with tempfile.TemporaryDirectory(prefix='core-s-cloc-') as directory:
        for i,p in enumerate(parts):Path(directory,f'{i}{p["extension"]}').write_text(p['text'])
        raw=subprocess.check_output(['cloc','--quiet','--json','--force-lang=C,vt','--force-lang=Assembly,s0',directory],text=True)
        data=json.loads(raw)['SUM']
    return {'physical_lines':sum(len(p['text'].splitlines()) for p in parts), 'cloc':data,
            'files':[{k:v for k,v in p.items() if k not in ('text','extension')} for p in parts]}

host=[];inline=[]
for p in sorted((root/'core-s/src').glob('*.rs')):
    text=p.read_text();lines=text.splitlines();test=next((i for i,l in enumerate(lines) if l=='#[cfg(test)]'),None)
    if test is None:host.append(piece(p))
    else:host.append(piece(p,0,test));inline.append(piece(p,test))
for name in ['src/source.rs','src/diagnostic.rs','src/syntax/lexer.rs']:host.append(piece(root/name))
groups={'host_compiler':host,'target_runtime':[piece(p) for p in sorted((root/'core-s/runtime').glob('*.vt'))],
 'target_compiler':[piece(p) for p in sorted((root/'core-s/compiler').glob('*.vt'))],
 'target_app':[piece(p) for p in sorted((root/'core-s/app').glob('*.vt'))],
 'target_assembly':[piece(p) for p in sorted((root/'core-s/runtime').glob('*.s0'))],
 'generated_unicode':[piece(root/'src/syntax/unicode.rs')],
 'tests':inline+[piece(p) for p in sorted((root/'core-s/tests').glob('*.rs'))],
 'behavior_inputs':[piece(p) for p in sorted((root/'core-s/fixtures').glob('*.vt'))]}
result={'date':'2026-10-10','cloc_version':subprocess.check_output(['cloc','--version'],text=True).strip(),
 'method':'Physical lines plus cloc code. VT counted as C only for its shared comment/string syntax. project.rs cfg(test) tail separated into tests. Entire source hashes and line intervals recorded. Third-party Cargo dependencies and build outputs excluded; generated Unicode separate; workflow validation scripts/independent probes are tooling, not shipped compiler/runtime.',
 'groups':{name:measure(parts) for name,parts in groups.items()}}
result['handwritten_total']={key:sum(result['groups'][name][key] for name in ('host_compiler','target_runtime','target_compiler','target_app','target_assembly')) for key in ('physical_lines',)}
result['handwritten_total']['code']=sum(result['groups'][name]['cloc']['code'] for name in ('host_compiler','target_runtime','target_compiler','target_app','target_assembly'))
output=root/'docs/workflows/bootstrap-b2/metrics-r1.json';output.write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:{'physical':v['physical_lines'],'code':v['cloc']['code']} for k,v in result['groups'].items()},indent=2));print(result['handwritten_total'])
