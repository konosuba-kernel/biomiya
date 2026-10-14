#!/usr/bin/env python3
"""Independent M malformed-block/type checks on real RV64 emitter execution."""
import hashlib, json, pathlib, subprocess, tempfile, sys
ROOT = pathlib.Path(__file__).resolve().parents[4]
CORE = ROOT / "core-s"
REVISION = sys.argv[1] if len(sys.argv) > 1 else "1"
for name,digest in json.loads((ROOT/f"docs/workflows/bootstrap-b3/M-r{REVISION}-candidate.json").read_text())["files"].items():
    assert hashlib.sha256((ROOT/name).read_bytes()).hexdigest()==digest,name
fixture=(CORE/"bootstrap/tests/machine.vt").read_text()
fixture=fixture.replace('emit(14,0,1,0,0,0,0);', '''if !model::equal(mode,"nolabel") {
    if model::equal(mode,"labeltwo") { emit(14,0,2,0,0,0,0); }
    else { emit(14,0,1,0,0,0,0); }
}
if model::equal(mode,"valuetypecycle") {
    unsafe {
        let ty:u32 = model::type_count+1_u32;
        let field:u32 = model::add_field(model::Field { name:0,ty:ty,flags:0,offset:0,span:model::Span {file:2,start:17,end:18} });
        let added:u32 = model::add_type(model::Type {kind:9,a:0,b:0,flags:0,size:0,align:1,first:field,count:1});
    }
}
if model::equal(mode,"mixedcycle") {
    unsafe {
        let ty:u32 = model::type_count+1_u32;
        let field:u32 = model::add_field(model::Field {name:0,ty:ty+1_u32,flags:0,offset:0,span:model::Span {file:2,start:17,end:18}});
        let a:u32 = model::add_type(model::Type {kind:9,a:0,b:0,flags:0,size:0,align:1,first:field,count:1});
        let b:u32 = model::add_type(model::Type {kind:7,a:ty,b:0,flags:0,size:0,align:1,first:0,count:0});
    }
}
if model::equal(mode,"deepchain") {
    var i:u32 = 0;
    while i < 8000_u32 {
        unsafe {
            var next:u32 = model::type_count+2_u32;
            if i == 7999_u32 { next = 1; }
            let added:u32 = model::add_type(model::Type {kind:7,a:next,b:1,flags:0,size:0,align:1,first:0,count:0});
        }
        i=i+1_u32;
    }
}
if model::equal(mode,"pointerprojection") {
    let array:u32=model::add_type(model::Type {kind:7,a:4,b:2,flags:0,size:8,align:4,first:0,count:0});
    let root_ptr:u32=pointer(array);
    let field_ptr:u32=pointer(4);
    let aggregate:u32=slot(array);
    let from:u32=slot(root_ptr);
    let dest:u32=slot(field_ptr);
    let offset:u32=slot(5);
    emit(3,from,aggregate,0,0,0,0);
    emit(1,offset,0,0,0,0,4);
    emit(7,dest,from,offset,0,0,0);
}
''',1)
RESULTS=[]
with tempfile.TemporaryDirectory(prefix="b3-m-review-") as temporary:
    root=pathlib.Path(temporary)
    paths=["bootstrap/model.vt","bootstrap/image.vt","bootstrap/rv64.vt","bootstrap/s0.vt","runtime/linux.vt"]
    for path in paths+["runtime/linux.s0"]:
        dest=root/path;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes((CORE/path).read_bytes())
    (root/"probe.vt").write_text(fixture)
    (root/"build").write_text("format 1\nprofile linux-user\nentry machine_test::run\n"+"".join("source "+p+"\n" for p in paths)+"source probe.vt\nassembly runtime/linux.s0\noutput driver.elf\n")
    subprocess.run([str(CORE/"target/debug/core-s"),"project","build"],cwd=root,check=True,capture_output=True)
    for mode in ["nolabel","labeltwo","valuetypecycle","wrap","ir","mixedcycle","deepchain","pointerprojection"]:
        case=root/mode;case.mkdir()
        result=subprocess.run(["/usr/bin/qemu-riscv64",str(root/"driver.elf"),mode],cwd=case,capture_output=True,timeout=30,env={"PATH":""})
        record={"mode":mode,"compiler_exit":result.returncode,"stderr":result.stderr.decode(),"elf_created":(case/"out.elf").exists()}
        if record["elf_created"]:
            (case/"out.elf").chmod(0o755)
            executed=subprocess.run(["/usr/bin/qemu-riscv64",str(case/"out.elf")],capture_output=True,timeout=30,env={"PATH":""})
            record["program_exit"]=executed.returncode
        RESULTS.append(record)
    if int(REVISION)>=2:
        for record in RESULTS[:3]:
            assert record["compiler_exit"]==1 and "I_INTERNAL" in record["stderr"] and not record["elf_created"],record
    assert RESULTS[3]["compiler_exit"]==0 and RESULTS[3]["program_exit"]==0,RESULTS[3]
    assert RESULTS[4]["compiler_exit"]==0 and RESULTS[4]["program_exit"]==97,RESULTS[4]
    if int(REVISION)>=2:
        assert RESULTS[5]["compiler_exit"]==1 and "I_INTERNAL" in RESULTS[5]["stderr"] and not RESULTS[5]["elf_created"],RESULTS[5]
        assert RESULTS[6]["compiler_exit"]==0 and RESULTS[6]["program_exit"]==101,RESULTS[6]
    if int(REVISION)>=3:
        assert RESULTS[7]["compiler_exit"]==0 and RESULTS[7]["program_exit"]==101,RESULTS[7]
print(json.dumps({"revision":REVISION,"results":RESULTS},indent=2))
