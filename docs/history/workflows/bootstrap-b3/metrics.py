#!/usr/bin/env python3
"""Record format-sensitive line counts and raw source sizes, without hiding helpers."""
import hashlib,json,subprocess,tempfile
from pathlib import Path
root=Path(__file__).resolve().parents[3]
record=root/'docs/workflows/bootstrap-b3'
base=json.loads((root/'docs/workflows/bootstrap-b2/metrics-r1.json').read_text())
# Original Rust seed/test/generated table groups retain their documented line ranges.
# Added target implementation is counted separately from seed and B2 input prototype.
groups={k:v for k,v in base['groups'].items() if k in ['host_compiler','generated_unicode','tests','behavior_inputs']}
def group(paths):
 files=[];physical=0;byte_count=0
 for p in sorted(paths):
  data=p.read_bytes();lines=len(data.splitlines());physical+=lines;byte_count+=len(data)
  files.append({'path':str(p.relative_to(root)),'first_line':1,'last_line':lines,'source_bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
 with tempfile.NamedTemporaryFile(mode='w') as listing:
  listing.write('\n'.join(str(root/f['path']) for f in files));listing.flush()
  p=subprocess.run(['cloc','--json','--quiet','--force-lang=C,vt','--force-lang=Assembly,s0','--force-lang=Python,txt','--list-file='+listing.name],capture_output=True,text=True,check=True)
  summary=json.loads(p.stdout)['SUM']
 return {'physical_lines':physical,'source_bytes':byte_count,'cloc':summary,'files':files}
groups['target_compiler']=group((root/'core-s/bootstrap').glob('*.vt'))
groups['target_runtime']=group((root/'core-s/runtime').glob('*.vt'))
groups['platform_s0']=group((root/'core-s/runtime').glob('*.s0'))
groups['b2_input_application']=group(list((root/'core-s/compiler').glob('*.vt'))+list((root/'core-s/app').glob('*.vt')))
groups['new_rust_tests']=group((root/'core-s/tests').glob('b3*.rs'))
groups['new_target_fixtures']=group((root/'core-s/bootstrap/tests').glob('*.vt'))
groups['b3_validation_tools']=group((root/'docs/workflows/bootstrap-b3').rglob('*.py'))
groups['I1_independent_probe_snapshots']=group((root/'docs/workflows/bootstrap-b3').glob('I-r1-independent-*-script.txt'))
result={'cloc_version':subprocess.check_output(['cloc','--version'],text=True).strip(),'method':'VT uses C comment/string recognition. Raw bytes supplement format-sensitive physical/cloc counts; dense VT lines can contain several statements, so line counts do not prove a proportional complexity reduction. Target compiler includes parser, types, checker, IR, lexer, model/containers, IO/manifest, RV64, s0, ELF and CLI. Platform and runtime are separate. Seed and B2 retained prototype are not removed. Tests, generated Unicode and workflow Python tools are separately disclosed. I1 independent Python probe snapshots saved as .txt are counted separately, including copied driver content; they are evidence snapshots, not additional production compiler implementation. No generated product source in B3.','groups':groups}
(record/'metrics.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:{f:v[f] for f in ['physical_lines','source_bytes','cloc'] if f in v} for k,v in groups.items()},indent=2))
