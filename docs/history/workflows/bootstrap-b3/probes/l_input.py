#!/usr/bin/env python3
"""Independent target input/lexer probes, including deterministic syscall faults."""
import hashlib, json, pathlib, re, subprocess, tempfile, sys
ROOT = pathlib.Path(__file__).resolve().parents[4]
CORE = ROOT / "core-s"
REVISION = sys.argv[1] if len(sys.argv) > 1 else "1"
for name, digest in json.loads((ROOT / f"docs/workflows/bootstrap-b3/L-r{REVISION}-candidate.json").read_text())["files"].items():
    assert hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == digest, name
PROBE = re.search(r'const PROBE: &str = r#"(.*?)"#;', (CORE / "tests/b3_input.rs").read_text(), re.S)[1]
PROBE += '''
pub fn assembly() -> u32 {
 model::reset_all(); input::load("project");
 unsafe { if model::assembly_count != 1_u32 { return 8; } input::save(model::assemblies[1].bytes); }
 return 0;
}
pub fn decode_small() -> u32 {
 model::reset_all(); input::load("project");
 let t:model::Token=lex::next(1,0); var b:[1]u8=[0;1];
 let n:usize=lex::decode(t,core::slice_of_mut(b)); return n as u32;
}
static mut injected:usize=0;
'''
BASE = "format 1\nprofile linux-user\nentry probe::run\nsource input.vt\nassembly adapter.s0\noutput copied.vt\n"
FILES = ["bootstrap/model.vt", "bootstrap/lex.vt", "bootstrap/input.vt", "runtime/linux.vt", "runtime/bytes.vt"]
RESULTS = []
with tempfile.TemporaryDirectory(prefix="b3-l-review-") as temporary:
    root = pathlib.Path(temporary)
    for relative in FILES:
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes((CORE / relative).read_bytes())
    (root / "probe.vt").write_text(PROBE)
    global_index = sum(len(re.findall(r'^(?:pub )?static(?: mut)? ', (CORE / p).read_text(), re.M)) for p in FILES)
    original = (CORE / "runtime/linux.s0").read_text()
    def build(mode="normal", entry="probe::run"):
        prefix = ""
        if mode in ("fchmod_error", "read_error", "write_zero", "write_error"):
            nr = {"fchmod_error":52, "read_error":63, "write_zero":64, "write_error":64}[mode]
            prefix = f"__b2_syscall6:\naddi x5,x0,{nr}\nbne x10,x5,.Lreal\n"
            if nr == 64:
                prefix += "addi x5,x0,2\nbeq x11,x5,.Lreal\n"
            if mode == "write_error":
                prefix += f"address x5,__core_s_g{global_index}\nld x6,0(x5)\nbne x6,x0,.Lfail\naddi x6,x0,1\nsd x6,0(x5)\naddi x13,x0,1\njump .Lreal\n.Lfail:\n"
            prefix += f"addi x10,x0,{0 if mode=='write_zero' else -5}\njalr x0,0(x1)\n.Lreal:\njump __real\n"
        if mode == "short_eintr":
            prefix = f"""__b2_syscall6:
addi x5,x0,63
beq x10,x5,.Lio
addi x5,x0,64
bne x10,x5,.Lreal
.Lio:
address x5,__core_s_g{global_index}
ld x6,0(x5)
addi x6,x6,1
sd x6,0(x5)
addi x7,x0,1
and x6,x6,x7
beq x6,x0,.Lshort
addi x10,x0,-4
jalr x0,0(x1)
.Lshort:
sltiu x5,x13,4
bne x5,x0,.Lreal
addi x13,x0,3
.Lreal:
jump __real
"""
        asm = prefix + original.replace("__b2_syscall6:", "__real:") if prefix else original
        (root / "shim.s0").write_text(asm)
        (root / "build").write_text("format 1\nprofile linux-user\nentry " + entry + "\n" + "".join("source " + p + "\n" for p in FILES) + "source probe.vt\nassembly shim.s0\noutput image.elf\n")
        subprocess.run([str(CORE / "target/debug/core-s"), "project", "build"], cwd=root, check=True, capture_output=True)
        return (root / "image.elf").read_bytes()
    normal = build()
    def run(name, source=b"fn f()->u32{return 17;}", manifest=BASE, image=normal, expected=0, code=None, extra=None, output=None):
        case = root / ("case-" + str(len(RESULTS)))
        case.mkdir()
        (case / "input.vt").write_bytes(source)
        (case / "adapter.s0").write_bytes(b"# assembly snapshot\n")
        (case / "project").write_text(manifest)
        (case / "probe.elf").write_bytes(image)
        (case / "probe.elf").chmod(0o755)
        if extra: extra(case)
        result = subprocess.run(["qemu-riscv64", str(case / "probe.elf"), "project"], cwd=case, capture_output=True, timeout=30)
        assert result.returncode == expected, (name,result.returncode,result.stderr)
        if code: assert code in result.stderr.decode(), (name,result.stderr)
        if expected == 0: assert (case / "copied.vt").read_bytes() == (source if output is None else output), name
        elif not extra: assert not (case / "copied.vt").exists(), name
        RESULTS.append({"name":name,"exit":result.returncode,"stderr":result.stderr.decode()})
    run("source_exact_4MiB", b" "*(4*1024*1024))
    run("source_over_4MiB", b" "*(4*1024*1024+1), expected=1, code="I_LIMIT")
    run("source_33", manifest=BASE+"".join(f"source x{i}.vt\n" for i in range(32)), expected=1, code="I_LIMIT")
    run("assembly_9", manifest=BASE+"".join(f"assembly x{i}.s0\n" for i in range(8)), expected=1, code="I_LIMIT")
    run("manifest_over_16KiB", manifest="#"+" "*16384, expected=1, code="I_LIMIT")
    run("assembly_over_1MiB", expected=1, code="I_LIMIT", extra=lambda p:(p/"adapter.s0").write_bytes(b" "*(1048576+1)))
    run("aggregate_source_overflow", source=b" "*(4194304-3), manifest=BASE+"source second.vt\n", expected=1, code="I_LIMIT", extra=lambda p:(p/"second.vt").write_bytes(b"abcd"))
    for name,data in [("surrogate",b'"\xed\xa0\x80"'),("overlong",b'"\xc0\xaf"'),("out_of_range",b'"\xf4\x90\x80\x80"'),("truncated",b'"\xe2\x82'),("bom",b'\xef\xbb\xbf')]:
        run(name,data,expected=1,code="E_PARSE")
    run("utf8_string_comment", '/* 中文 */ "é日本"'.encode())
    run("assembly_snapshot",image=build(entry="probe::assembly"),output=b"# assembly snapshot\n")
    run("decode_capacity",b'"AB"',image=build(entry="probe::decode_small"),expected=1,code="I_LIMIT")
    run("repeated_eintr_short_io",b"/*abc*/\n"*1300,image=build("short_eintr"))
    for mode in ("fchmod_error","read_error","write_zero","write_error"):
        run(mode,image=build(mode),expected=1,code="E_IO")
    def dangling(case): (case/"copied.vt").symlink_to("missing-output")
    run("dangling_output_symlink",expected=1,code="E_IO",extra=dangling)
    def hardlink(case): (case/"copied.vt").hardlink_to(case/"input.vt")
    run("hardlink_input_output",expected=1,code="E_IO",extra=hardlink)
    if int(REVISION) >= 2:
        expected_limits = {
            "source_over_4MiB":"actual=4194305 limit=4194304 I_LIMIT file=1 byte=4194304\n",
            "source_33":"actual=33 limit=32 I_LIMIT file=0 byte=",
            "assembly_9":"actual=9 limit=8 I_LIMIT file=0 byte=",
            "manifest_over_16KiB":"actual=16385 limit=16384 I_LIMIT file=0 byte=16384\n",
            "assembly_over_1MiB":"actual=1048577 limit=1048576 I_LIMIT file=2 byte=1048576\n",
            "aggregate_source_overflow":"actual=4194305 limit=4194304 I_LIMIT file=2 byte=3\n",
            "decode_capacity":"actual=2 limit=1 I_LIMIT file=1 byte=0\n",
        }
        for result in RESULTS:
            if result["name"] in expected_limits:
                assert expected_limits[result["name"]] in result["stderr"], result
print(json.dumps({"revision":REVISION,"probes":len(RESULTS),"results":RESULTS},indent=2))
