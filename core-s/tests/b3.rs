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
    "bootstrap/lex.vt",
    "bootstrap/input.vt",
    "bootstrap/types.vt",
    "bootstrap/parse.vt",
    "bootstrap/ir.vt",
    "bootstrap/check.vt",
    "bootstrap/image.vt",
    "bootstrap/rv64.vt",
    "bootstrap/s0.vt",
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
