#!/usr/bin/env python3
"""N3 machine-only byte equivalence, target sizeof and discriminating mutants.
All mutations and builds are confined to temporary copies. Not a self-host claim.
"""
from pathlib import Path
import hashlib, json, subprocess, tarfile, tempfile

root = Path(__file__).resolve().parents[3]
core = root / 'core-s'
files = ['bootstrap/'+n+'.vt' for n in ['model', 'rules', 'image', 'rv64', 's0']]
files += ['bootstrap/tests/machine.vt', 'runtime/linux.vt', 'runtime/linux.s0']
sha = lambda data: hashlib.sha256(data).hexdigest()
current = {name: (core/name).read_bytes() for name in files}
results = {}
outputs = {}
review = json.loads((root/'docs/workflows/bootstrap-b31/N2-r1-review.json').read_text())
n1 = json.loads((root/'docs/workflows/bootstrap-b31/N1-r1-candidate.json').read_text())
with tarfile.open(root/'docs/workflows/bootstrap-b31/N3-machine-baseline.tar.gz') as archive:
    old = {member.name: archive.extractfile(member).read() for member in archive.getmembers()}
assert sha(old['bootstrap/model.vt']) == review['file_hashes_verified']['core-s/bootstrap/model.vt']
for name in ['bootstrap/image.vt', 'bootstrap/rv64.vt', 'bootstrap/s0.vt']:
    assert sha(old[name]) == n1['files']['core-s/'+name]
assert old['bootstrap/tests/machine.vt'] == subprocess.check_output(
    ['git', 'show', '1c0f143:core-s/bootstrap/tests/machine.vt'], cwd=root)
for name in ['runtime/linux.vt', 'runtime/linux.s0']:
    assert old[name] == current[name]

variants = ['current', 'slot_mutation', 'inst_mutation', 'arg_mutation',
            'parameter_mutation', 'count_mutation', 'missing_high_reference',
            'stale_definition', 'N2_machine_baseline']
for variant in variants:
    sources = dict(old if variant == 'N2_machine_baseline' else current)
    if variant not in ['current', 'N2_machine_baseline']:
        text = sources['bootstrap/rv64.vt'].decode()
        mutations = {
            'slot_mutation': 'model::slots[1].flags = model::slots[1].flags ^ 1_u32;',
            'inst_mutation': 'model::insts[1].span.end = model::insts[1].span.end+1_u32;',
            'arg_mutation': 'if model::arg_count != 0_u32 { model::args[1] = 0; }',
            'parameter_mutation': 'if model::parameter_slot_count != 0_u32 { model::parameter_slots[1] = 0; }',
            'count_mutation': 'model::function_decl = 0;',
        }
        if variant in mutations:
            # Mutate after emission: output bytes cannot conceal changes to input IR.
            anchor = '            instruction(model::insts[at as usize], decl);\n            at = at+1_u32;\n        }'
            assert text.count(anchor) == 1
            text = text.replace(anchor, anchor+'\n        '+mutations[variant])
        elif variant == 'missing_high_reference':
            anchor = '        j = 1;\n        while j <= (label_high as usize) {'
            assert text.count(anchor) == 1
            text = text.replace(anchor, anchor.replace('<=', '<'))
        else:
            anchor = '            label_defs[j] = 0;'
            assert text.count(anchor) == 1
            text = text.replace(anchor, '')
        sources['bootstrap/rv64.vt'] = text.encode()
    # An executable target query measures actual model::Slot layout.
    name = 'bootstrap/tests/machine.vt'
    text = sources[name].decode()
    anchor = '    model::reset_all();'
    assert text.count(anchor) == 1
    text = text.replace(anchor, '    if model::equal(mode,"slot_size") { return sizeof(model::Slot) as u32; }\n'+anchor)
    sources[name] = text.encode()
    with tempfile.TemporaryDirectory(prefix='b31-n3-machine-') as tmp:
        path = Path(tmp)
        for name, data in sources.items():
            dest = path/name
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(data)
        (path/'driver.build').write_text('format 1\nprofile linux-user\nentry machine_test::run\n'+
            ''.join('source '+name+'\n' for name in sources if name.endswith('.vt'))+
            'assembly runtime/linux.s0\noutput driver.elf\n')
        built = subprocess.run([str(core/'target/debug/core-s'), 'project', 'driver.build'],
            cwd=path, capture_output=True, text=True, timeout=60)
        assert built.returncode == 0, (variant, built.stderr)
        if variant in ['current', 'N2_machine_baseline']:
            modes = ['ir', 'wrap', 'constants', 'slot_size']
            if variant == 'current':
                modes += ['labels_reuse', 'labels_yes_missing', 'labels_no_missing']
        elif variant == 'missing_high_reference':
            modes = ['labels_yes_missing', 'labels_no_missing']
        elif variant == 'stale_definition':
            modes = ['labels_reuse']
        else:
            modes = ['ir']
        records = {}
        for mode in modes:
            run_dir = path/mode
            run_dir.mkdir()
            run = subprocess.run(['/usr/bin/qemu-riscv64', '-s', '1048576',
                str(path/'driver.elf'), mode], cwd=run_dir, capture_output=True, text=True, timeout=60)
            record = {'exit': run.returncode, 'stderr': run.stderr,
                'new_output': (run_dir/'out.elf').exists()}
            if record['new_output']:
                data = (run_dir/'out.elf').read_bytes()
                record['elf_sha256'] = sha(data)
                outputs[variant, mode] = data
            records[mode] = record
        results[variant] = records
for mode in ['ir', 'wrap', 'constants']:
    assert outputs['current', mode] == outputs['N2_machine_baseline', mode], mode
assert results['current']['slot_size']['exit'] == 20
assert results['N2_machine_baseline']['slot_size']['exit'] == 32
assert results['current']['labels_reuse']['exit'] == 0
for variant in variants[1:6]:
    assert results[variant]['ir'] == {'exit': 1, 'stderr': 'I_INTERNAL file=2 byte=9001\n', 'new_output': False}
for mode in ['labels_yes_missing', 'labels_no_missing']:
    assert 'I_INTERNAL file=2 byte=17' in results['current'][mode]['stderr']
    assert 'E_LINK' in results['missing_high_reference'][mode]['stderr']
    assert not results['missing_high_reference'][mode]['new_output']
assert results['stale_definition']['labels_reuse']['exit'] == 1
print(json.dumps({
    'scope': 'author machine probes only; independent N3 acceptance remains pending',
    'input_hashes': {name: sha(data) for name, data in current.items()},
    'baseline_hashes': {name: sha(data) for name, data in old.items()},
    'stack_bytes': 1048576,
    'same_ELF_as_N2_machine': ['ir', 'wrap', 'constants'],
    'measured_slot_bytes': {'N2': 32, 'N3': 20},
    'production_table_payload_delta': (20-32)*32769+8*32769+4,
    'results': results,
    'mutants_discriminated': True,
}, indent=2))
