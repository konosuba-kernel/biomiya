#!/usr/bin/env python3
"""Account maintained Q, frozen bootstraps, support, tests and tools separately."""
import hashlib
import json
from pathlib import Path
import re

CORE = Path(__file__).resolve().parents[1]
REPO = CORE
FROZEN = CORE / "bootstrap/frozen"
SEED_CORE = FROZEN / "core-s"


def code_lines(source):
    # Preserve line boundaries while excluding strings and nested comments.
    out = []
    i = 0
    depth = 0
    quoted = False
    while i < len(source):
        if depth:
            if source.startswith('/*', i):
                depth += 1; i += 2
            elif source.startswith('*/', i):
                depth -= 1; i += 2
            else:
                out.append('\n' if source[i] == '\n' else ' '); i += 1
        elif quoted:
            if source[i] == '\\':
                out.extend('  '); i += 2
            elif source[i] == '"':
                out.append('"'); quoted = False; i += 1
            else:
                out.append('\n' if source[i] == '\n' else ' '); i += 1
        elif source.startswith('//', i):
            end = source.find('\n', i)
            i = len(source) if end == -1 else end
        elif source.startswith('/*', i):
            depth = 1; i += 2
        elif source[i] == '"':
            quoted = True; out.append('"'); i += 1
        else:
            out.append(source[i]); i += 1
    return sum(bool(line.strip()) for line in ''.join(out).splitlines())


def measure(paths):
    files = []
    for path in sorted(set(paths)):
        data = path.read_bytes()
        source = data.decode()
        files.append({'path': str(path.relative_to(REPO)), 'physical_lines': len(source.splitlines()),
                      'bytes': len(data), 'nonempty_noncomment_lines': code_lines(source),
                      'sha256': hashlib.sha256(data).hexdigest()})
    return {'files': files, **{key: sum(item[key] for item in files) for key in
                               ['physical_lines', 'bytes', 'nonempty_noncomment_lines']}}


def main():
    q = CORE / 'compiler'
    groups = {
        'maintained_compiler_q': list(q.glob('*.vt')),
        'runtime_vt': list((q / 'runtime').glob('*.vt')),
        'runtime_s0': list((q / 'runtime').glob('*.s0')),
        'system_applications': list((CORE / 'experiments/system').glob('*.vt')),
        'system_platform': [CORE / ('experiments/system/' + name) for name in ['monitor.S', 'machine.s0', 'linux.s0']],
        'frozen_transition_p': list((CORE / 'bootstrap/transition').glob('*.vt')),
        'frozen_bootstrap_b': list((SEED_CORE / 'bootstrap').glob('*.vt')),
        'frozen_rust_seed': list((SEED_CORE / 'src').rglob('*.rs')) + [FROZEN / ('src/' + name) for name in
                                                              ['source.rs','diagnostic.rs','syntax/lexer.rs']],
        'frozen_generated_unicode': [FROZEN / 'src/syntax/unicode.rs'],
        'language_tests': list((CORE / 'tests/language').rglob('*.vt')) + list((CORE / 'tests/language').rglob('*.s0')),
        'q_internal_tests': [p for p in (CORE / 'tests/compiler').rglob('*') if p.suffix in ('.vt', '.s0', '.s')],
        'representation_trials': list((CORE / 'tests/trials').glob('*.vt')),
        'new_host_tools': list((CORE / 'tools').glob('*.py'))
                          + list((CORE / 'experiments/system').glob('*.py'))
                          + [CORE / 'tools/q_machine_oracle.rs'],
    }
    frozen = [FROZEN / name for name in json.loads((CORE / 'bootstrap/transition/seed.json').read_text())['source_sha256']]
    groups['frozen_runtime'] = [p for p in frozen if p.parent == SEED_CORE / 'runtime' and p.suffix in ('.vt', '.s0')]
    groups['frozen_tests'] = [p for p in frozen if p.suffix in ('.rs', '.vt') and
                              any(p.is_relative_to(SEED_CORE / folder) for folder in ('tests', 'fixtures', 'bootstrap/tests'))]
    groups['frozen_host_tools'] = [p for p in frozen if p.parent == SEED_CORE / 'tools']
    counted = {p for paths in groups.values() for p in paths}
    groups['frozen_bootstrap_metadata'] = [p for p in frozen if p not in counted]
    report = {'categories': {name: measure(paths) for name, paths in groups.items()},
              'compiler_budget_lines': 8000, 'budget_is_met': False,
              'notes': ['Nonempty/noncomment lines use VT/C-style lexical comments; Python and assembly # comments remain counted.',
                        'Frozen categories cover all 71 seed inputs; metadata includes manifests, reference documents, lockfiles and the historical test log. Other repository legacy trees are outside this S-next delivery.',
                        'ELF writable reservation is not measured peak RSS; compiler max_frame is not total call-chain stack.']}
    report['budget_is_met'] = report['categories']['maintained_compiler_q']['physical_lines'] <= 8000
    owners = []
    for path in sorted(q.glob('*.vt')):
        source = path.read_text()
        variables = re.findall(r'(?m)^(pub )?static mut (\w+):([^=\n]+)', source)
        owners.append({'module': path.stem, 'mutable_statics': len(variables),
                       'public_mutable_statics': [name for public, name, ty in variables if public],
                       'private_mutable_statics': [name for public, name, ty in variables if not public]})
    report['state_inventory'] = owners
    out = CORE / 'build/cost.json'
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2) + '\n')
    for name, value in report['categories'].items():
        print(name, value['physical_lines'], value['bytes'])


if __name__ == '__main__':
    main()
