#!/usr/bin/env python3
"""Compile the complete examples embedded in a versioned language reference."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


CORE = Path(__file__).resolve().parents[1]
REFERENCES = CORE / "docs/reference"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def extract(document):
    pattern = r"<!-- example: ([a-z0-9_]+/[^\s]+) -->\n```(?:vt|text)\n(.*?)\n```"
    examples = {}
    for name, body in re.findall(pattern, document, re.S):
        case, filename = name.split("/", 1)
        assert re.fullmatch(r"[a-zA-Z0-9_.-]+", filename) and filename not in [".", ".."]
        files = examples.setdefault(case, {})
        assert filename not in files, f"duplicate example: {name}"
        files[filename] = body + "\n"
    assert len(re.findall(r"<!-- example:", document)) == sum(map(len, examples.values()))
    assert document.count("```vt\n") == sum(name.endswith(".vt") for files in examples.values() for name in files)
    return examples


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", default="b3.1", choices=sorted(p.stem for p in REFERENCES.glob("*.json")))
    parser.add_argument("--compiler", type=Path, default=CORE / "build/b31-final.elf")
    parser.add_argument("--record-dir", type=Path, help="new directory for verification results")
    args = parser.parse_args()
    metadata_path = REFERENCES / f"{args.version}.json"
    metadata = json.loads(metadata_path.read_text())
    compiler = args.compiler.resolve()
    compiler_sha = sha(compiler.read_bytes())
    allowed = [metadata["compiler_sha256"]]
    allowed.extend(build["sha256"] for build in metadata.get("compatible_compilers", []))
    assert compiler_sha in allowed, "compiler does not match this reference version"
    qemu = shutil.which("qemu-riscv64")
    assert qemu, "qemu-riscv64 is required"
    document_path = REFERENCES / metadata["document"]
    document = document_path.read_text()
    examples = extract(document)
    names = [case["name"] for case in metadata["cases"]]
    assert len(names) == len(set(names)) and set(names) == set(examples)
    if args.record_dir:
        record = args.record_dir.resolve()
        record.mkdir(parents=True, exist_ok=False)
    else:
        record = Path(tempfile.mkdtemp(prefix="core-s-reference-"))
    evidence = {"version": args.version, "document_revision": metadata["document_revision"],
                "compiler_sha256": compiler_sha, "document_sha256": sha(document_path.read_bytes()),
                "metadata_sha256": sha(metadata_path.read_bytes()), "runner_sha256": sha(Path(__file__).read_bytes()),
                "qemu": qemu, "guest_stack_bytes": 1048576, "cases": []}
    print(f"Evidence: {record}", flush=True)

    with tempfile.TemporaryDirectory(prefix="core-s-reference-input-") as temporary:
        for case in metadata["cases"]:
            folder = Path(temporary) / case["name"]
            folder.mkdir()
            files = examples[case["name"]]
            for name, source in files.items():
                (folder / name).write_text(source)
            manifest = files.get("program.build")
            if manifest is None:
                manifest = f"format 1\nprofile linux-user\nentry {case['entry']}\n"
                manifest += "".join(f"source {name}\n" for name in files if name.endswith(".vt"))
                manifest += "".join(f"assembly {name}\n" for name in files if name.endswith(".s0"))
                manifest += "output program.elf\n"
                (folder / "program.build").write_text(manifest)
            assert "output program.elf\n" in manifest
            result = {"name": case["name"], "source_sha256": {n: sha(s.encode()) for n, s in files.items()}, "commands": []}

            def invoke(arguments):
                output = subprocess.run([qemu, "-s", "1048576", *arguments], cwd=folder,
                                        env={"PATH": ""}, capture_output=True, text=True, timeout=60)
                result["commands"].append({"arguments": arguments, "exit": output.returncode,
                                           "stdout": output.stdout, "stderr": output.stderr})
                return output

            for command in ["check", "build"]:
                output = invoke([str(compiler), command, "program.build"])
                if "reject" in case:
                    assert output.returncode != 0 and re.search(r"\b" + case["reject"] + r"\b", output.stderr), result
                    assert not (folder / "program.elf").exists()
                else:
                    assert output.returncode == 0, result
                    if command == "check":
                        assert not (folder / "program.elf").exists()
            if "exit" in case:
                output = invoke(["program.elf"])
                assert output.returncode == case["exit"], result
                result["elf_sha256"] = sha((folder / "program.elf").read_bytes())
            if "blocking" in case:
                settings = case["blocking"]
                for mode, expected in [("nonblocking", "Verified"), ("unknown", "Unknown"), ("may_block", "Violation")]:
                    output_name = f"{mode}.elf"
                    (folder / "analysis.build").write_text(manifest.replace("output program.elf", f"output {output_name}"))
                    policy = f"format 1\nrequire {settings['root']}\n"
                    if mode != "unknown":
                        policy += f"{mode} {settings['external']}\n"
                    if mode == "nonblocking":
                        assert policy == files["program.blocking"], "documented policy differs"
                    (folder / "program.blocking").write_text(policy)
                    output = invoke([str(compiler), "build", "analysis.build", "--blocking", "program.blocking"])
                    assert f": {expected}\n" in output.stderr, result
                    if mode == "nonblocking":
                        assert output.returncode == 0 and (folder / output_name).read_bytes() == (folder / "program.elf").read_bytes(), result
                    else:
                        assert output.returncode != 0 and not (folder / output_name).exists(), result
                result["analysis_on_off_equal"] = True
            evidence["cases"].append(result)
            (record / "verification.json").write_text(json.dumps(evidence, indent=2) + "\n")
            print(f"PASS {case['name']}", flush=True)
    evidence["passed"] = len(evidence["cases"])
    (record / "verification.json").write_text(json.dumps(evidence, indent=2) + "\n")
    print(f"All {evidence['passed']} reference examples passed.")


if __name__ == "__main__":
    main()
