#!/usr/bin/env python3
"""Check the complete migrated language corpus against a named Q executable."""

# Assertions are verification checks, including in imported helpers.
if not __debug__:
    raise SystemExit("verification requires assertions; unset PYTHONOPTIMIZE and omit -O/-OO")

import argparse
import hashlib
import json
import re
from pathlib import Path
import shutil
from check_next import invoke
from verify import memory

CORE = Path(__file__).resolve().parents[1]

def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', type=Path, required=True)
    parser.add_argument('--record-dir', type=Path, required=True)
    args = parser.parse_args()
    record = args.record_dir.resolve()
    record.mkdir(parents=True, exist_ok=False)
    work = record / 'cases'
    work.mkdir()
    compiler = record / 'compiler.elf'
    compiler.write_bytes(args.compiler.read_bytes())
    compiler.chmod(0o755)
    qemu = shutil.which('qemu-riscv64')
    assert qemu
    inputs = {}
    def read(name):
        data = (CORE / name).read_bytes()
        inputs[name] = sha(data)
        path = record / 'inputs' / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        return data.decode()
    read('tools/check_q.py')
    read('tools/check_next.py')
    read('tools/verify.py')
    manifest = read('compiler/compiler.build')
    for line in manifest.splitlines():
        if line.startswith(('source ', 'assembly ')):
            read(line.split()[1])
    specifications = json.loads(read('tests/language/p3/cases.json'))
    assert len({item['name'] for item in specifications}) == len(specifications)
    inherited = {item['name'] for item in json.loads(read('tests/language/p2/cases.json'))}
    assert inherited <= {item['name'] for item in specifications}
    evidence = {'compiler_sha256': sha(compiler.read_bytes()), 'memory': memory(compiler.read_bytes()),
                'input_sha256': inputs, 'guest_stack_bytes': 1048576, 'cases': [], 'failed': []}
    def save():
        (record / 'verification.json').write_text(json.dumps(evidence, indent=2) + '\n')

    def case(name, files, settings):
        folder = work / name
        folder.mkdir()
        for filename, body in files.items():
            (folder / filename).write_text(body)
        build = files.get("program.build")
        if build is None:
            build = f"format 1\nprofile linux-user\nentry {settings.get('entry', 'run')}\n"
            build += "".join(f"source {n}\n" for n in files if n.endswith(".vt"))
            build += "".join(f"assembly {n}\n" for n in files if n.endswith(".s0"))
            build += "output program.elf\n"
            (folder / "program.build").write_text(build)
        result = {"name": name, "expected": settings, "source_sha256":
                  {n: sha(s.encode()) for n, s in files.items()}, "commands": []}
        evidence["cases"].append(result)

        def run(arguments):
            output = invoke([qemu, "-s", "1048576", *arguments], folder)
            result["commands"].append(output)
            save()
            return output

        for mode in ["check", "build"]:
            output = run([str(compiler), mode, "program.build"])
            if "reject" in settings:
                assert output["exit"] != 0 and re.search(r"\b" + settings["reject"] + r"\b", output["stderr"]), result
                assert not (folder / "program.elf").exists(), result
                if "diagnostic" in settings:
                    where = settings["diagnostic"]
                    assert f"file={where['file']} byte={where['byte']}" in output["stderr"], result
            else:
                assert output["exit"] == 0, result
                if mode == "check":
                    assert not (folder / "program.elf").exists(), result
        if "exit" in settings:
            output = run(["program.elf"])
            assert output["exit"] == settings["exit"] and output["stdout"] == settings.get("stdout", ""), result
            result["elf_sha256"] = sha((folder / "program.elf").read_bytes())
        if "blocking" in settings:
            contract = settings["blocking"]
            for mode, expected in [("nonblocking", "Verified"), ("unknown", "Unknown"), ("may_block", "Violation")]:
                policy = f"format 1\nrequire {contract['root']}\n"
                if mode != "unknown":
                    policy += f"{mode} {contract['external']}\n"
                (folder / "policy.blocking").write_text(policy)
                (folder / "analysis.build").write_text(build.replace("output program.elf", f"output {mode}.elf"))
                output = run([str(compiler), "build", "analysis.build", "--blocking", "policy.blocking"])
                assert f": {expected}\n" in output["stderr"], result
                if mode == "nonblocking":
                    assert output["exit"] == 0 and (folder / f"{mode}.elf").read_bytes() == (folder / "program.elf").read_bytes(), result
                else:
                    assert output["exit"] != 0 and not (folder / f"{mode}.elf").exists(), result
        result["passed"] = True
        save()
        print(f"PASS {name}", flush=True)

    for settings in specifications:
        if 'files' in settings:
            files = {name: read('tests/language/' + path) for name, path in settings['files'].items()}
        else:
            files = {'main.vt': read('tests/language/' + settings['source'])}
            if 'assembly' in settings:
                files['trace.s0'] = read('tests/language/' + settings['assembly'])
        try:
            case(settings['name'], files, settings)
        except AssertionError as error:
            evidence['failed'].append(settings['name'])
            evidence['cases'][-1]['failure'] = str(error)
            save()
            print('FAIL', settings['name'], flush=True)
    for name, digest in inputs.items():
        assert sha((CORE / name).read_bytes()) == digest, 'changed verification input: ' + name
    evidence['passed'] = sum(item.get('passed', False) for item in evidence['cases'])
    save()
    assert not evidence['failed'], evidence['failed']
    print('All', evidence['passed'], 'Q language cases passed.')


if __name__ == '__main__':
    main()
