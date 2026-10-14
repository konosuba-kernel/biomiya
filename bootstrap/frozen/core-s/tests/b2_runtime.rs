#![cfg(all(feature = "pipeline", feature = "image"))]
use biomiya_core_s::{
    backend, check, frontend, image,
    machine::{DataObject, IOp, Inst, Program},
    s0,
    source::SourceMap,
};
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
static SERIAL: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "core-s-b2-{}-{}",
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
fn run(cmd: &mut Command) -> Output {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("required QEMU/toolchain");
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() > end {
            child.kill().unwrap();
            panic!("timeout {cmd:?}: {:?}", child.wait_with_output().unwrap());
        }
        thread::sleep(Duration::from_millis(5));
    }
}
fn success(cmd: &mut Command) -> Output {
    let o = run(cmd);
    assert!(
        o.status.success(),
        "{cmd:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    o
}
fn write_executable(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}
fn program(extra: Option<&str>, entry: &str, provider: &str) -> Program {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut map = SourceMap::default();
    let mut programs = Vec::new();
    for name in [
        "runtime/bytes.vt",
        "runtime/linux.vt",
        "tests/fixtures/b2-input/source.vt",
        "tests/fixtures/b2-input/scan.vt",
        "tests/fixtures/b2-input/manifest.vt",
        "tests/fixtures/b2-input/copy.vt",
    ] {
        let path = base.join(name);
        let text = fs::read_to_string(&path).unwrap();
        let id = map.add(path, text);
        programs.push(frontend::parse(id, &map.get(id).text).unwrap_or_else(|e| {
            panic!(
                "{}:{:?}: {e}",
                map.get(e.span.file).path.display(),
                map.line_column(e.span)
            )
        }));
    }
    if let Some(extra) = extra {
        let id = map.add("test.vt", extra.into());
        programs.push(frontend::parse(id, extra).unwrap());
    }
    let resolved = frontend::resolve_files(programs).unwrap_or_else(|e| {
        panic!(
            "{}:{:?}: {e}",
            map.get(e.span.file).path.display(),
            map.line_column(e.span)
        )
    });
    let compiled = check::check(&resolved).unwrap_or_else(|e| {
        panic!(
            "{}:{:?}: {e}",
            map.get(e.span.file).path.display(),
            map.line_column(e.span)
        )
    });
    let entry = compiled.functions.iter().find(|f| f.name == entry).unwrap();
    let mut p = Program {
        instructions: vec![
            Inst::Label("_start".into()),
            Inst::I {
                op: IOp::Addi,
                rd: 27,
                rs1: 2,
                imm: 0,
            },
            Inst::Jump {
                rd: 1,
                target: entry.symbol.clone(),
            },
            Inst::Li { rd: 17, bits: 93 },
            Inst::Ecall,
            Inst::Unimp,
            Inst::Label("__core_s_panic".into()),
            Inst::Li { rd: 10, bits: 101 },
            Inst::Li { rd: 17, bits: 93 },
            Inst::Ecall,
            Inst::Unimp,
        ],
        data: vec![],
    };
    let body = backend::machine(&compiled).unwrap();
    p.instructions.extend(body.instructions);
    p.data.extend(body.data);
    let id = map.add("linux.s0", provider.into());
    let asm = s0::parse(id, provider).unwrap();
    for symbol in compiled.externs.values() {
        assert!(
            asm.instructions
                .iter()
                .any(|i| matches!(i,Inst::Label(name) if name==symbol)),
            "missing external provider {symbol}"
        );
    }
    p.instructions.extend(asm.instructions);
    p
}
fn provider() -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime/linux.s0")).unwrap()
}
fn copy_image() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        image::executable(&program(None, "app::start", &provider()), "_start").unwrap()
    })
}
fn manifest(sources: &[&str]) -> String {
    let mut m = String::from("format 1\nprofile linux-user\nentry app::start\n");
    for name in sources {
        m.push_str(&format!("source {name}\n"));
    }
    m.push_str("output copied.vt\n");
    m
}
fn prepare(t: &Scratch, m: &str, files: &[(&str, &[u8])]) {
    fs::write(t.file("project"), m).unwrap();
    for (name, bytes) in files {
        fs::write(t.file(name), bytes).unwrap();
    }
}
fn execute(t: &Scratch, bytes: &[u8], args: &[&str]) -> Output {
    write_executable(&t.file("copy.elf"), bytes);
    run(Command::new("qemu-riscv64")
        .arg(t.file("copy.elf"))
        .args(args)
        .current_dir(&t.0))
}
#[test]
fn real_program_reads_scans_and_copies_multiple_files_and_empty_input() {
    let t = Scratch::new();
    let long = "/* nested /* comment */ ok */\n".repeat(400);
    prepare(
        &t,
        &manifest(&["empty.vt", "one.vt", "two.vt"]),
        &[
            ("empty.vt", b""),
            ("one.vt", long.as_bytes()),
            ("two.vt", b"fn f() -> u32 { return 42; }\n"),
        ],
    );
    let o = execute(&t, copy_image(), &["project"]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(
        fs::read(t.file("copied.vt")).unwrap(),
        [long.as_bytes(), b"fn f() -> u32 { return 42; }\n"].concat()
    );
    let o = execute(&t, copy_image(), &["project"]);
    assert_eq!(o.status.code(), Some(12));
    assert_eq!(
        fs::read(t.file("copied.vt")).unwrap(),
        [long.as_bytes(), b"fn f() -> u32 { return 42; }\n"].concat()
    );
    let empty = Scratch::new();
    prepare(&empty, &manifest(&["empty.vt"]), &[("empty.vt", b"")]);
    assert_eq!(
        execute(&empty, copy_image(), &["project"]).status.code(),
        Some(0)
    );
    assert!(fs::read(empty.file("copied.vt")).unwrap().is_empty());
}
#[test]
fn direct_and_external_toolchain_execute_the_same_target_program() {
    let p = program(None, "app::start", &provider());
    let t = Scratch::new();
    fs::write(t.file("copy.s"), backend::assembly(&p).unwrap()).unwrap();
    success(
        Command::new("clang")
            .args([
                "--target=riscv64-unknown-elf",
                "-march=rv64im",
                "-mabi=lp64",
                "-mno-relax",
                "-c",
            ])
            .arg(t.file("copy.s"))
            .arg("-o")
            .arg(t.file("copy.o")),
    );
    success(
        Command::new("ld.lld")
            .args(["--no-relax", "-e", "_start", "-Ttext=0x10000"])
            .arg(t.file("copy.o"))
            .arg("-o")
            .arg(t.file("reference.elf")),
    );
    prepare(
        &t,
        &manifest(&["input.vt"]),
        &[("input.vt", b"static text:[]const u8 = \"reference\\n\";\n")],
    );
    assert_eq!(
        execute(&t, copy_image(), &["project"]).status.code(),
        Some(0)
    );
    let direct = fs::read(t.file("copied.vt")).unwrap();
    fs::remove_file(t.file("copied.vt")).unwrap();
    let result = run(Command::new("qemu-riscv64")
        .arg(t.file("reference.elf"))
        .arg("project")
        .current_dir(&t.0));
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(fs::read(t.file("copied.vt")).unwrap(), direct);
}
fn injected(kind: &str) -> Vec<u8> {
    let mut shim = String::from("__b2_syscall6:\n");
    let mut data = vec![];
    if kind == "short" {
        shim.push_str("addi x5, x0, 63\nbeq x10, x5, .Lread\naddi x5, x0, 64\nbeq x10, x5, .Lwrite\njump __b2_real\n.Lread:\naddress x5, __test_read\njump .Linterrupt\n.Lwrite:\naddress x5, __test_write\n.Linterrupt:\nlbu x6, 0(x5)\nbne x6, x0, .Lshort\naddi x6, x0, 1\nsb x6, 0(x5)\naddi x10, x0, -4\njalr x0, 0(x1)\n.Lshort:\nsltiu x5, x13, 8\nbne x5, x0, .Lreal\naddi x13, x0, 7\n.Lreal:\njump __b2_real\n");
        for name in ["__test_read", "__test_write"] {
            data.push(DataObject {
                symbol: name.into(),
                mutable: true,
                align: 1,
                bytes: vec![0],
                zero_fill: 0,
                relocations: vec![],
            });
        }
    } else if kind == "close_error" {
        shim.push_str("addi x5, x0, 57\nbne x10, x5, .Lreal\naddi x5, x0, 4\nbne x11, x5, .Lreal\naddi x17, x0, 57\naddi x10, x11, 0\necall\naddi x10, x0, -5\njalr x0, 0(x1)\n.Lreal:\njump __b2_real\n");
    } else if kind == "read_error" {
        shim.push_str("addi x5, x0, 63\nbne x10, x5, .Lreal\naddi x5, x0, 4\nbne x11, x5, .Lreal\naddi x10, x0, -5\njalr x0, 0(x1)\n.Lreal:\njump __b2_real\n");
    } else {
        shim.push_str("addi x5, x0, 64\nbne x10, x5, .Lreal\n");
        shim.push_str(if kind == "zero_write" {
            "addi x10, x0, 0\n"
        } else {
            "addi x10, x0, -5\n"
        });
        shim.push_str("jalr x0, 0(x1)\n.Lreal:\njump __b2_real\n");
    }
    shim.push_str(&provider().replace("__b2_syscall6:", "__b2_real:"));
    let mut p = program(None, "app::start", &shim);
    p.data.extend(data);
    image::executable(&p, "_start").unwrap()
}
#[test]
fn short_io_and_eintr_are_retried_in_target_code() {
    let t = Scratch::new();
    let text = "fn test() -> u32 { return 42; }\n".repeat(250);
    prepare(
        &t,
        &manifest(&["input.vt"]),
        &[("input.vt", text.as_bytes())],
    );
    assert_eq!(
        execute(&t, &injected("short"), &["project"]).status.code(),
        Some(0)
    );
    assert_eq!(fs::read(t.file("copied.vt")).unwrap(), text.as_bytes());
}
#[test]
fn io_failures_close_files_and_remove_only_the_new_output() {
    for (kind, code) in [
        ("read_error", 15),
        ("zero_write", 16),
        ("write_error", 16),
        ("close_error", 13),
    ] {
        let t = Scratch::new();
        prepare(
            &t,
            &manifest(&["input.vt"]),
            &[("input.vt", b"fn f() -> u32 { return 0; }")],
        );
        write_executable(&t.file("copy.elf"), &injected(kind));
        let out = run(Command::new("qemu-riscv64")
            .arg("-strace")
            .arg(t.file("copy.elf"))
            .arg("project")
            .current_dir(&t.0));
        assert_eq!(out.status.code(), Some(code), "{kind}");
        assert!(!t.file("copied.vt").exists());
        let trace = String::from_utf8_lossy(&out.stderr);
        assert!(trace.contains("close(4) = 0"), "{trace}");
        assert!(trace.matches("close(3) = 0").count() >= 2, "{trace}");
        assert!(trace.contains("unlinkat("), "{trace}");
        assert_eq!(
            fs::read(t.file("input.vt")).unwrap(),
            b"fn f() -> u32 { return 0; }"
        );
    }
}
#[test]
fn malformed_manifest_missing_source_scanner_error_and_limits_fail_cleanly() {
    for (m, data, code) in [
        (manifest(&["missing.vt"]), Vec::new(), 12),
        (manifest(&["input.vt"]), b"/* unterminated".to_vec(), 21),
        (manifest(&["input.vt"]), "变量".as_bytes().to_vec(), 21),
        (manifest(&["input.vt"]), vec![32; 65537], 17),
        (manifest(&["../input.vt"]), Vec::new(), 20),
        (manifest(&["input.vt", "input.vt"]), Vec::new(), 20),
        (manifest(&["input.vt"]) + "entry other\n", Vec::new(), 20),
        (
            manifest(&["input.vt"]).replace("linux-user", "sos"),
            Vec::new(),
            20,
        ),
        (manifest(&["input.vt"]) + "unknown value\n", Vec::new(), 20),
        ("#".to_owned() + &"x".repeat(16384), Vec::new(), 17),
    ] {
        let t = Scratch::new();
        prepare(&t, &m, &[("input.vt", &data)]);
        assert_eq!(
            execute(&t, copy_image(), &["project"]).status.code(),
            Some(code),
            "{m}"
        );
        assert!(!t.file("copied.vt").exists());
    }
    let t = Scratch::new();
    assert_eq!(execute(&t, copy_image(), &[]).status.code(), Some(10));
    prepare(
        &t,
        &manifest(&["input.vt"]),
        &[("input.vt", &vec![32; 65536])],
    );
    assert_eq!(
        execute(&t, copy_image(), &["project"]).status.code(),
        Some(0)
    );
    assert_eq!(fs::metadata(t.file("copied.vt")).unwrap().len(), 65536);
}
#[test]
fn scanner_reports_token_ranges_and_error_offsets() {
    let source = r#"module probe;
import compiler::source;
import compiler::scan;
pub fn run() -> u32 {
 var s:source::Source = source::begin("  ab 12 \"x\" /* a /* b */ c */ +% // end\n");
 unsafe {
  match scan::next(core::addr_of_mut(s)) {Result::<source::Token,source::Error>::Ok(t:source::Token)=>{if t.kind != 1_u32 || t.start != 2_usize || t.end != 4_usize {return 1;}},Result::<source::Token,source::Error>::Err(e:source::Error)=>{return 2;},}
  match scan::next(core::addr_of_mut(s)) {Result::<source::Token,source::Error>::Ok(t:source::Token)=>{if t.kind != 2_u32 || t.start != 5_usize || t.end != 7_usize {return 3;}},Result::<source::Token,source::Error>::Err(e:source::Error)=>{return 4;},}
  match scan::next(core::addr_of_mut(s)) {Result::<source::Token,source::Error>::Ok(t:source::Token)=>{if t.kind != 3_u32 || t.start != 8_usize || t.end != 11_usize {return 5;}},Result::<source::Token,source::Error>::Err(e:source::Error)=>{return 6;},}
  match scan::next(core::addr_of_mut(s)) {Result::<source::Token,source::Error>::Ok(t:source::Token)=>{if t.kind != 4_u32 || t.end-t.start != 2_usize {return 7;}},Result::<source::Token,source::Error>::Err(e:source::Error)=>{return 8;},}
  match scan::next(core::addr_of_mut(s)) {Result::<source::Token,source::Error>::Ok(t:source::Token)=>{if t.kind != 0_u32 || t.end != s.bytes.len {return 9;}},Result::<source::Token,source::Error>::Err(e:source::Error)=>{return 10;},}
  s = source::begin("  /* bad");
  match scan::next(core::addr_of_mut(s)) {Result::<source::Token,source::Error>::Err(e:source::Error)=>{if e.code != 1_u32 || e.offset != 2_usize {return 11;}},Result::<source::Token,source::Error>::Ok(t:source::Token)=>{return 12;},}
  s = source::begin(" \"\\q\"");
  match scan::next(core::addr_of_mut(s)) {Result::<source::Token,source::Error>::Err(e:source::Error)=>{if e.code != 4_u32 || e.offset != 3_usize {return 13;}},Result::<source::Token,source::Error>::Ok(t:source::Token)=>{return 14;},}
 }
 return 0;
}"#;
    let p = program(Some(source), "probe::run", &provider());
    let t = Scratch::new();
    assert_eq!(
        execute(&t, &image::executable(&p, "_start").unwrap(), &[])
            .status
            .code(),
        Some(0)
    );
}
