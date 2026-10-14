#!/usr/bin/env python3
"""Guarded entry point for the byte-preserved B3.1 tool in bootstrap/frozen."""
if not __debug__:
    raise SystemExit("verification requires assertions; unset PYTHONOPTIMIZE and omit -O/-OO")

from pathlib import Path
import runpy

FROZEN_TOOL = Path(__file__).resolve().parents[1] / "bootstrap/frozen/core-s/tools/check_reference.py"
_frozen = runpy.run_path(str(FROZEN_TOOL))
extract = _frozen["extract"]

if __name__ == "__main__":
    _frozen["main"]()
