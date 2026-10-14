//! B3 production fixed point: C0 -> RV64 C1 -> RV64 C2 -> RV64 C3.
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
const SOURCES: &[&str] = &[
    "bootstrap/model.vt",
    "bootstrap/rules.vt",
    "bootstrap/lex.vt",
    "bootstrap/input.vt",
    "bootstrap/types.vt",
    "bootstrap/parse.vt",
    "bootstrap/ir.vt",
    "bootstrap/check.vt",
    "bootstrap/image.vt",
    "bootstrap/rv64.vt",
    "bootstrap/s0.vt",
    "bootstrap/blocking.vt",
    "bootstrap/main.vt",
    "runtime/bytes.vt",
    "runtime/linux.vt",
];
static SERIAL: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "core-s-b3-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn file(&self, n: &str) -> PathBuf {
        self.0.join(n)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn run(cmd: &mut Command) -> Output {
    // Files avoid deadlocking a verbose syscall trace against a full pipe.
    let logs = Scratch::new();
    let out = logs.file("stdout");
    let err = logs.file("stderr");
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(fs::File::create(&out).unwrap())
        .stderr(fs::File::create(&err).unwrap())
        .spawn()
        .expect("required tool");
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return Output {
                status,
                stdout: fs::read(out).unwrap(),
                stderr: fs::read(err).unwrap(),
            };
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "{cmd:?} timed out: {}",
                String::from_utf8_lossy(&fs::read(err).unwrap())
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
}
fn success(cmd: &mut Command) -> Output {
    let o = run(cmd);
    assert!(
        o.status.success(),
        "{cmd:?}: {:?}\n{}",
        o.status,
        String::from_utf8_lossy(&o.stderr)
    );
    o
}
fn executable(p: &Path, bytes: &[u8]) {
    fs::write(p, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
    }
}
fn qemu() -> &'static Path {
    static QEMU: OnceLock<PathBuf> = OnceLock::new();
    QEMU.get_or_init(|| {
        let o = success(Command::new("which").arg("qemu-riscv64"));
        PathBuf::from(String::from_utf8(o.stdout).unwrap().trim())
    })
}
fn target(t: &Scratch, compiler: &str, mode: &str, manifest: &str, trace: bool) -> Output {
    let mut cmd = Command::new(qemu());
    cmd.args(["-s", "1048576"]);
    if trace {
        cmd.arg("-strace");
    }
    cmd.arg(t.file(compiler))
        .args([mode, manifest])
        .current_dir(&t.0)
        .env_clear()
        .env("PATH", "");
    run(&mut cmd)
}
fn stage(t: &Scratch) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for f in SOURCES.iter().copied().chain(["runtime/linux.s0"]) {
        let p = t.file(f);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::copy(root.join(f), p).unwrap();
    }
}
fn manifest(t: &Scratch, name: &str, entry: &str, files: &[&str], asm: &[&str], output: &str) {
    let mut s = format!("format 1\nprofile linux-user\nentry {entry}\n");
    for f in files {
        s.push_str(&format!("source {f}\n"));
    }
    for f in asm {
        s.push_str(&format!("assembly {f}\n"));
    }
    s.push_str(&format!("output {output}\n"));
    fs::write(t.file(name), s).unwrap();
}
fn elf_metrics(bytes: &[u8]) -> (u64, u64) {
    fn u64at(b: &[u8], at: usize) -> u64 {
        u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
    }
    let ph = u64at(bytes, 32) as usize;
    let count = u16::from_le_bytes(bytes[56..58].try_into().unwrap()) as usize;
    let mut data = 0;
    let mut span = 0;
    for n in 0..count {
        let p = ph + n * 56;
        let mem = u64at(bytes, p + 40);
        let addr = u64at(bytes, p + 16);
        span = span.max(addr + mem - 0x10000);
        if bytes[p + 4] & 2 != 0 {
            data += mem;
        }
    }
    (data, span)
}
struct Generations {
    images: [Vec<u8>; 3],
}
fn generations() -> &'static Generations {
    static ALL: OnceLock<Generations> = OnceLock::new();
    ALL.get_or_init(|| {
        let t = Scratch::new();
        stage(&t);
        manifest(
            &t,
            "self.build",
            "bootstrap::main::run",
            SOURCES,
            &["runtime/linux.s0"],
            "c1.elf",
        );
        success(
            Command::new(BIN)
                .args(["project", "self.build"])
                .current_dir(&t.0)
                .env("PATH", ""),
        );
        let c1 = fs::read(t.file("c1.elf")).unwrap();
        // Each target compiler sees only declared source inputs and the previous executable.
        let mut images = [c1, Vec::new(), Vec::new()];
        for stage_id in 0..2 {
            let clean = Scratch::new();
            stage(&clean);
            executable(&clean.file("compiler.elf"), &images[stage_id]);
            manifest(
                &clean,
                "self.build",
                "bootstrap::main::run",
                SOURCES,
                &["runtime/linux.s0"],
                "next.elf",
            );
            let o = target(&clean, "compiler.elf", "build", "self.build", true);
            assert!(
                o.status.success(),
                "generation {}: {:?}\n{}",
                stage_id + 2,
                o.status,
                String::from_utf8_lossy(&o.stderr)
            );
            let trace = String::from_utf8(o.stderr).unwrap();
            assert!(!trace.contains("execve("));
            assert!(
                !trace.contains("\"compiler.elf\""),
                "target opened own executable"
            );
            for line in trace.lines().filter(|l| l.contains("openat(")) {
                let name = line.split('"').nth(1).expect("trace path");
                assert!(
                    name == "self.build"
                        || name == "next.elf"
                        || name == "runtime/linux.s0"
                        || SOURCES.contains(&name),
                    "undeclared input: {line}"
                );
            }
            images[stage_id + 1] = fs::read(clean.file("next.elf")).unwrap();
        }
        assert_eq!(images[1], images[2], "C2 != C3");
        for image in &images {
            let (rw, span) = elf_metrics(image);
            assert!(rw <= 64 * 1024 * 1024, "RW budget {rw}");
            assert!(span <= 64 * 1024 * 1024, "image budget {span}");
        }
        Generations { images }
    })
}
#[test]
fn compiler_reaches_full_file_fixed_point_and_compiles_changed_behavior() {
    let all = generations();
    let programs = [
        (
            "struct P{s:[]const u8,t:[]const u8}static A:[2][2]P=[[P{s:\"a\",t:\"b\"};2];2];pub fn run()->u32{unsafe{if core::addr(A[0][0].s.data)!=core::addr(A[1][1].s.data){return 1;}if core::addr(A[0][0].t.data)!=core::addr(A[1][1].t.data){return 2;}return (A[1][0].s[0] as u32)+(A[0][1].t[0] as u32);}}",
            195,
        ),
        ("pub fn run()->u32{return 17;}", 17),
        (
            "static S:[2][]const u8=[\"x\";2];pub fn run()->u32{unsafe{if core::addr(S[0].data)==core::addr(S[1].data){return 1;}return 2;}}",
            1,
        ),
        (
            "pub fn run()->u32{var x:u32=0;var i:u32=0;while i<7_u32{x=x+i;i=i+1_u32;}if x==21_u32{return x+2_u32;}return 1;}",
            23,
        ),
        (
            "struct Pair{a:u32,b:u32} fn change(p:*Pair)->u32{unsafe{(*p).a=90;}return 0;} fn take(p:Pair,n:u32)->u32{return p.a+p.b+n;} pub fn run()->u32{var p:Pair=Pair{a:11,b:7};return take(p,change(core::addr_of_mut(p)));}",
            18,
        ),
        (
            "static mut n:u32=0;fn f()->u32{var x:u32=13;defer{unsafe{n=5;}x=90;}return x;}pub fn run()->u32{let x:u32=f();unsafe{return x+n;}}",
            18,
        ),
        (
            "fn bad()->Result<u32,u32>{return Result::<u32,u32>::Err(29);}fn go()->Result<u32,u32>{let n:u32=bad()?;return Result::<u32,u32>::Ok(n);}pub fn run()->u32{match go(){Result::<u32,u32>::Ok(n:u32)=>{return n;},Result::<u32,u32>::Err(e:u32)=>{return e;},}}",
            29,
        ),
        (
            "static s:[]const u8=\"A\";pub fn run()->u32{unsafe{return s[0] as u32;}}",
            65,
        ),
        (
            "static s:[]const u8=\"Z\";pub fn run()->u32{unsafe{return s[0] as u32;}}",
            90,
        ),
        (
            "pub fn run()->u32{let a:u8=255;let b:u8=a+1_u8;return b as u32;}",
            101,
        ),
        (
            "pub fn run()->u32{let a:u8=255;let b:u8=a+%2_u8;return b as u32;}",
            1,
        ),
    ];
    for (src, expected) in programs {
        let t = Scratch::new();
        fs::write(t.file("program.vt"), src).unwrap();
        manifest(&t, "case.build", "run", &["program.vt"], &[], "out.elf");
        for stage_id in 0..4 {
            if stage_id == 0 {
                success(
                    Command::new(BIN)
                        .args(["project", "case.build"])
                        .current_dir(&t.0)
                        .env("PATH", ""),
                );
            } else {
                executable(&t.file("compiler.elf"), &all.images[stage_id - 1]);
                let o = target(&t, "compiler.elf", "build", "case.build", false);
                assert!(
                    o.status.success(),
                    "C{stage_id}: {src}\n{}",
                    String::from_utf8_lossy(&o.stderr)
                );
            }
            let o = run(Command::new(qemu()).arg(t.file("out.elf")).env_clear());
            assert_eq!(
                o.status.code(),
                Some(expected),
                "C{stage_id}: {src}\n{}",
                String::from_utf8_lossy(&o.stderr)
            );
            fs::remove_file(t.file("out.elf")).unwrap();
        }
    }
}
#[test]
fn self_compiler_diagnostic_change_is_translated_and_check_creates_no_output() {
    let all = generations();
    let t = Scratch::new();
    stage(&t);
    executable(&t.file("compiler.elf"), &all.images[0]);
    let p = t.file("bootstrap/model.vt");
    let src = fs::read_to_string(&p).unwrap();
    assert!(src.contains("\"E_PARSE\""));
    fs::write(p, src.replace("\"E_PARSE\"", "\"MUTATED_PARSE\"")).unwrap();
    manifest(
        &t,
        "self.build",
        "bootstrap::main::run",
        SOURCES,
        &["runtime/linux.s0"],
        "mutated.elf",
    );
    let o = target(&t, "compiler.elf", "build", "self.build", false);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    fs::write(t.file("bad.vt"), "pub fn run( {").unwrap();
    manifest(&t, "bad.build", "run", &["bad.vt"], &[], "bad.elf");
    let o = target(&t, "mutated.elf", "build", "bad.build", false);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("MUTATED_PARSE"));
    assert!(!t.file("bad.elf").exists());
    fs::write(t.file("bad.vt"), "pub fn run()->u32{return 0;}").unwrap();
    let o = target(&t, "mutated.elf", "check", "bad.build", false);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(!t.file("bad.elf").exists());
}
#[test]
fn all_generations_resolve_modules_externs_and_reject_without_output() {
    let all = generations();
    for (source, entry) in [
        ("fn run()->u32{return 37;}", "root::run"),
        ("module app;fn run()->u32{return 37;}", "root::app::run"),
    ] {
        for stage_id in 0..4 {
            let t = Scratch::new();
            fs::write(t.file("main.vt"), source).unwrap();
            manifest(&t, "case.build", entry, &["main.vt"], &[], "out.elf");
            if stage_id == 0 {
                success(
                    Command::new(BIN)
                        .args(["project", "case.build"])
                        .current_dir(&t.0)
                        .env("PATH", ""),
                );
            } else {
                executable(&t.file("compiler.elf"), &all.images[stage_id - 1]);
                let o = target(&t, "compiler.elf", "check", "case.build", false);
                assert!(
                    o.status.success(),
                    "C{stage_id} check {entry}: {}",
                    String::from_utf8_lossy(&o.stderr)
                );
                assert!(!t.file("out.elf").exists());
                let o = target(&t, "compiler.elf", "build", "case.build", false);
                assert!(
                    o.status.success(),
                    "C{stage_id} build {entry}: {}",
                    String::from_utf8_lossy(&o.stderr)
                );
            }
            let o = run(Command::new(qemu()).arg(t.file("out.elf")).env_clear());
            assert_eq!(o.status.code(), Some(37), "C{stage_id}: {entry}");
        }
    }
    for image in &all.images {
        let t = Scratch::new();
        executable(&t.file("compiler.elf"), image);
        fs::write(t.file("lib.vt"),"module lib;pub struct Pair{pub x:u32,pub y:u32}pub fn pair()->Pair{return Pair{x:9,y:4};}").unwrap();
        fs::write(t.file("main.vt"),"module app;import lib;extern abi(C) fn increment(n:u32)->u32;static label:[]const u8=\"A\";pub fn run()->u32{let p:lib::Pair=lib::pair();unsafe{return increment(p.x+p.y)+(label[0] as u32);}}").unwrap();
        fs::write(
            t.file("provider.s0"),
            "increment:\naddi x10,x10,1\njalr x0,0(x1)\n",
        )
        .unwrap();
        manifest(
            &t,
            "case.build",
            "app::run",
            &["lib.vt", "main.vt"],
            &["provider.s0"],
            "out.elf",
        );
        let o = target(&t, "compiler.elf", "build", "case.build", false);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let o = run(Command::new(qemu()).arg(t.file("out.elf")).env_clear());
        assert_eq!(o.status.code(), Some(79));
        fs::remove_file(t.file("out.elf")).unwrap();
        for (source, code) in [
            ("pub fn run( {", "E_PARSE"),
            ("pub fn run()->u32{return missing;}", "E_NAME"),
            ("pub fn run()->u32{return true;}", "E_TYPE"),
            (
                "pub fn run()->u32{let x:u32=1;let p:*const u32=core::addr_of(x);return *p;}",
                "E_PERMISSION",
            ),
            ("pub fn run()->u32{let n:u64=0;return 0;}", "I_UNSUPPORTED"),
            (
                "extern abi(C) fn missing()->u32;pub fn run()->u32{unsafe{return missing();}}",
                "E_LINK",
            ),
        ] {
            fs::write(t.file("main.vt"), source).unwrap();
            manifest(&t, "case.build", "run", &["main.vt"], &[], "out.elf");
            let o = target(&t, "compiler.elf", "build", "case.build", false);
            assert_eq!(o.status.code(), Some(1), "{source}: {:?}", o.status);
            assert!(
                String::from_utf8_lossy(&o.stderr).contains(code),
                "{source}: {}",
                String::from_utf8_lossy(&o.stderr)
            );
            assert!(!t.file("out.elf").exists());
        }
        fs::write(t.file("main.vt"), "pub fn run()->u32{return 7;}").unwrap();
        manifest(&t, "case.build", "run", &["main.vt"], &[], "main.vt");
        let before = fs::read(t.file("main.vt")).unwrap();
        let o = target(&t, "compiler.elf", "build", "case.build", false);
        assert_eq!(o.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&o.stderr).contains("E_IO"));
        assert_eq!(fs::read(t.file("main.vt")).unwrap(), before);
    }
}

fn target_blocking(t: &Scratch, mode: &str) -> Output {
    run(Command::new(qemu())
        .args(["-s", "1048576"])
        .arg(t.file("compiler.elf"))
        .args([mode, "case.build", "--blocking", "policy.txt"])
        .current_dir(&t.0)
        .env_clear()
        .env("PATH", ""))
}
#[test]
fn blocking_contracts_propagate_without_changing_generated_elf() {
    let all = generations();
    // Both external declarations have real assembly implementations. No contract is inferred from that.
    let externs = "extern abi(C) fn poll()->u32;extern abi(C) fn wait()->u32;";
    let cases = [
        ("pub fn run()->u32{return 7;}", "", "Verified"),
        (
            "fn a()->u32{return b();}fn b()->u32{return a();}pub fn run()->u32{return a();}",
            "",
            "Verified",
        ),
        ("pub fn run()->u32{unsafe{return poll();}}", "", "Unknown"),
        (
            "pub fn run()->u32{unsafe{return poll();}}",
            "nonblocking app::poll\n",
            "Verified",
        ),
        (
            "pub fn run()->u32{unsafe{return poll();}}",
            "may_block app::poll\n",
            "Violation",
        ),
        (
            "pub fn run()->u32{unsafe{return poll()+wait();}}",
            "may_block app::wait\n",
            "Violation",
        ),
        (
            "pub fn run()->u32{unsafe{return wait()+poll();}}",
            "may_block app::wait\n",
            "Violation",
        ),
        (
            "fn a()->u32{return b();}fn b()->u32{unsafe{let x:u32=poll();}return a();}pub fn run()->u32{return a();}",
            "",
            "Unknown",
        ),
        (
            "fn a()->u32{return b();}fn b()->u32{unsafe{let x:u32=poll();}return a();}pub fn run()->u32{return a();}",
            "may_block app::poll\n",
            "Violation",
        ),
        (
            "pub fn run()->u32{return 7;}",
            "may_block app::poll\n",
            "Verified",
        ),
        (
            "pub fn run()->u32{if false{unsafe{return poll();}}return 7;}",
            "may_block app::poll\n",
            "Violation",
        ),
        (
            "pub fn run()->u32{defer{unsafe{let n:u32=poll();}}return 7;}",
            "may_block app::poll\n",
            "Violation",
        ),
        (
            "pub fn run()->u32{var n:u32=0;while n<2_u32{defer{unsafe{let x:u32=poll();}}n=n+1_u32;if n==1_u32{continue;}break;}return 7;}",
            "may_block app::poll\n",
            "Violation",
        ),
    ];
    for image in &all.images {
        let t = Scratch::new();
        executable(&t.file("compiler.elf"), image);
        fs::write(
            t.file("provider.s0"),
            "poll:\naddi x10,x0,7\njalr x0,0(x1)\nwait:\naddi x10,x0,9\njalr x0,0(x1)\n",
        )
        .unwrap();
        manifest(
            &t,
            "case.build",
            "app::run",
            &["main.vt"],
            &["provider.s0"],
            "out.elf",
        );
        for (source, contracts, expected) in cases {
            let source = format!("module app;{externs}{source}");
            let policy = format!("format 1\nrequire root::app::run\n{contracts}");
            fs::write(t.file("main.vt"), &source).unwrap();
            fs::write(t.file("policy.txt"), &policy).unwrap();
            let disabled = target(&t, "compiler.elf", "build", "case.build", false);
            assert!(
                disabled.status.success(),
                "{}",
                String::from_utf8_lossy(&disabled.stderr)
            );
            let elf = fs::read(t.file("out.elf")).unwrap();
            fs::remove_file(t.file("out.elf")).unwrap();
            for mode in ["check", "build"] {
                let output = target_blocking(&t, mode);
                let report = String::from_utf8_lossy(&output.stderr);
                assert!(
                    report.contains(&format!("Blocking app::run: {expected}")),
                    "{source}: {report}"
                );
                assert_eq!(
                    output.status.code(),
                    Some(if expected == "Verified" { 0 } else { 1 }),
                    "{report}"
                );
                if mode == "build" && expected == "Verified" {
                    assert_eq!(
                        fs::read(t.file("out.elf")).unwrap(),
                        elf,
                        "analysis changed ELF"
                    );
                    fs::remove_file(t.file("out.elf")).unwrap();
                } else {
                    assert!(!t.file("out.elf").exists());
                }
                assert_eq!(fs::read_to_string(t.file("main.vt")).unwrap(), source);
                assert_eq!(fs::read_to_string(t.file("policy.txt")).unwrap(), policy);
            }
        }
    }
}
#[test]
fn blocking_policy_is_strict_and_failure_preserves_existing_files() {
    let all = generations();
    let t = Scratch::new();
    executable(&t.file("compiler.elf"), &all.images[1]);
    let source = "module app;extern abi(C) fn poll()->u32;const n:u32=1;pub fn safe()->u32{return 7;}pub fn run()->u32{unsafe{return poll();}}";
    fs::write(t.file("main.vt"), source).unwrap();
    fs::write(
        t.file("alias.vt"),
        "module alias;extern abi(C) fn poll()->u32;",
    )
    .unwrap();
    fs::write(
        t.file("provider.s0"),
        "poll:\naddi x10,x0,7\njalr x0,0(x1)\n",
    )
    .unwrap();
    manifest(
        &t,
        "case.build",
        "app::run",
        &["main.vt", "alias.vt"],
        &["provider.s0"],
        "out.elf",
    );
    let invalid = [
        "",
        "format 2\nrequire app::run\n",
        "format 1\n",
        "require app::run\nformat 1\n",
        "format 1\nformat 1\nrequire app::run\n",
        "format 1\nrequire app::missing\n",
        "format 1\nrequire app::n\n",
        "format 1\nrequire app::poll\n",
        "format 1\nrequire app::run\nrequire root::app::run\n",
        "format 1\nrequire app::run\nnonblocking app::run\n",
        "format 1\nrequire app::run\nnonblocking app::missing\n",
        "format 1\nrequire app::run\nnonblocking app::poll\nnonblocking app::poll\n",
        "format 1\nrequire app::run\nnonblocking app::poll\nmay_block alias::poll\n",
        "format 1\nrequire app::run\nnonblocking app::poll\nnonblocking alias::poll\n",
        "format 1\nrequire app::run extra\n",
        "format 1\nrequire\n",
        "format 1\nrequire app::run\nmagic app::poll\n",
    ];
    for policy in invalid {
        fs::write(t.file("policy.txt"), policy).unwrap();
        let o = target_blocking(&t, "build");
        assert_eq!(o.status.code(), Some(1), "{policy}");
        assert!(String::from_utf8_lossy(&o.stderr).contains("E_BLOCKING_CONFIG"));
        assert!(!t.file("out.elf").exists());
    }
    fs::write(
        t.file("policy.txt"),
        format!("#{}\nformat 1\nrequire app::safe\n", "x".repeat(16384)),
    )
    .unwrap();
    let o = target_blocking(&t, "build");
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("E_BLOCKING_CONFIG"));
    fs::remove_file(t.file("policy.txt")).unwrap();
    assert_eq!(target_blocking(&t, "build").status.code(), Some(1));
    for requirements in [
        "require app::safe\nrequire app::run\n",
        "require app::run\nrequire app::safe\n",
    ] {
        fs::write(t.file("policy.txt"), format!("format 1\n{requirements}")).unwrap();
        let o = target_blocking(&t, "build");
        let report = String::from_utf8_lossy(&o.stderr);
        assert_eq!(o.status.code(), Some(1));
        assert!(report.contains("Blocking app::safe: Verified"));
        assert!(report.contains("Blocking app::run: Unknown"));
        assert!(!t.file("out.elf").exists());
    }
    // One linker-symbol contract covers its declarations in both modules.
    fs::write(
        t.file("policy.txt"),
        "# policy\r\nformat 1\r\nrequire root::app::run\r\nnonblocking alias::poll\r\n",
    )
    .unwrap();
    assert!(target_blocking(&t, "check").status.success());
    assert!(!t.file("out.elf").exists());
    for policy in [
        "format 1\nrequire app::run\n",
        "format 1\nrequire app::run\nnonblocking app::poll\n",
    ] {
        fs::write(t.file("policy.txt"), policy).unwrap();
        fs::write(t.file("out.elf"), b"existing output").unwrap();
        assert_eq!(target_blocking(&t, "build").status.code(), Some(1));
        assert_eq!(fs::read(t.file("out.elf")).unwrap(), b"existing output");
        fs::remove_file(t.file("out.elf")).unwrap();
        for output in ["main.vt", "policy.txt"] {
            manifest(
                &t,
                "case.build",
                "app::run",
                &["main.vt", "alias.vt"],
                &["provider.s0"],
                output,
            );
            assert_eq!(target_blocking(&t, "build").status.code(), Some(1));
            assert_eq!(fs::read_to_string(t.file("main.vt")).unwrap(), source);
            assert_eq!(fs::read_to_string(t.file("policy.txt")).unwrap(), policy);
        }
        manifest(
            &t,
            "case.build",
            "app::run",
            &["main.vt", "alias.vt"],
            &["provider.s0"],
            "out.elf",
        );
    }
}
#[test]
fn blocking_long_chains_are_order_independent_and_do_not_skip_core_errors() {
    let all = generations();
    let t = Scratch::new();
    executable(&t.file("compiler.elf"), &all.images[1]);
    fs::write(
        t.file("provider.s0"),
        "poll:\naddi x10,x0,7\njalr x0,0(x1)\n",
    )
    .unwrap();
    manifest(
        &t,
        "case.build",
        "app::run",
        &["main.vt"],
        &["provider.s0"],
        "out.elf",
    );
    let mut functions = Vec::new();
    for i in 0..300 {
        functions.push(format!("fn f{i}()->u32{{return f{}();}}", i + 1));
    }
    functions.push("fn f300()->u32{unsafe{return poll();}}".to_owned());
    fs::write(
        t.file("policy.txt"),
        "format 1\nrequire app::run\nmay_block app::poll\n",
    )
    .unwrap();
    for _ in 0..2 {
        let source = format!(
            "module app;extern abi(C) fn poll()->u32;pub fn run()->u32{{return f0();}}{}",
            functions.join("\n")
        );
        fs::write(t.file("main.vt"), source).unwrap();
        let o = target_blocking(&t, "check");
        assert_eq!(o.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&o.stderr).contains("Blocking app::run: Violation"));
        functions.reverse();
    }
    for (source, error) in [
        ("module app;pub fn run()->u32{return true;}", "E_TYPE"),
        (
            "module app;extern abi(C) fn missing()->u32;pub fn run()->u32{return 0;}",
            "E_LINK",
        ),
    ] {
        fs::write(t.file("main.vt"), source).unwrap();
        fs::write(t.file("policy.txt"), "format 1\nrequire app::run\n").unwrap();
        manifest(&t, "case.build", "app::run", &["main.vt"], &[], "out.elf");
        let o = target_blocking(&t, "build");
        assert_eq!(o.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&o.stderr).contains(error));
        assert!(!t.file("out.elf").exists());
    }
}
#[test]
fn blocking_missing_capture_invalid_ir_and_summary_capacity_fail_closed() {
    let t = Scratch::new();
    stage(&t);
    fs::write(t.file("driver.vt"),r#"
module probe;
import bootstrap::model;
import bootstrap::input;
import bootstrap::parse;
import bootstrap::blocking;
import runtime::linux;
import runtime::bytes;
pub fn run()->u32 {
    var mode:[]const u8="";
    match linux::argument(1) {
        Result::<[]const u8,u32>::Ok(value:[]const u8)=>{mode=value;},
        Result::<[]const u8,u32>::Err(error:u32)=>{return 99;},
    }
    model::reset_all();
    input::load("sample.build");
    parse::collect();
    if !blocking::load("sample.policy") {return 98;}
    if bytes::equal(mode,"missing") {
        if blocking::finish() {return 97;}
        return 0;
    }
    unsafe {
        var target:u32=3;
        if bytes::equal(mode,"bad") {target=4;}
        let call:model::Inst=model::Inst{op:model::IR_CALL,dst:0,a:target,b:0,c:0,aux:0,bits:0,span:model::Span{file:1,start:0,end:0}};
        model::inst_count=1;
        model::insts[1]=call;
        if bytes::equal(mode,"edges") {
            var i:u32=1;
            while i<=65536_u32 {model::insts[i as usize]=call;i=i+1_u32;}
            model::inst_count=65536;
        }
        blocking::capture(1);
        model::inst_count=1;
        blocking::capture(2);
    }
    return 96;
}
"#).unwrap();
    fs::write(t.file("sample.vt"),"pub fn run()->(){return ();}fn other()->(){return ();}fn sink()->(){return ();}const n:u32=7;").unwrap();
    fs::write(t.file("sample.policy"), "format 1\nrequire run\n").unwrap();
    manifest(
        &t,
        "sample.build",
        "run",
        &["sample.vt"],
        &[],
        "should-not-exist.elf",
    );
    let mut sources = SOURCES.to_vec();
    sources.push("driver.vt");
    manifest(
        &t,
        "driver.build",
        "probe::run",
        &sources,
        &["runtime/linux.s0"],
        "driver.elf",
    );
    success(
        Command::new(BIN)
            .args(["project", "driver.build"])
            .current_dir(&t.0)
            .env("PATH", ""),
    );
    for (mode, code, message) in [
        ("missing", 0, "Blocking run: Unknown"),
        ("bad", 1, "I_INTERNAL"),
        ("edges", 1, "I_LIMIT"),
    ] {
        let output = run(Command::new(qemu())
            .args(["-s", "1048576"])
            .arg(t.file("driver.elf"))
            .arg(mode)
            .current_dir(&t.0)
            .env_clear());
        let report = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(code), "{mode}: {report}");
        assert!(report.contains(message), "{mode}: {report}");
        if mode == "edges" {
            assert!(report.contains("actual=65537 limit=65536"), "{report}");
        }
        assert!(!t.file("should-not-exist.elf").exists());
    }
}
