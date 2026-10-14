#!/usr/bin/env python3
"""Independent read-only evidence checks; writes only acceptance evidence and a fresh probe."""
import hashlib,json,pathlib,re,subprocess,sys,tarfile
ROOT=pathlib.Path(__file__).resolve().parents[3]
CORE=ROOT/'core-s'
WORK=pathlib.Path(__file__).parent
checks=[]
def require(value,label):
    if not value: raise ValueError(label)
    checks.append(label)
def sha(data): return hashlib.sha256(data).hexdigest()
def digest(path): return sha(path.read_bytes())
def read(path): return json.loads(path.read_text())
def hashes(base,entries,label):
    for name,want in entries.items(): require(digest(base/name)==want,f'{label}: {name}')
candidate=read(WORK/'I-candidate-r1.json')
hashes(ROOT,candidate['files'],'I frozen candidate')
require(len(candidate['files'])==58,'58 candidate files')
require(digest(ROOT.parent/'suba/CODE_STYLE.md')==candidate['standard_sha256'],'style identity')
results=read(WORK/'I-results.json')
require((WORK/'I-results.json').read_bytes()==(CORE/'next/q/maintenance-2026-10-14.json').read_bytes(),'maintenance identity')
seed=read(CORE/'next/seed.json')
hashes(ROOT,seed['source_sha256'],'frozen seed')
qhash=results['bootstrap']['q_source_sha256']
hashes(CORE,qhash,'current Q sources')
identity=sha(''.join(f'{p}\0{h}\n' for p,h in sorted(qhash.items())).encode())
require(identity==results['bootstrap']['q_source_identity'],'Q source aggregate identity')
for path in ('core-s/next/q/release.json','core-s/next/q/cost.json'):
    old=subprocess.check_output(['git','show',candidate['baseline']+':'+path],cwd=ROOT)
    require(old==(ROOT/path).read_bytes(),'historical unchanged: '+path)
for label in ('record','p3_record'):
    name=results['bootstrap'][label]
    require(digest(CORE/name)==results['bootstrap'][label+'_sha256'],'bootstrap record '+label)
p4dir=CORE/pathlib.Path(results['bootstrap']['record']).parent
p4=read(p4dir/'verification.json')
p3=read(CORE/results['bootstrap']['p3_record'])
require(p3['passed']==126 and len(p3['retained_target_tests'])==11 and not p3['failed'],'P3 126 language and 11 target cases')
require(p4['mechanical_checks_passed'],'P4 mechanical checks')
require(p4['q_source_sha256']==qhash,'P4 Q identity')
hashes(CORE,p4['tool_sha256'],'P4 tools')
hashes(p4dir,p4['capsule_sha256'],'P4 capsules')
require(digest(p4dir/'b.elf')==seed['compiler_sha256'] if 'compiler_sha256' in seed else digest(p4dir/'b.elf')==results['bootstrap']['seed_sha256'],'seed binary')
e3=(p4dir/'e3.elf').read_bytes()
require(sha(e3)==candidate['compiler_sha256'],'E3 candidate identity')
require(all((p4dir/(name+'.elf')).read_bytes()==e3 for name in ('e1','e2')),'E1 E2 E3 actual bytes equal')
with tarfile.open(p4dir/'q-source.tar') as archive:
    q={m.name:archive.extractfile(m).read() for m in archive.getmembers() if m.isfile()}
require({p:sha(b) for p,b in q.items()}==qhash,'Q capsule all bytes')
with tarfile.open(p4dir/'perturbed-source.tar') as archive:
    changed={m.name:archive.extractfile(m).read() for m in archive.getmembers() if m.isfile()}
require(set(q)==set(changed),'perturbation file set')
require([p for p in q if q[p]!=changed[p]]==['next/q/main.vt'],'only main changed')
mutation=p4['perturbation']
require(sha(changed[mutation['source']])==mutation['after_sha256'],'mutation digest')
require(changed[mutation['source']]==q[mutation['source']].replace(mutation['old_prefix'].encode(),mutation['new_prefix'].encode()),'actual prefix source mutation')
sys.path.insert(0,str(CORE/'tools'))
from selfhost_next import trace_contract
for stage in p4['stages']:
    name=stage['name']
    require(stage['exit']==0 and stage['environment']=={'PATH':''},'stage execution '+name)
    for stream in ('stdout','stderr'):
        require((p4dir/'logs'/f'{name}.{stream}').read_text()==stage[stream],'stage log '+name+' '+stream)
    access=trace_contract((p4dir/'logs'/f'{name}.strace').read_text(),set(stage['input_sha256']),'compiler.elf' if stage['mode']=='build' else None)
    require(access==stage['access'],'trace contract '+name)
    if stage['mode']=='build': require(digest(p4dir/(name+'.elf'))==stage['output']['sha256'],'stage actual ELF '+name)
require(p4['stages'][0]['output']['sha256']==p3['compiler_sha256'],'B P link')
for i in range(1,4): require(p4['stages'][i]['compiler_sha256']==p4['stages'][i-1]['output']['sha256'],'stage compiler chain '+str(i))
counts={}
for name,suite in results['suites'].items():
    path=CORE/suite['record']; data=read(path)
    require(digest(path)==suite['record_sha256'],'suite record '+name)
    identity=data.get('compiler_sha256',data.get('input_sha256',{}).get('compiler.elf'))
    require(identity==candidate['compiler_sha256'],'suite E3 '+name)
    if name=='system':
        require(data['accepted'] and all(c['exit']==0 for c in data['commands']),'P5 commands')
        hashes(CORE,suite['nested_records'],'P5 nested record')
        neg=read(path.parent/'negative/verification.json')
        require(neg['accepted'] and len(neg['cases'])==10,'P5 negative count')
        for case in neg['cases']:
            run=read(path.parent/'negative'/case['name']/'machine/verification.json')
            require(run['accepted'] and run['commands'][-1]['exit']==case['expected_exit'],'P5 negative '+case['name'])
        continue
    cases=data.get('cases',data.get('tests'))
    require(len(cases)==suite['case_count'] and bool(data['passed']),'suite count/pass '+name)
    require(all(c.get('passed',True) is True for c in cases),'case pass flags '+name)
    require(not data.get('failed',[]) and not data.get('changed_inputs',[]),'no failures/changed inputs '+name)
    if name=='diagnostics':
        inputs={str(path.relative_to(CORE).parent/k):h for k,h in data['input_sha256'].items()}
        hashes(CORE,inputs,'diagnostic snapshot')
        hashes(CORE,data['cases'][-1]['input_sha256'],'diagnostic IR sources')
    else:
        inputs=data.get('input_sha256',data.get('inputs',{}))
        if isinstance(inputs,dict): hashes(CORE,inputs,'suite inputs '+name)
    counts[name]=len(cases)
require(sum(counts.values())==435,'435 total regression scenarios')
guards=results['tool_guards']; path=CORE/guards['record']; g=read(path)
require(digest(path)==guards['record_sha256'],'guard evidence identity')
require(g['compiler_sha256']==candidate['compiler_sha256'] and g['passed'],'guard E3 passed')
require(len(g['entry_points'])==88 and len(g['corpus'])==5,'88 guard entry modes and 5 corpus invocations')
hashes(CORE/'tools',g['tool_sha256'],'guard tools')
for entry in g['entry_points']: require((entry['exit']==0)==(entry['mode']=='normal'),'guard exit '+entry['tool']+' '+entry['mode'])
missing=[]
for name in candidate['files']:
    if not name.endswith('.md'):continue
    path=ROOT/name
    for target in re.findall(r'\]\(([^)]+)\)',path.read_text()):
        if '://' in target or target.startswith('#'):continue
        target=target.split('#')[0]
        destination=(path.parent/target).resolve()
        if not destination.exists() and destination!=WORK/'acceptance-r1.md':missing.append((name,target))
require(not missing,'candidate local Markdown links (acceptance-r1 pending)')
probe=CORE/'build/behavior-fixes-acceptance-perturbation-r1';probe.mkdir(exist_ok=False)
(probe/'sample.vt').write_text('module proof::sample;\nfn run() -> u32 { return 42; }\n')
(probe/'sample.build').write_text(mutation['sample_manifest'])
runs=[]
for binary,prefix in (('e3',mutation['old_prefix']),('perturbed',mutation['new_prefix'])):
    cmd=['/usr/bin/qemu-riscv64','-s','1048576',str(p4dir/(binary+'.elf')),'check','sample.build']
    run=subprocess.run(cmd,cwd=probe,env={'PATH':''},capture_output=True,text=True,timeout=60)
    require(run.returncode==0 and run.stderr.startswith(prefix+' sources=') and not run.stdout,'independent perturbation behavior '+binary)
    runs.append({'command':cmd,'exit':run.returncode,'stdout':run.stdout,'stderr':run.stderr})
require(not (probe/'compiler.elf').exists(),'check probe no output')
report={'passed':True,'candidate_sha256':digest(WORK/'I-candidate-r1.json'),'compiler_sha256':sha(e3),'checks':checks,'suite_counts':counts,'total_regression_scenarios':sum(counts.values()),'independent_perturbation_runs':runs,'missing_links':missing}
(WORK/'acceptance-r1-evidence.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'passed':True,'checks':len(checks),'scenarios':sum(counts.values())}))
