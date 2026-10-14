#!/usr/bin/env python3
"""Independent H contract checks; no product modifications."""
import hashlib, json, pathlib, struct, subprocess, tempfile, sys

ROOT = pathlib.Path(__file__).resolve().parents[4]
CANDIDATE = ROOT / ("docs/workflows/bootstrap-b3/H-r" + (sys.argv[1] if len(sys.argv) > 1 else "1") + "-candidate.json")
for relative, expected in json.loads(CANDIDATE.read_text())["files"].items():
    assert hashlib.sha256((ROOT / relative).read_bytes()).hexdigest() == expected, relative
CORE = ROOT / "core-s"
SEED = CORE / "target/debug/core-s"
subprocess.run([str(SEED), "project", "bootstrap/tests/model.build"], cwd=CORE, check=True)
subprocess.run(["qemu-riscv64", "target/b3-h-probe.elf"], cwd=CORE, check=True)
elf = (CORE / "target/b3-h-probe.elf").read_bytes()
phoff = struct.unpack_from("<Q", elf, 32)[0]
rw_mem = struct.unpack_from("<Q", elf, phoff + 2 * 56 + 40)[0]
assert rw_mem == 23162440, rw_mem

SOURCE = '''module probe;
import bootstrap::model;
import runtime::linux;
pub fn start() -> u32 {
    model::reset_all();
    var mode:usize = linux::argc();
    unsafe {
        if mode == 2_usize {
            model::source_count = 32;
            let id:u32 = model::add_source(model::sources[1]);
            return 91;
        }
        if mode == 3_usize {
            model::data_byte_count = 8388608;
            model::data_byte(0);
            return 92;
        }
        if mode == 4_usize {
            model::name_byte_count = 2097151;
            let id:u32 = model::name("zz");
            return 93;
        }
        if mode == 5_usize {
            let n:u32 = model::name("conflict");
            let s:u32 = model::symbol(n,1);
            let t:u32 = model::symbol(n,2);
            return 94;
        }
        let empty:u32 = model::name("");
        if empty == 0_u32 || model::name("") != empty { return 10; }
        var local:[3]u8 = [65,66,67];
        let saved:u32 = model::name(core::slice_of(local));
        local[0] = 90;
        if model::name("ABC") != saved { return 11; }
        if model::name(core::slice_of(local)) == saved { return 12; }
        let a:u32 = model::symbol(0,0);
        let b:u32 = model::symbol(0,0);
        if a == b { return 13; }
        let s:u32 = model::symbol(saved,0);
        if model::symbol(saved,2) != s || model::symbols[s as usize].kind != 2_u32 { return 14; }
        model::slot_count = 20; model::inst_count = 17;
        model::arg_count = 2; model::parameter_slot_count = 3;
        model::reset_function();
        if model::slot_count != 0_u32 || model::inst_count != 0_u32 || model::arg_count != 0_u32 || model::parameter_slot_count != 0_u32 { return 15; }
        if model::max_ir != 17_usize || model::name("ABC") != saved { return 16; }
        model::reset_all();
        if model::max_ir != 0_usize || model::symbol_count != 0_u32 || model::name_count != 0_u32 || model::type_count != 5_u32 { return 17; }
        if model::types[1].size != 0_usize || model::types[1].align != 1_usize || model::types[3].size != 1_usize || model::types[4].size != 4_usize || model::types[5].size != 8_usize { return 18; }
    }
    return 0;
}
'''
with tempfile.TemporaryDirectory(prefix="b3-h-review-") as temporary:
    root = pathlib.Path(temporary)
    (root / "model.vt").write_bytes((CORE / "bootstrap/model.vt").read_bytes())
    (root / "linux.vt").write_bytes((CORE / "runtime/linux.vt").read_bytes())
    (root / "linux.s0").write_bytes((CORE / "runtime/linux.s0").read_bytes())
    (root / "probe.vt").write_text(SOURCE)
    (root / "probe.build").write_text("format 1\nprofile linux-user\nentry probe::start\nsource model.vt\nsource linux.vt\nsource probe.vt\nassembly linux.s0\noutput result.elf\n")
    subprocess.run([str(SEED), "project", "probe.build"], cwd=root, check=True)
    for argc, diagnostic in [(1, None), (2, "actual=33 limit=32 I_LIMIT"), (3, "actual=8388609 limit=8388608 I_LIMIT"), (4, "actual=2097153 limit=2097152 I_LIMIT"), (5, "E_LINK")]:
        result = subprocess.run(["qemu-riscv64", str(root / "result.elf"), *(["x"] * (argc-1))], capture_output=True)
        assert result.returncode == (0 if diagnostic is None else 1), (argc, result.returncode, result.stderr)
        if diagnostic:
            assert diagnostic in result.stderr.decode(), result.stderr
print(json.dumps({"hashes": "match", "rw_mem_bytes": rw_mem, "independent_probes": 5, "result": "pass"}))
