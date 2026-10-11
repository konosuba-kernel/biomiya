#![cfg(all(feature = "pipeline", feature = "image"))]
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
const BIN: &str = env!("CARGO_BIN_EXE_core-s");
static SERIAL: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "core-s-b3-frontend-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn run(command: &mut Command) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("required tool");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!(
                "{command:?} timeout: {:?}",
                child.wait_with_output().unwrap()
            );
        }
        thread::sleep(Duration::from_millis(5));
    }
}
fn success(command: &mut Command) -> Output {
    let o = run(command);
    assert!(
        o.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    o
}
fn executable(path: &Path, data: &[u8]) {
    fs::write(path, data).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}
const FRONTEND: &[&str] = &[
    "bootstrap/model.vt",
    "bootstrap/rules.vt",
    "bootstrap/lex.vt",
    "bootstrap/input.vt",
    "bootstrap/types.vt",
    "bootstrap/parse.vt",
    "bootstrap/ir.vt",
    "bootstrap/check.vt",
    "runtime/bytes.vt",
    "runtime/linux.vt",
];
const PROBE: &str = r#"module probe;
import bootstrap::model;
import bootstrap::input;
import bootstrap::parse;
import bootstrap::check;
import bootstrap::types;
import bootstrap::lex;
import bootstrap::ir;
import runtime::linux;
pub fn run()->u32 {
 model::reset_all();
 var path:[]const u8="";
 match linux::argument(1) {
  Result::<[]const u8,u32>::Ok(p:[]const u8)=>{path=p;},
  Result::<[]const u8,u32>::Err(e:u32)=>{return 2;},
 }
 input::load(path);
 parse::collect();
 unsafe {
  if model::decl_count==0_u32 {return 3;}
  var i:u32=1;
  while i<=model::decl_count {
   if model::decls[i as usize].kind==2_u32 {
    let d:model::Decl=model::decls[i as usize];
    let expected:model::Token=lex::next(d.file,d.end as usize);
    check::function(i);
    let actual:model::Token=types::token;
    if actual.file!=expected.file || actual.start!=expected.start || actual.end!=expected.end || actual.kind!=expected.kind || actual.number!=expected.number || actual.suffix!=expected.suffix {return 9;}
    if types::diagnostic.file!=d.file {return 10;}
    if linux::argc()==3_usize {
    let slots:u32=model::slot_count;
    let insts:u32=model::inst_count;
    let args:u32=model::arg_count;
    let labels:u32=ir::labels;
    check::function(i);
    if model::slot_count!=slots || model::inst_count!=insts || model::arg_count!=args || ir::labels!=labels || model::parameter_slot_count!=d.count {return 11;}
    }
    if model::function_decl!=i || model::inst_count==0_u32 {return 4;}
    if model::insts[1].op!=14_u32 || model::insts[1].a!=1_u32 {return 5;}
    var n:u32=1;var terminated:bool=true;
    while n<=model::inst_count {
     let op:u32=model::insts[n as usize].op;
     if op==14_u32 {if !terminated {return 6;}terminated=false;}
     else {if terminated {return 7;}if op>=15_u32 && op<=18_u32 {terminated=true;}}
     n=n+1_u32;
    }
    if !terminated {return 8;}
   }
   i=i+1_u32;
  }
 }
 return 0;
}
"#;
fn stage(t: &Scratch) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for f in FRONTEND.iter().copied().chain(["runtime/linux.s0"]) {
        let p = t.file(f);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::copy(root.join(f), p).unwrap();
    }
}
fn manifest(t: &Scratch, name: &str, files: &[&str]) {
    let mut s = "format 1\nprofile linux-user\nentry probe::run\noutput ignored.elf\n".to_owned();
    for f in files {
        s.push_str(&format!("source {f}\n"));
    }
    s.push_str("assembly runtime/linux.s0\n");
    fs::write(t.file(name), s).unwrap();
}
fn image() -> &'static [u8] {
    static IMAGE: OnceLock<Vec<u8>> = OnceLock::new();
    IMAGE.get_or_init(|| {
        let t = Scratch::new();
        stage(&t);
        fs::write(t.file("probe.vt"), PROBE).unwrap();
        let mut files = FRONTEND.to_vec();
        files.push("probe.vt");
        manifest(&t, "probe.build", &files);
        success(
            Command::new(BIN)
                .args(["project", "probe.build"])
                .current_dir(&t.0)
                .env("PATH", ""),
        );
        fs::read(t.file("ignored.elf")).unwrap()
    })
}
fn check(files: &[(&str, &str)]) -> Output {
    check_with_repeat(files, false)
}
fn check_with_repeat(files: &[(&str, &str)], repeat: bool) -> Output {
    let t = Scratch::new();
    stage(&t);
    let mut names = Vec::new();
    for (name, src) in files {
        fs::write(t.file(name), src).unwrap();
        names.push(*name);
    }
    manifest(&t, "test.build", &names);
    executable(&t.file("probe.elf"), image());
    let mut command = Command::new("qemu-riscv64");
    command
        .args(["-s", "1048576"])
        .arg(t.file("probe.elf"))
        .arg("test.build")
        .current_dir(&t.0);
    if repeat {
        command.arg("repeat");
    }
    let o = run(&mut command);
    assert!(!t.file("ignored.elf").exists());
    o
}
#[test]
fn frontend_checks_actual_bootstrap_sources_and_ir_block_contract() {
    let t = Scratch::new();
    stage(&t);
    manifest(&t, "test.build", FRONTEND);
    executable(&t.file("probe.elf"), image());
    let o = run(Command::new("qemu-riscv64")
        .args(["-s", "1048576"])
        .arg(t.file("probe.elf"))
        .arg("test.build")
        .current_dir(&t.0));
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(!t.file("ignored.elf").exists());
}
#[test]
fn frontend_accepts_forward_layout_snapshots_results_and_cleanup() {
    let cases = [
        "struct Pair{a:u32,b:u32} const N:usize=M; const M:usize=3; static values:[N]Pair=[Pair{a:1,b:2};N]; fn f(x:Pair)->Pair{return x;} pub fn run()->u32{var x:Pair=Pair{a:3,b:4};let y:Pair=f(x);return y.a;}",
        "static mut count:u32=0;fn fail()->Result<u32,u32>{return Result::<u32,u32>::Err(7);}fn go()->Result<u32,u32>{defer{unsafe{count=count+1_u32;}}let n:u32=fail()?;return Result::<u32,u32>::Ok(n);}pub fn run()->u32{match go(){Result::<u32,u32>::Ok(n:u32)=>{return n;},Result::<u32,u32>::Err(e:u32)=>{return e;},}}",
        "pub fn run()->u32{var a:[3]u8=[1,2,3];let s:[]u8=core::slice_of_mut(a);var i:usize=0;while i<s.len {unsafe{s[i]=s[i]+%1_u8;}i=i+1_usize;}unsafe{let p:*u8=core::ptr_add(s.data,1);let q:*const u8=core::ptr_const(p);return *q as u32;}}",
        "pub fn run()->u32{var n:u32=0;loop{defer{n=n+1_u32;}if n==2_u32{break;}continue;}return n;}",
    ];
    for src in cases {
        let o = check(&[("main.vt", src)]);
        assert_eq!(
            o.status.code(),
            Some(0),
            "{src}\n{}",
            String::from_utf8_lossy(&o.stderr)
        );
    }
    let o = check(&[
        (
            "a.vt",
            "module a;pub struct P{pub n:u32} pub fn get()->P{return P{n:5};}",
        ),
        (
            "b.vt",
            "module b;import a;pub fn run()->u32{return a::get().n;}",
        ),
    ]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}
#[test]
fn frontend_merges_module_imports_and_accepts_prefix_namespaces() {
    let o = check(&[
        (
            "page.vt",
            "module memory::page; pub fn get()->u32{return 3;}",
        ),
        ("imports.vt", "module app; import memory;"),
        (
            "main.vt",
            "module app; pub fn run()->u32{return memory::page::get();}",
        ),
    ]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let o = check(&[
        ("root.vt", "fn a()->u32{return 0;}"),
        ("child.vt", "module a::child;"),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("E_NAME"));
}

#[test]
fn frontend_rejects_invalid_programs_with_source_diagnostics() {
    let cases = [
        ("pub fn run( {", "E_PARSE"),
        ("pub fn run()->u32{return absent;}", "E_NAME"),
        ("pub fn run()->u32{return true;}", "E_TYPE"),
        ("pub fn run()->u32{let x:u32=1;x=2;return x;}", "E_TYPE"),
        (
            "pub fn run()->u32{let x:u32=1;let p:*const u32=core::addr_of(x);return *p;}",
            "E_PERMISSION",
        ),
        (
            "struct Infinite{next:Infinite} pub fn run()->u32{return 0;}",
            "E_TYPE",
        ),
        ("pub fn run()->u32{let x:u64=0;return 0;}", "I_UNSUPPORTED"),
    ];
    for (src, code) in cases {
        let o = check(&[("main.vt", src)]);
        assert_eq!(
            o.status.code(),
            Some(1),
            "{src}\n{}",
            String::from_utf8_lossy(&o.stderr)
        );
        let d = String::from_utf8_lossy(&o.stderr);
        assert!(d.contains(code), "{src}\n{d}");
        assert!(d.contains("file=1 byte="), "{d}");
    }
    let o = check(&[
        ("a.vt", "module a;fn secret()->u32{return 1;}"),
        (
            "b.vt",
            "module b;import a;pub fn run()->u32{return a::secret();}",
        ),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("E_NAME"));
}
#[test]
fn frontend_static_layout_keeps_large_zero_arrays_in_bss() {
    let t = Scratch::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sources = [
        "bootstrap/model.vt",
        "bootstrap/rules.vt",
        "bootstrap/lex.vt",
        "bootstrap/input.vt",
        "bootstrap/types.vt",
        "bootstrap/parse.vt",
        "runtime/bytes.vt",
        "runtime/linux.vt",
    ];
    for f in sources.iter().copied().chain(["runtime/linux.s0"]) {
        let p = t.file(f);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::copy(root.join(f), p).unwrap();
    }
    let probe = r#"module probe;import bootstrap::model;import bootstrap::input;import bootstrap::parse;
 pub fn run()->u32{model::reset_all();input::load("test.build");parse::collect();unsafe{
  var n:u32=1;var found:bool=false;
  while n<=model::data_count{let d:model::Data=model::data_objs[n as usize];if d.size==17825792_usize{if d.region!=4_u32||d.count!=0_usize{return 2;}found=true;}n=n+1_u32;}
  if !found{return 3;}if model::data_byte_count>1048576_usize{return 4;}
  if model::data_relocation_count<8193_u32{return 5;}
  let target:u32=model::data_relocations[1].target;
  n=2;while n<=model::data_relocation_count{if model::data_relocations[n as usize].target!=target{return 6;}n=n+1_u32;}
 }return 0;}"#;
    fs::write(t.file("probe.vt"), probe).unwrap();
    let mut all = sources.to_vec();
    all.push("probe.vt");
    manifest(&t, "probe.build", &all);
    success(
        Command::new(BIN)
            .args(["project", "probe.build"])
            .current_dir(&t.0)
            .env("PATH", ""),
    );
    fs::write(t.file("data.vt"),"struct Name{bytes:[]const u8,hash:usize}static mut names:[8193]Name=[Name{bytes:\"\",hash:0};8193];static mut big:[17825792]u8=[0;17825792];").unwrap();
    manifest(&t, "test.build", &["data.vt"]);
    let o = run(Command::new("qemu-riscv64")
        .args(["-s", "1048576"])
        .arg(t.file("ignored.elf"))
        .current_dir(&t.0));
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}
#[test]
fn frontend_depth_limits_and_type_shadowing_fail_before_guest_stack_overflow() {
    let sources = [
        format!(
            "fn f()->u32{{return {}0{};}}",
            "(".repeat(100),
            ")".repeat(100)
        ),
        format!(
            "fn f()->u32{{{}return 0;{}}}",
            "{".repeat(80),
            "}".repeat(80)
        ),
        format!(
            "static A:{}u8=null;fn f()->u32{{return 0;}}",
            "*".repeat(100)
        ),
        (0..40)
            .map(|i| format!("struct A{i}{{x:A{}}}", i + 1))
            .collect::<String>()
            + "struct A40{x:u8}fn f()->u32{return 0;}",
    ];
    for source in sources {
        let o = check(&[("deep.vt", &source)]);
        assert_eq!(
            o.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
        let error = String::from_utf8_lossy(&o.stderr);
        for expected in ["I_LIMIT", "actual=", "limit=", "file=1 byte="] {
            assert!(error.contains(expected), "{error}");
        }
    }
    let o = check(&[(
        "shadow.vt",
        "struct A{x:u32}fn f()->u32{let A:u32=0;let x:A=A{x:1};return x.x;}",
    )]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("E_TYPE"));
}
#[test]
fn frontend_reports_permission_and_capacity_errors_without_target_panics() {
    for (source, expected) in [
        ("static A:[0]u32=[\"x\";0];fn f()->u32{return 0;}", "E_TYPE"),
        (
            "static A:[0]*const u8=[core::addr_of(missing);0];fn f()->u32{return 0;}",
            "E_NAME",
        ),
    ] {
        let o = check(&[("zero.vt", source)]);
        assert_eq!(o.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&o.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
    }

    let o = check(&[(
        "permission.vt",
        "static mut A:[2]u8=[0;2];fn f()->usize{return A.len;}",
    )]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("E_PERMISSION"));
    for source in [
        "static mut A:[4294967296_usize]u8=[0;4294967296_usize];fn f()->u32{return 0;}",
        "static A:[1]u8=[0;4294967296_usize];fn f()->u32{return 0;}",
    ] {
        let o = check(&[("count.vt", source)]);
        assert_eq!(o.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&o.stderr)
                .contains("actual=4294967296 limit=4294967295 I_LIMIT")
        );
    }
    let mut hierarchy = "struct A0{".to_owned()
        + &(0..256)
            .map(|n| format!("f{n}:[67108864]u8,"))
            .collect::<String>()
        + "}";
    for level in 1..=5 {
        hierarchy.push_str(&format!("struct A{level}{{"));
        for field in 0..256 {
            hierarchy.push_str(&format!("f{field}:A{},", level - 1));
        }
        hierarchy.push('}');
    }
    hierarchy.push_str("fn f()->u32{return 0;}");
    let o = check(&[("layout.vt", &hierarchy)]);
    assert_eq!(
        o.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let error = String::from_utf8_lossy(&o.stderr);
    assert!(
        error.contains("I_LIMIT") && error.contains("actual=") && error.contains("limit="),
        "{error}"
    );
    let oversized = "struct Big{".to_owned()
        + &(0..256)
            .map(|n| format!("f{n}:[67108864]u8,"))
            .collect::<String>()
        + "}static mut A:[4294967295]Big=[Big{"
        + &(0..256)
            .map(|n| format!("f{n}:[0;67108864],"))
            .collect::<String>()
        + "};4294967295];fn f()->u32{return 0;}";
    let o = check(&[("overflow.vt", &oversized)]);
    assert_eq!(
        o.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let error = String::from_utf8_lossy(&o.stderr);
    assert!(
        error.contains("I_LIMIT") && error.contains("actual=") && error.contains("limit="),
        "{error}"
    );
    for source in [
        format!("fn f()->u32{{var a:u32=0;return a{};}}", ".x".repeat(3000)),
        format!("fn f()->u32{{var a:u32=0;return a{};}}", "[0]".repeat(3000)),
        format!(
            "fn f()->u32{{{}{{}}return 0;}}",
            "if false {} else ".repeat(3000)
        ),
        format!(
            "static N:u32={}1_u32;fn f()->u32{{return 0;}}",
            "1_u32+".repeat(3000)
        ),
    ] {
        let o = check(&[("deep.vt", &source)]);
        assert_eq!(
            o.status.code(),
            Some(1),
            "deep input crashed: {:?}\n{}",
            o.status,
            String::from_utf8_lossy(&o.stderr)
        );
        let error = String::from_utf8_lossy(&o.stderr);
        assert!(
            error.contains("I_LIMIT") || error.contains("E_TYPE"),
            "{error}"
        );
    }
}

#[test]
fn frontend_separates_cursor_diagnostics_and_reuses_function_arenas() {
    let source = "struct Outer{inner:Inner,tail:u32} struct Inner{leaf:Leaf} struct Leaf{value:u32} const N:usize=M; const M:usize=K; const K:usize=2; fn heavy(x:u32)->u32{var a:[N]u32=[x;N];var n:u32=0;defer{n=n+1_u32;}while n<1_u32{unsafe{let p:*u32=core::addr_of_mut(n);*p=*p+1_u32;}if n==1_u32{break;}}return a[0]+n;} fn tiny()->u32{return 7;} pub fn run()->u32{let v:Outer=Outer{inner:Inner{leaf:Leaf{value:3}},tail:4};return heavy(v.inner.leaf.value)+tiny();}";
    let o = check_with_repeat(&[("main.vt", source)], true);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

#[test]
fn frontend_reports_exact_ast_and_recursive_header_locations() {
    let cases = [
        ("pub fn run()->u32{return missing;}", "E_NAME", "missing"),
        ("pub fn run()->u32{return true;}", "E_TYPE", "true"),
        (
            "pub fn run()->u32{let n:u32=0;return *core::addr_of(n);}",
            "E_PERMISSION",
            "*core",
        ),
        (
            "static bad:[2]u32=[1_u32,true];pub fn run()->u32{return 0;}",
            "E_TYPE",
            "true",
        ),
        (
            "struct A{b:B} struct B{a:A} pub fn run()->u32{return 0;}",
            "E_TYPE",
            "} pub",
        ),
        (
            "struct Outer{bad:[67108865]Inner}struct Inner{v:u8}pub fn run()->u32{return 0;}",
            "I_LIMIT",
            "}struct",
        ),
        (
            "const A:u32=B; const B:bool=true;pub fn run()->u32{return 0;}",
            "E_TYPE",
            "; const B",
        ),
    ];
    for (source, code, marker) in cases {
        let o = check(&[
            ("prefix.vt", "module prefix;pub const N:u32=0;"),
            ("main.vt", source),
        ]);
        assert_eq!(o.status.code(), Some(1));
        let diagnostic = String::from_utf8_lossy(&o.stderr);
        let expected = format!("{code} file=2 byte={}", source.find(marker).unwrap());
        assert!(
            diagnostic.contains(&expected),
            "expected {expected}: {diagnostic}"
        );
    }
}

#[test]
fn pure_rules_use_only_explicit_scalar_and_layout_inputs() {
    let t = Scratch::new();
    stage(&t);
    let probe = r#"module probe;
import bootstrap::model;
import bootstrap::rules;
pub fn run()->u32 {
 if !rules::literal(255,model::TYPE_U8).ok || rules::literal(256,model::TYPE_U8).ok {return 1;}
 if rules::literal(1,model::TYPE_BOOL).ok {return 2;}
 if rules::unary(model::UNARY_NOT,1,model::TYPE_BOOL).bits!=0_usize {return 3;}
 if rules::unary(model::UNARY_BIT_NOT,0,model::TYPE_U8).bits!=255_usize {return 4;}
 if !rules::unary(model::UNARY_NEG,0,model::TYPE_U8).ok || rules::unary(model::UNARY_NEG,1,model::TYPE_U8).ok {return 5;}
 if rules::cast(256,model::TYPE_U32,model::TYPE_U8).bits!=0_usize || rules::cast(1,model::TYPE_BOOL,model::TYPE_U8).ok {return 6;}
 if rules::binary(model::BIN_ADD,255,1,model::TYPE_U8).ok || rules::binary(model::BIN_WRAP_ADD,255,1,model::TYPE_U8).bits!=0_usize {return 7;}
 if rules::binary(model::BIN_SUB,0,1,model::TYPE_U8).ok || rules::binary(model::BIN_WRAP_SUB,0,1,model::TYPE_U8).bits!=255_usize {return 8;}
 if rules::binary(model::BIN_MUL,128,2,model::TYPE_U8).ok || rules::binary(model::BIN_WRAP_MUL,128,2,model::TYPE_U8).bits!=0_usize {return 9;}
 if rules::binary(model::BIN_DIV,1,0,model::TYPE_U8).ok || rules::binary(model::BIN_REM,1,0,model::TYPE_U8).ok {return 10;}
 if rules::binary(model::BIN_DIV,7,2,model::TYPE_U8).bits!=3_usize || rules::binary(model::BIN_REM,7,2,model::TYPE_U8).bits!=1_usize {return 11;}
 if rules::binary(model::BIN_SHL,1,8,model::TYPE_U8).ok || rules::binary(model::BIN_SHR,128,7,model::TYPE_U8).bits!=1_usize {return 12;}
 if rules::binary(model::BIN_BIT_AND,3,1,model::TYPE_U8).bits!=1_usize || rules::binary(model::BIN_BIT_OR,2,1,model::TYPE_U8).bits!=3_usize || rules::binary(model::BIN_BIT_XOR,3,1,model::TYPE_U8).bits!=2_usize {return 13;}
 if rules::binary(model::BIN_EQ,1,1,model::TYPE_U8).bits!=1_usize || rules::binary(model::BIN_NE,1,2,model::TYPE_U8).bits!=1_usize {return 14;}
 if rules::binary(model::BIN_LT,1,2,model::TYPE_U8).bits!=1_usize || rules::binary(model::BIN_LE,1,1,model::TYPE_U8).bits!=1_usize || rules::binary(model::BIN_GT,2,1,model::TYPE_U8).bits!=1_usize || rules::binary(model::BIN_GE,2,2,model::TYPE_U8).bits!=1_usize {return 15;}
 if rules::binary(model::BIN_AND,1,0,model::TYPE_BOOL).bits!=0_usize || rules::binary(model::BIN_OR,1,0,model::TYPE_BOOL).bits!=1_usize {return 16;}
 if rules::binary(model::BIN_INVALID,1,1,model::TYPE_U8).ok {return 17;}
 if rules::binary(model::BIN_EQ,1,0,model::TYPE_UNIT).ok || !rules::binary(model::BIN_EQ,0,0,model::TYPE_UNIT).ok {return 21;}
 if !rules::add_size(0xffffffffffffffff,0).ok || rules::add_size(0xffffffffffffffff,1).ok {return 18;}
 if rules::align_up(1,0).ok || rules::align_up(1,3).ok || rules::align_up(0xffffffffffffffff,8).ok {return 19;}
 if rules::align_up(67108865,4096).size!=67112960_usize || rules::align_up(0xffffffffffffffff,1).size!=0xffffffffffffffff_usize {return 20;}
 return 0;
}"#;
    fs::write(t.file("probe.vt"), probe).unwrap();
    manifest(
        &t,
        "probe.build",
        &["bootstrap/model.vt", "bootstrap/rules.vt", "probe.vt"],
    );
    success(
        Command::new(BIN)
            .args(["project", "probe.build"])
            .current_dir(&t.0),
    );
    let o = run(Command::new("qemu-riscv64")
        .args(["-s", "1048576"])
        .arg(t.file("ignored.elf")));
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

#[test]
fn constant_walkers_agree_on_values_and_reject_boundaries() {
    let t = Scratch::new();
    stage(&t);
    let probe = r#"module probe;
import bootstrap::model;
import bootstrap::input;
import bootstrap::parse;
import runtime::linux;
pub fn run()->u32 {
 model::reset_all();
 match linux::argument(1) {
  Result::<[]const u8,u32>::Ok(path:[]const u8)=>{input::load(path);},
  Result::<[]const u8,u32>::Err(e:u32)=>{return 2;},
 }
 parse::collect();
 unsafe {
  var c:usize=0;var expected:usize=0;var symbol:u32=0;var size:usize=0;var i:u32=1;
  while i<=model::decl_count {
   let d:model::Decl=model::decls[i as usize];
   if d.name==model::name("C") {c=d.bits;}
   if d.name==model::name("EXPECTED") {expected=d.bits;}
   if d.name==model::name("S") {symbol=d.symbol;size=model::types[d.ty as usize].size;}
   i=i+1_u32;
  }
  if c!=expected || symbol==0_u32 {return 3;}
  i=1;
  while i<=model::data_count {
   let d:model::Data=model::data_objs[i as usize];
   if d.symbol==symbol {
    var bits:usize=0;var n:usize=0;
    while n<size {bits=bits|((model::data_bytes[d.first+n] as usize)<<(n*8_usize));n=n+1_usize;}
    if bits!=expected {return 4;}return 0;
   }
   i=i+1_u32;
  }
 }
 return 5;
}"#;
    fs::write(t.file("probe.vt"), probe).unwrap();
    let mut files = FRONTEND.to_vec();
    files.push("probe.vt");
    manifest(&t, "probe.build", &files);
    success(
        Command::new(BIN)
            .args(["project", "probe.build"])
            .current_dir(&t.0),
    );
    let cases = [
        ("u8", "255_u8+%1_u8", Some(0u64)),
        ("u8", "~0_u8", Some(255)),
        ("u8", "-0_u8", Some(0)),
        ("u8", "256_u32 as u8", Some(0)),
        ("u8", "1_u8<<7_u8", Some(128)),
        ("bool", "1_u8==1_u8", Some(1)),
        ("bool", "!false", Some(1)),
        ("usize", "18446744073709551615_usize+%1_usize", Some(0)),
        ("u8", "255_u8+1_u8", None),
        ("u8", "-1_u8", None),
        ("u8", "1_u8<<8_u8", None),
        ("u8", "1_u8/0_u8", None),
        ("u8", "true as u8", None),
        ("usize", "18446744073709551615_usize+1_usize", None),
    ];
    for (ty, expression, expected) in cases {
        for walker in 0..if expected.is_some() { 1 } else { 2 } {
            let (constant, static_) = if expected.is_some() {
                (expression, expression)
            } else if walker == 0 {
                (expression, "0")
            } else {
                ("0", expression)
            };
            fs::write(t.file("values.vt"), format!("const C:{ty}={constant};static S:{ty}={static_};const EXPECTED:usize={};pub fn run()->u32{{return 0;}}", expected.unwrap_or(0))).unwrap();
            manifest(&t, "values.build", &["values.vt"]);
            let o = run(Command::new("qemu-riscv64")
                .args(["-s", "1048576"])
                .arg(t.file("ignored.elf"))
                .arg("values.build")
                .current_dir(&t.0));
            assert_eq!(
                o.status.code(),
                Some(if expected.is_some() { 0 } else { 1 }),
                "{ty} {expression} walker {walker}: {}",
                String::from_utf8_lossy(&o.stderr)
            );
            if expected.is_none() {
                assert!(String::from_utf8_lossy(&o.stderr).contains("E_TYPE"));
            }
        }
    }
}

#[test]
fn static_validation_visits_invalid_tails_and_empty_repeats() {
    for source in [
        "static S:[2]u32=[1_u32,true];pub fn run()->u32{return 0;}",
        "struct P{a:u32,b:u32}static S:P=P{a:1,b:true};pub fn run()->u32{return 0;}",
        "static S:[0]u32=[true;0];pub fn run()->u32{return 0;}",
        "static S:[0]u32=[1_u32/0_u32;0];pub fn run()->u32{return 0;}",
        "pub fn run()->u32{return -0_u32;}",
    ] {
        let o = check(&[("main.vt", source)]);
        assert_eq!(o.status.code(), Some(1), "{source}");
        assert!(String::from_utf8_lossy(&o.stderr).contains("E_TYPE"));
    }
}
