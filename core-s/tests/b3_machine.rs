#![cfg(all(feature = "pipeline", feature = "image"))]
//! A target-language emitter builds new ELF files; host tools only check the result.
use biomiya_core_s::{backend, encode, image, machine::*, s0, source::FileId};
use std::os::unix::fs::PermissionsExt;
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
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "b3-machine-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn run(cmd: &mut Command) -> Output {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("required executable");
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() > until {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("timeout {cmd:?}");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
fn good(cmd: &mut Command) -> Output {
    let output = run(cmd);
    assert!(
        output.status.success(),
        "{cmd:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn driver() -> &'static Path {
    static DRIVER: OnceLock<Dir> = OnceLock::new();
    &DRIVER.get_or_init(||{
        let dir=Dir::new();let root=Path::new(env!("CARGO_MANIFEST_DIR"));
        let manifest="format 1\nprofile linux-user\nentry machine_test::run\nsource bootstrap/model.vt\nsource bootstrap/rules.vt\nsource bootstrap/image.vt\nsource bootstrap/rv64.vt\nsource bootstrap/s0.vt\nsource bootstrap/tests/machine.vt\nsource runtime/linux.vt\nassembly runtime/linux.s0\noutput driver.elf\n";
        // All declared inputs are copied into a clean project; no legacy compiler files.
        for path in ["bootstrap/model.vt","bootstrap/rules.vt","bootstrap/image.vt","bootstrap/rv64.vt","bootstrap/s0.vt","bootstrap/tests/machine.vt","runtime/linux.vt","runtime/linux.s0"] {let dest=dir.0.join(path);fs::create_dir_all(dest.parent().unwrap()).unwrap();fs::copy(root.join(path),dest).unwrap();}
        fs::write(dir.0.join("driver.build"),manifest).unwrap();
        good(Command::new(env!("CARGO_BIN_EXE_core-s")).arg("project").arg(dir.0.join("driver.build")).arg("--root").arg(&dir.0));dir
    }).0
}
fn generate(mode: &str, source: Option<&str>) -> (Dir, Output) {
    let dir = Dir::new();
    if let Some(source) = source {
        fs::write(dir.0.join("input.s0"), source).unwrap();
    }
    let output = run(Command::new("/usr/bin/qemu-riscv64")
        .args(["-s", "1048576"])
        .arg(driver().join("driver.elf"))
        .arg(mode)
        .current_dir(&dir.0)
        .env("PATH", ""));
    (dir, output)
}
fn u64_at(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(data[at..at + 8].try_into().unwrap())
}
fn target(dir: &Dir) -> Output {
    let path = dir.0.join("out.elf");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    run(Command::new("/usr/bin/qemu-riscv64")
        .args(["-s", "1048576"])
        .arg(path)
        .env("PATH", ""))
}
#[test]
fn target_ir_aggregate_abi_snapshot_abs64_bss_and_checked_panic() {
    let (dir, out) = generate("ir", None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(target(&dir).status.code(), Some(97));
    let data = fs::read(dir.0.join("out.elf")).unwrap();
    assert_eq!(u64_at(&data, 64 + 2 * 56 + 32), 8);
    assert_eq!(u64_at(&data, 64 + 2 * 56 + 40), 12288);
    for mode in ["overflow", "divzero", "shift"] {
        let (dir, out) = generate(mode, None);
        assert!(out.status.success(), "{mode}");
        assert_eq!(target(&dir).status.code(), Some(101));
    }
    let (dir, out) = generate("wrap", None);
    assert!(out.status.success());
    assert_eq!(target(&dir).status.code(), Some(0));
}
fn reference(program: &Program, expected: &[u8]) {
    let dir = Dir::new();
    fs::write(
        dir.0.join("reference.s"),
        backend::assembly(program).unwrap(),
    )
    .unwrap();
    good(
        Command::new("clang")
            .args([
                "--target=riscv64-unknown-elf",
                "-march=rv64im_zicsr",
                "-mabi=lp64",
                "-mno-relax",
                "-c",
            ])
            .arg(dir.0.join("reference.s"))
            .arg("-o")
            .arg(dir.0.join("reference.o")),
    );
    good(
        Command::new("ld.lld")
            .args(["--no-relax", "--oformat=binary", "-Ttext=0x10000"])
            .arg(dir.0.join("reference.o"))
            .arg("-o")
            .arg(dir.0.join("reference.bin")),
    );
    assert_eq!(expected, fs::read(dir.0.join("reference.bin")).unwrap());
}
#[test]
fn target_constants_match_rust_and_independent_assembler() {
    let numbers = [
        0,
        2047,
        2048,
        4095,
        32767,
        32768,
        268435455,
        2147483648,
        4294967295,
        9223372036854775807,
        9223372036854775808,
        18446744073709549568,
        18446744073709551615,
    ];
    let mut instructions = vec![
        Inst::Label("entry".into()),
        Inst::I {
            op: IOp::Addi,
            rd: 2,
            rs1: 2,
            imm: -112,
        },
    ];
    for (index, bits) in numbers.iter().copied().enumerate() {
        instructions.push(Inst::Li { rd: 5, bits });
        instructions.push(Inst::Store {
            op: StoreOp::Double,
            rs: 5,
            base: 2,
            offset: (index * 8) as i64,
        });
    }
    instructions.extend([
        Inst::Li { rd: 10, bits: 1 },
        Inst::I {
            op: IOp::Addi,
            rd: 11,
            rs1: 2,
            imm: 0,
        },
        Inst::Li { rd: 12, bits: 104 },
        Inst::Li { rd: 17, bits: 64 },
        Inst::Ecall,
        Inst::Li { rd: 10, bits: 0 },
        Inst::Li { rd: 17, bits: 93 },
        Inst::Ecall,
    ]);
    let program = Program {
        instructions,
        data: vec![],
    };
    let (dir, out) = generate("constants", None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read(dir.0.join("out.elf")).unwrap(),
        image::executable(&program, "entry").unwrap()
    );
    let executed = target(&dir);
    assert!(executed.status.success());
    assert_eq!(
        executed.stdout,
        numbers
            .into_iter()
            .flat_map(u64::to_le_bytes)
            .collect::<Vec<_>>()
    );
    // LLVM is allowed a different li expansion; compare its execution, not instruction selection.
    fs::write(
        dir.0.join("reference.s"),
        backend::assembly(&program).unwrap(),
    )
    .unwrap();
    good(
        Command::new("clang")
            .args([
                "--target=riscv64-unknown-elf",
                "-march=rv64im_zicsr",
                "-mabi=lp64",
                "-mno-relax",
                "-c",
            ])
            .arg(dir.0.join("reference.s"))
            .arg("-o")
            .arg(dir.0.join("reference.o")),
    );
    good(
        Command::new("ld.lld")
            .args(["--no-relax", "-Ttext=0x10000", "-e", "entry"])
            .arg(dir.0.join("reference.o"))
            .arg("-o")
            .arg(dir.0.join("reference.elf")),
    );
    assert_eq!(
        good(Command::new("/usr/bin/qemu-riscv64").arg(dir.0.join("reference.elf"))).stdout,
        executed.stdout
    );
}
#[test]
fn target_s0_all_operations_far_fixups_match_independent_bytes() {
    let mut source = String::from("entry:\njump far\n.Llocal:\n");
    for op in [
        "add", "sub", "mul", "mulhu", "divu", "remu", "and", "or", "xor", "sltu", "sll", "srl",
    ] {
        source.push_str(&format!("{op} x5,x6,x7\n"));
    }
    for op in ["addi", "addiw", "xori", "sltiu"] {
        for n in [-2048, -1, 0, 2047] {
            source.push_str(&format!("{op} x8,x9,{n}\n"));
        }
    }
    for op in ["slli", "srli"] {
        source.push_str(&format!("{op} x10,x11,63\n"));
    }
    for op in ["lbu", "lwu", "ld", "sb", "sw", "sd", "jalr"] {
        source.push_str(&format!("{op} x12,-2048(x13)\n"));
    }
    for op in ["beq", "bne", "bltu", "bgeu"] {
        source.push_str(&format!("{op} x5,x6,far\n"));
    }
    source.push_str("address x7,far\ncall far\njump .Llocal\nunimp\n");
    for _ in 0..1700 {
        source.push_str("addi x0,x0,0\n");
    }
    source.push_str("far:\naddi x10,x0,0\naddi x17,x0,93\necall\n");
    let (dir, out) = generate("asm", Some(&source));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let program = s0::parse(FileId(7), &source).unwrap();
    assert_eq!(
        fs::read(dir.0.join("out.elf")).unwrap(),
        image::executable(&program, "entry").unwrap()
    );
    reference(&program, &encode::encode(&program, 0x10000).unwrap().bytes);
    assert!(target(&dir).status.success());
}
#[test]
fn target_rejects_invalid_ir_and_link_data_before_output() {
    for mode in [
        "badir",
        "badpointer",
        "badoffsetconst",
        "badoffsetmut",
        "badcall",
        "badreturn",
        "badlabel",
        "duplicatelabel",
        "badtype",
        "frame_limit",
        "duplicate",
        "absout",
        "absunder",
        "overlap",
        "jumpdata",
        "undefined",
    ] {
        let (dir, out) = generate(mode, None);
        assert!(!out.status.success(), "{mode}");
        let text = String::from_utf8_lossy(&out.stderr);
        assert!(
            text.contains(if mode == "frame_limit" {
                "I_LIMIT"
            } else if mode.starts_with("bad") || mode == "duplicatelabel" {
                "I_INTERNAL"
            } else {
                "E_LINK"
            }),
            "{mode}: {text}"
        );
        if mode == "badir" {
            assert!(text.contains("file=2 byte=17"));
        }
        if mode == "frame_limit" {
            assert!(text.contains("actual="));
            assert!(text.contains("limit=262144"));
        }
        assert!(!dir.0.join("out.elf").exists());
    }
}
#[test]
fn target_requires_label_one_and_finite_value_layouts() {
    for mode in ["badnolabel", "badlabeltwo", "badcycle", "badmutualcycle"] {
        let (dir, out) = generate(mode, None);
        assert_eq!(out.status.code(), Some(1), "{mode}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("I_INTERNAL"),
            "{mode}"
        );
        assert!(!dir.0.join("out.elf").exists(), "{mode}");
    }
    for mode in ["selfpointer", "selfslice"] {
        let (dir, out) = generate(mode, None);
        assert!(
            out.status.success(),
            "{mode}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(target(&dir).status.code(), Some(0), "{mode}");
    }
}

#[test]
fn target_preserves_semantic_ir_and_reuses_sparse_label_ranges() {
    // emit_function snapshots every active Slot/Inst/Arg/parameter-slot field
    // and all function IR counts around each backend invocation.
    let (dir, out) = generate("labels_reuse", None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(target(&dir).status.code(), Some(42));
    for mode in [
        "labels_zero",
        "labels_over",
        "labels_missing",
        "labels_yes_missing",
        "labels_no_missing",
        "labels_duplicate",
        "labels_unterminated",
        "labels_afterterm",
        "labels_fallthrough",
    ] {
        let (dir, out) = generate(mode, None);
        assert_eq!(out.status.code(), Some(1), "{mode}");
        let text = String::from_utf8_lossy(&out.stderr);
        assert!(
            text.contains("I_INTERNAL") && text.contains("file=2 byte=17"),
            "{mode}: {text}"
        );
        assert!(!dir.0.join("out.elf").exists(), "{mode}");
    }
}

#[test]
fn target_s0_rejects_syntax_ranges_and_unsupported_grammar() {
    for line in [
        "add x32,x1,x2",
        "addi x1,x2,2048",
        "addi x1,x2,-2049",
        "slli x1,x2,64",
        "srli x1,x2,-1",
        "ld x1,(x2)",
        "ld x1,0xg(x2)",
        "call foo+8",
        "address x0,entry",
        "ecall x0",
        "addi x1,x2,0xAB",
        "entry: ecall",
        "entry:\nentry:",
    ] {
        let (dir, out) = generate("asm", Some(&format!("# ignored\n{line}\n")));
        assert!(!out.status.success(), "{line}");
        let text = String::from_utf8_lossy(&out.stderr);
        assert!(
            text.contains(if line == "entry:\nentry:" {
                "file=7 byte=17"
            } else {
                "file=7 byte=10"
            }),
            "{line}: {text}"
        );
        assert!(!dir.0.join("out.elf").exists());
    }
    for line in ["li x1,0", "la x1,entry", ".word 1", "csrrw x1,0,x2"] {
        let (_, out) = generate("asm", Some(line));
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("I_UNSUPPORTED"),
            "{line}"
        );
    }
}
