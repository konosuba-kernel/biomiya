#!/usr/bin/env python3
"""Check the maintained layout without rewriting historical records or artifacts."""
import hashlib
import json
from pathlib import Path
import re
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]


def require(condition, message):
    if not condition:
        raise ValueError(message)


def main():
    seed = json.loads((ROOT / 'bootstrap/transition/seed.json').read_text())
    frozen = ROOT / 'bootstrap/frozen'
    for name, digest in seed['source_sha256'].items():
        path = frozen / name
        require(path.resolve().is_relative_to(frozen), f'frozen input escapes tree: {name}')
        require(hashlib.sha256(path.read_bytes()).hexdigest() == digest,
                f'frozen input changed: {name}')
    identity = hashlib.sha256(json.dumps(seed['source_sha256'], sort_keys=True,
                                        separators=(',', ':')).encode()).hexdigest()
    require(identity == seed['source_identity'], 'frozen identity changed')
    for old in ('core-s', 'src', 'Cargo.toml', 'runtime', 'platforms', 'vendor'):
        require(not (ROOT / old).exists(), f'obsolete top-level entry remains: {old}')

    manifests = []
    for directory in ('compiler', 'bootstrap/transition', 'tests/compiler',
                      'tests/trials', 'experiments/system', 'examples'):
        for manifest in sorted((ROOT / directory).rglob('*.build')):
            sources = []
            for raw in manifest.read_text().splitlines():
                parts = raw.split()
                if not parts or parts[0].startswith('#'):
                    continue
                require(len(parts) == 2, f'malformed manifest line: {manifest}: {raw}')
                key, name = parts
                if key not in ('source', 'assembly', 'output'):
                    continue
                path = ROOT / name
                require(not Path(name).is_absolute() and
                        all(p not in ('', '.', '..') for p in name.split('/')) and
                        path.resolve().is_relative_to(ROOT), f'unsafe manifest path: {name}')
                require(len(name.encode()) <= 96, f'path exceeds compiler limit: {name}')
                if key != 'output':
                    require(path.is_file(), f'missing manifest input: {manifest}: {name}')
                    require(name not in sources, f'duplicate input: {manifest}: {name}')
                    sources.append(name)
            manifests.append(str(manifest.relative_to(ROOT)))

    # Deliberately exclude frozen/legacy/design/history documents: those describe
    # old versions and preserve their original relative links and bytes.
    documents = [ROOT / name for name in ('README.md', 'bootstrap/README.md',
                 'legacy/README.md', 'docs/building.md', 'docs/layout-migration.md',
                 'docs/history/README.md', 'editors/vscode/README.md')]
    for directory in ('compiler', 'tests', 'tools', 'docs/reference', 'experiments/system'):
        documents.extend((ROOT / directory).rglob('*.md'))
    links = 0
    for document in sorted(set(documents)):
        for target in re.findall(r'\[[^\]]+\]\(([^\s)]+)\)', document.read_text()):
            parsed = urlsplit(target)
            if parsed.scheme or parsed.netloc or not parsed.path:
                continue
            require((document.parent / unquote(parsed.path)).exists(),
                    f'broken link: {document.relative_to(ROOT)} -> {target}')
            links += 1
    print(json.dumps({'passed': True, 'frozen_inputs': len(seed['source_sha256']),
                      'manifests': len(manifests), 'local_links': links}, indent=2))


if __name__ == '__main__':
    main()
