#!/usr/bin/env python3
"""Regression tests for acceptance evidence; these do not execute language tests."""

import copy
import json
import pathlib
import tempfile
import unittest
from unittest import mock
import subprocess
import test_runtime

import accept
import conformance
import reproduce


class GateTests(unittest.TestCase):
    def test_compiler_protocol(self):
        self.assertEqual(
            conformance.compiler_report("{", 0)["status"], "infrastructure-failure"
        )
        for report, code in [
            ({"status": "ok", "diagnostics": []}, 1),
            ({"status": "error", "diagnostics": [{"code": "E_TYPE"}]}, 0),
            ({"status": "ok", "diagnostics": [{"code": "E_TYPE"}]}, 1),
        ]:
            self.assertEqual(
                conformance.compiler_report(json.dumps(report), code)["status"],
                "infrastructure-failure",
            )
        self.assertEqual(
            conformance.compiler_report(
                json.dumps(
                    {"status": "error", "diagnostics": [{"code": "I_UNSUPPORTED"}]}
                ),
                2,
            )["status"],
            "unsupported",
        )

    def test_qemu_arbitrary_panic_bytes_and_timeout(self):
        original_run = subprocess.run

        def fake_qemu(command, **kwargs):
            return original_run(
                [
                    __import__("sys").executable,
                    "-c",
                    "import sys;sys.stdout.buffer.write(b'BIOMIYA:PANIC\\n\\xff')",
                ],
                **kwargs,
            )

        with mock.patch.object(test_runtime.subprocess, "run", side_effect=fake_qemu):
            result = test_runtime.run(pathlib.Path("unused.elf"))
        self.assertEqual(result["status"], "panic")
        self.assertIn("\ufffd", result["output"])
        with mock.patch.object(
            test_runtime.subprocess,
            "run",
            side_effect=subprocess.TimeoutExpired(
                "qemu", 1, output=b"\xff", stderr=b"\xfe"
            ),
        ):
            result = test_runtime.run(pathlib.Path("unused.elf"))
        self.assertEqual(result["status"], "timeout")
        self.assertEqual(result["output"], "\ufffd\ufffd")

    def test_missing_compiler_is_infrastructure_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            result = conformance.invoke(
                [str(pathlib.Path(directory) / "missing-compiler")]
            )
        self.assertEqual(result["status"], "infrastructure-failure")
        self.assertIn("error", result)

    def test_exact_case_counts_and_skips(self):
        cases = [{"id": "one", "kind": "accept", "file": "one.vt"}]
        valid = {
            "schema": 1,
            "selected": 1,
            "passed": 1,
            "results": [
                {
                    "id": "one",
                    "kind": "accept",
                    "passed": True,
                    "outcome": "Core-accepted",
                    "compilation": {
                        "stdout": json.dumps({"status": "ok", "diagnostics": []}),
                        "exit": 0,
                    },
                }
            ],
        }
        self.assertFalse(conformance.validate_result_report(valid, cases))
        for mutate in [
            lambda r: r.update(selected=0),
            lambda r: r["results"].append(r["results"][0]),
            lambda r: r["results"][0].update(passed="yes"),
            lambda r: r["results"][0].update(outcome="unsupported"),
        ]:
            invalid = copy.deepcopy(valid)
            mutate(invalid)
            self.assertTrue(conformance.validate_result_report(invalid, cases))

    def test_empty_or_malformed_suites(self):
        for data in [None, [], {}, {"schema": 1, "groups": []}]:
            self.assertTrue(accept.validate_integration_report(data))
            self.assertTrue(accept.validate_clean_report(data))
        self.assertTrue(accept.validate_platform_report([]))

    def test_complete_tool_inventory(self):
        names = ["host", "python", "rustc", "clang", "ld.lld", "qemu-system-riscv64"]
        data = {
            "schema": 1,
            "reference_environment": True,
            "checks": [{"tool": n, "passed": True} for n in names],
        }
        self.assertFalse(accept.validate_toolchain_report(data))
        data["checks"].pop()
        self.assertTrue(accept.validate_toolchain_report(data))

    def test_source_inventory_add_change_delete(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "src").mkdir()
            path = root / "src/a.rs"
            path.write_text("one")
            before = reproduce.source_inventory(root)
            path.write_text("two")
            self.assertNotEqual(before, reproduce.source_inventory(root))
            path.unlink()
            self.assertNotEqual(before, reproduce.source_inventory(root))
            (root / "src/b.rs").write_text("three")
            self.assertIn("src/b.rs", reproduce.source_inventory(root))

    def test_report_cannot_overwrite_sources(self):
        for path in [accept.ROOT / "Cargo.toml", accept.ROOT / "docs/new-report.json"]:
            with self.assertRaises(ValueError):
                accept.protect_report_path(path)
        accept.protect_report_path(accept.ROOT / "build/report.json")

    def test_elf_header_required(self):
        self.assertFalse(conformance.validate_elf_image(b"not an ELF"))
        image = bytearray(64)
        image[:7] = b"\x7fELF\x02\x01\x01"
        image[16:18] = (2).to_bytes(2, "little")
        image[18:20] = (243).to_bytes(2, "little")
        image[52:54] = (64).to_bytes(2, "little")
        self.assertTrue(conformance.validate_elf_image(image))
        image[48] = 1
        self.assertFalse(conformance.validate_elf_image(image))


if __name__ == "__main__":
    unittest.main()
