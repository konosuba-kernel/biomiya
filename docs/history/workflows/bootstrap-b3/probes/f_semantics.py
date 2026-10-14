#!/usr/bin/env python3
"""Independent F acceptance: real source -> F IR -> accepted M -> guest behavior."""
import pathlib,json,hashlib,subprocess,tempfile,sys
ROOT=pathlib.Path(__file__).resolve().parents[4];CORE=ROOT/"core-s"
REV=sys.argv[1] if len(sys.argv)>1 else "1"
for path,digest in json.loads((ROOT/f"docs/workflows/bootstrap-b3/F-r{REV}-candidate.json").read_text())["files"].items():
    assert hashlib.sha256((ROOT/path).read_bytes()).hexdigest()==digest,path
DRIVER='''module probe;
import bootstrap::model;import bootstrap::input;import bootstrap::parse;import bootstrap::check;
import bootstrap::image;import bootstrap::rv64;import bootstrap::s0;
pub fn run()->u32 {
 model::reset_all();input::load("test.build");parse::collect();image::reset();
 unsafe {
  var id:u32=1;var target:u32=0;
  while id<=model::decl_count {let d:model::Decl=model::decls[id as usize];if d.kind==2_u32 && model::equal(model::names[d.name as usize].bytes,"run"){target=d.symbol;} id=id+1_u32;}
  if target==0_u32{return 90;}
  let entry:u32=rv64::start(target);id=1;
  while id<=model::decl_count {if model::decls[id as usize].kind==2_u32 {check::function(id);rv64::function(id);}id=id+1_u32;}
  id=1;while id<=model::assembly_count{s0::parse(model::source_count+id,model::assemblies[id as usize].bytes);id=id+1_u32;}
  input::save(image::finish(entry));
 }
 return 0;
}
'''
CASES=[
("aggregate_argument_snapshot",'''struct P{n:u32} static mut p:P=P{n:7};fn mutate()->u32{unsafe{p.n=99;}return 1;}fn f(a:P,b:u32)->u32{return a.n+b;}fn run()->u32{unsafe{return f(p,mutate());}}''',8),
("return_snapshot",'''struct P{n:u32} static mut n:u32=0;fn f()->P{var p:P=P{n:7};defer{p.n=9;unsafe{n=n+1_u32;}}return p;}fn run()->u32{let p:P=f();unsafe{return p.n*10_u32+n;}}''',71),
("rhs_before_lhs",'''static mut n:usize=0;fn ix()->usize{unsafe{let r:usize=n;n=n+1_usize;return r;}}fn rhs()->u32{unsafe{n=1;}return 7;}fn run()->u32{var a:[3]u32=[1,2,3];a[ix()]=rhs();return a[0]*10_u32+a[1];}''',17),
("defer_lifo",'''static mut n:u32=0;fn put(x:u32)->(){unsafe{n=n*10_u32+x;}return ();}fn f()->(){defer{put(1);}defer{put(2);}return ();}fn run()->u32{f();unsafe{return n;}}''',21),
("loop_cleanup",'''static mut n:u32=0;fn f()->(){defer{unsafe{n=n+100_u32;}}var i:u32=0;while i<3_u32 {defer{unsafe{n=n+1_u32;}}i=i+1_u32;if i==2_u32{continue;}if i==3_u32{break;}}return ();}fn run()->u32{f();unsafe{return n;}}''',103),
("try_cleanup",'''static mut n:u32=0;fn bad()->Result<u32,u32>{return Result::<u32,u32>::Err(7);}fn f()->Result<u32,u32>{defer{unsafe{n=n+1_u32;}}let x:u32=bad()?;return Result::<u32,u32>::Ok(x);}fn run()->u32{match f(){Result::<u32,u32>::Err(e:u32)=>{unsafe{return e*10_u32+n;}},Result::<u32,u32>::Ok(v:u32)=>{return v;},}}''',71),
("short_circuit",'''static mut n:u32=0;fn bump()->bool{unsafe{n=n+1_u32;}return true;}fn run()->u32{let a:bool=false&&bump();let b:bool=true||bump();unsafe{return n;}}''',0),
("captured_lhs_call",'''static mut n:usize=0;fn ix(x:usize)->usize{unsafe{n=n+1_usize;}return x;}fn rhs(x:u32)->u32{unsafe{n=n+10_usize;}return x;}fn run()->u32{var a:[2]u32=[0;2];a[ix(1)]=rhs(7);unsafe{return a[1]+(n as u32);}}''',18),
("match_scrutinee_once",'''static mut n:u32=0;fn get()->Result<u32,u32>{unsafe{n=n+1_u32;}return Result::<u32,u32>::Ok(7);}fn run()->u32{match get(){Result::<u32,u32>::Err(e:u32)=>{return e;},Result::<u32,u32>::Ok(v:u32)=>{unsafe{return v*10_u32+n;}},}}''',71),
("bounds",'''fn run()->u32{let a:[2]u32=[1,2];return a[2];}''',101),
("slice_projection",'''fn run()->u32{var a:[3]u8=[1,2,3];let s:[]u8=core::slice_of_mut(a);unsafe{s[1]=9;let c:[]const u8=core::slice_const(s);return (c[1] as u32)+(c.len as u32);}}''',12),
("const_loss",'''fn run()->u32{let a:[2]u8=[1,2];let s:[]const u8=core::slice_of(a);unsafe{let bad:[]u8=core::slice_from_raw_parts_mut(s.data,s.len);return 0;}}''',"E_TYPE"),
("unsafe_required",'''static mut n:u32=0;fn run()->u32{return n;}''',"E_PERMISSION"),
("defer_return_rejected",'''fn run()->u32{defer{return 1;}return 0;}''',"E_TYPE"),
("defer_binding_snapshot",'''static mut n:u32=0;fn f()->(){var x:u32=1;defer{unsafe{n=x;}}x=7;return ();}fn run()->u32{f();unsafe{return n;}}''',7),
("nested_defer_branches",'''static mut n:u32=0;fn f(b:bool)->u32{defer{if b{unsafe{n=n+1_u32;}}else{unsafe{n=n+2_u32;}}}if b{return 3;}return 4;}fn run()->u32{let a:u32=f(true);let b:u32=f(false);unsafe{return a*10_u32+b+n;}}''',37),
("mutable_static_array_len",'''static mut A:[2]u8=[0;2];fn run()->u32{return A.len as u32;}''',"E_PERMISSION"),
("aggregate_alias_snapshot",'''struct P{a:u32,b:u32}fn run()->u32{var p:P=P{a:1,b:2};let ptr:*P=core::addr_of_mut(p);unsafe{(*ptr)=P{a:(*ptr).b,b:(*ptr).a};}return p.a*10_u32+p.b;}''',21),
("defer_nested_scope",'''static mut n:u32=0;fn f()->u32{defer{unsafe{n=n+1_u32;}}{defer{unsafe{n=n+10_u32;}}return 7;}return 9;}fn run()->u32{let v:u32=f();unsafe{return v*10_u32+root::n;}}''',81),
("known_seed_root_shadow_bug",'''static mut n:u32=0;fn f()->u32{defer{unsafe{n=n+1_u32;}}{defer{unsafe{n=n+10_u32;}}return 7;}return 9;}fn run()->u32{let n:u32=f();unsafe{return n*10_u32+root::n;}}''',81),
("try_return_snapshot",'''struct E{n:u32}static mut e:E=E{n:7};fn bad()->Result<u32,E>{unsafe{return Result::<u32,E>::Err(e);}}fn f()->Result<u32,E>{defer{unsafe{e.n=99;}}let n:u32=bad()?;return Result::<u32,E>::Ok(n);}fn run()->u32{match f(){Result::<u32,E>::Ok(n:u32)=>{return n;},Result::<u32,E>::Err(v:E)=>{return v.n;},}}''',7),
("target_only_array_capacity",'''static mut A:[4294967296_usize]u8=[0;4294967296_usize];fn run()->u32{return 0;}''',"I_LIMIT"),
("target_only_deep_fields",'fn run()->u32{var a:u32=0;return a'+'.x'*3000+';}',"I_LIMIT"),
("target_only_deep_index",'fn run()->u32{var a:u32=0;return a'+'[0]'*3000+';}',"I_LIMIT"),
("target_only_size_overflow",'struct Big{'+','.join(f'f{i}:[67108864]u8' for i in range(256))+'}static mut A:[4294967295]Big=[Big{'+','.join(f'f{i}:[0;67108864]' for i in range(256))+'};4294967295];fn run()->u32{return 0;}',"I_LIMIT"),
("static_repeat_string_identity",'''static S:[2][]const u8=["x";2];fn run()->u32{unsafe{if core::addr(S[0].data)==core::addr(S[1].data){return 1;}return 2;}}''',1),
("static_repeat_nested_identity",'''struct R{names:[2][]const u8,n:u32}static A:[3]R=[R{names:["abc";2],n:7};3];fn run()->u32{unsafe{if core::addr(A[0].names[0].data)!=core::addr(A[2].names[1].data){return 99;}return A[2].n+(A[1].names[1].len as u32);}}''',10),
("static_repeat_pointer_and_string",'''static N:u32=5;struct R{p:*const u32,s:[]const u8}static A:[3]R=[R{p:core::addr_of(N),s:"xy"};3];fn run()->u32{unsafe{if A[0].p!=A[2].p||core::addr(A[0].s.data)!=core::addr(A[2].s.data){return 99;}return *A[2].p+(A[1].s.len as u32);}}''',7),
("static_repeat_zero_bad_string",'''static S:[0]u8=["x";0];fn run()->u32{return 0;}''',"E_TYPE"),
("static_repeat_zero_bad_address",'''static N:u32=1;static S:[0]*u32=[core::addr_of(N);0];fn run()->u32{return 0;}''',"E_TYPE"),
]
if len(sys.argv)>2:
    CASES=[case for case in CASES if case[0].startswith(sys.argv[2])]
    assert CASES,"Unknown case filter"
RESULTS=[]
with tempfile.TemporaryDirectory(prefix="b3-f-review-") as temporary:
    root=pathlib.Path(temporary)
    paths=["bootstrap/"+p+".vt" for p in ["model","lex","input","types","parse","ir","check","image","rv64","s0"]]+["runtime/bytes.vt","runtime/linux.vt"]
    for path in paths+["runtime/linux.s0"]:
        dest=root/path;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes((CORE/path).read_bytes())
    (root/"probe.vt").write_text(DRIVER)
    (root/"build").write_text("format 1\nprofile linux-user\nentry probe::run\n"+"".join("source "+p+"\n" for p in paths)+"source probe.vt\nassembly runtime/linux.s0\noutput driver.elf\n")
    subprocess.run([str(CORE/"target/debug/core-s"),"project","build"],cwd=root,check=True,capture_output=True)
    for name,source,expected in CASES:
        case=root/name;case.mkdir();(case/"input.vt").write_text(source)
        (case/"test.build").write_text("format 1\nprofile linux-user\nentry run\nsource input.vt\noutput result.elf\n")
        built=subprocess.run(["/usr/bin/qemu-riscv64","-s","1048576",str(root/"driver.elf")],cwd=case,capture_output=True,timeout=30,env={"PATH":""})
        seed=subprocess.CompletedProcess([],1,b'',b'Not run: target-only resource-limit probe') if name.startswith("target_only_") else subprocess.run([str(CORE/"target/debug/core-s"),"build","input.vt","--entry","run","-o","seed.elf"],cwd=case,capture_output=True,timeout=30)
        record={"name":name,"expected":expected,"target_compile_exit":built.returncode,"target_stderr":built.stderr.decode(),"seed_compile_exit":seed.returncode,"seed_stderr":seed.stderr.decode()}
        for compiler,output,compiled in [("target","result.elf",built),("seed","seed.elf",seed)]:
            if compiled.returncode==0:
                executed=subprocess.run(["/usr/bin/qemu-riscv64",str(case/output)],capture_output=True,timeout=10)
                record[compiler+"_program_exit"]=executed.returncode
        if isinstance(expected,int):
            record["passed"]=record.get("target_program_exit")==expected and record.get("seed_program_exit")==expected
            if name=="known_seed_root_shadow_bug":
                record["known_seed_bug"]="C0 incorrectly resolves explicit root::n to the same-name local n; target resolves the global correctly."
                record["passed"]=record.get("target_program_exit")==81 and record.get("seed_program_exit")==77
        else:
            record["passed"]=built.returncode==1 and expected in record["target_stderr"] and seed.returncode!=0 and not (case/"result.elf").exists()
        RESULTS.append(record)
print(json.dumps({"revision":REV,"results":RESULTS},indent=2))
assert all(r["passed"] for r in RESULTS), "Independent source semantics mismatched: "+str([r["name"] for r in RESULTS if not r["passed"]])
