#![cfg(all(feature = "pipeline", feature = "image"))]
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

const BIN: &str = env!("CARGO_BIN_EXE_core-s");
const SOURCE: &str = include_str!("../fixtures/bootstrap.vt");
static SERIAL: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "core-s-cli-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
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
        .expect("required tool must be installed");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            let out = child.wait_with_output().unwrap();
            panic!(
                "command timed out: {command:?}, stderr={}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        thread::sleep(Duration::from_millis(5));
    }
}
fn success(command: &mut Command) -> Output {
    let result = run(command);
    assert!(
        result.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    result
}
fn build(input: &Path, entry: &str, output: &Path, kind: &str) {
    success(
        Command::new(BIN)
            .arg(kind)
            .arg(input)
            .args(["--entry", entry, "-o"])
            .arg(output),
    );
}

#[test]
fn real_source_direct_and_reference_paths_agree() {
    let temp = Scratch::new();
    let source = temp.file("program.vt");
    fs::write(&source, SOURCE).unwrap();
    success(Command::new(BIN).arg("check").arg(&source));
    let cases = [
        ("arithmetic", 42),
        ("stack_arguments", 55),
        ("aggregate_snapshot", 42),
        ("aggregate_result", 42),
        ("return_cleanup", 43),
        ("cleanup_order", 21),
        ("loop_cleanup", 3),
        ("result_cleanup", 24),
        ("recursion", 8),
        ("layout_and_array", 11),
        ("wide_constant", 42),
        ("narrow_abi", 42),
        ("wrapping", 1),
        ("short_circuit", 42),
        ("zero_array", 0),
        ("overflow", 101),
        ("divide_zero", 101),
        ("bounds", 101),
        ("shift_bounds", 101),
    ];
    for (entry, expected) in cases {
        let direct = temp.file("direct.elf");
        let asm = temp.file("program.s");
        let object = temp.file("program.o");
        let reference = temp.file("reference.elf");
        build(&source, entry, &direct, "build");
        let actual = run(Command::new("qemu-riscv64").arg(&direct));
        assert_eq!(
            actual.status.code(),
            Some(expected),
            "direct {entry}: {}",
            String::from_utf8_lossy(&actual.stderr)
        );
        build(&source, entry, &asm, "asm");
        success(
            Command::new("clang")
                .args([
                    "--target=riscv64-unknown-elf",
                    "-march=rv64im",
                    "-mabi=lp64",
                    "-mno-relax",
                    "-c",
                ])
                .arg(&asm)
                .arg("-o")
                .arg(&object),
        );
        success(
            Command::new("ld.lld")
                .args(["--no-relax", "-e", "_start", "-Ttext=0x10000"])
                .arg(&object)
                .arg("-o")
                .arg(&reference),
        );
        let other = run(Command::new("qemu-riscv64").arg(&reference));
        assert_eq!(
            other.status.code(),
            Some(expected),
            "reference {entry}: {}",
            String::from_utf8_lossy(&other.stderr)
        );
    }
}

#[test]
fn direct_build_needs_no_external_tools_and_is_deterministic() {
    let temp = Scratch::new();
    let input = temp.file("source.vt");
    fs::write(&input, "fn run() -> u32 { return 42; }").unwrap();
    let one = temp.file("one.elf");
    let two = temp.file("two.elf");
    for out in [&one, &two] {
        success(
            Command::new(BIN)
                .env("PATH", "")
                .arg("build")
                .arg(&input)
                .args(["--entry", "run", "-o"])
                .arg(out),
        );
    }
    assert_eq!(fs::read(&one).unwrap(), fs::read(&two).unwrap());
    assert_eq!(
        run(Command::new("qemu-riscv64").arg(one)).status.code(),
        Some(42)
    );
}

#[test]
fn rejected_inputs_never_replace_existing_output() {
    let temp = Scratch::new();
    let input = temp.file("source.vt");
    let output = temp.file("existing.elf");
    fs::write(&output, b"keep old artifact").unwrap();
    for (source, code) in [
        ("fn run() -> u32 { return ; }", "E_PARSE"),
        ("fn run() -> u32 { return unknown; }", "E_NAME"),
        ("fn run() -> u32 { return false; }", "E_TYPE"),
        ("enum E { A }", "I_UNSUPPORTED"),
        ("fn other() -> u32 { return 0; }", "E_ENTRY"),
        ("fn run(x:u32) -> u32 { return x; }", "E_ENTRY"),
        ("unsafe fn run() -> u32 { return 0; }", "E_ENTRY"),
        ("fn run() -> bool { return false; }", "E_ENTRY"),
    ] {
        fs::write(&input, source).unwrap();
        let failure = run(Command::new(BIN)
            .arg("build")
            .arg(&input)
            .args(["--entry", "run", "-o"])
            .arg(&output));
        assert!(!failure.status.success());
        assert!(
            String::from_utf8_lossy(&failure.stderr).contains(code),
            "{}",
            String::from_utf8_lossy(&failure.stderr)
        );
        assert_eq!(fs::read(&output).unwrap(), b"keep old artifact");
    }
    fs::write(&input, b"\xff").unwrap();
    let failure = run(Command::new(BIN).arg("check").arg(&input));
    assert!(String::from_utf8_lossy(&failure.stderr).contains("E_UTF8"));
    fs::write(&input, "fn run() -> u32 {\n return unknown;\n}").unwrap();
    let failure = run(Command::new(BIN).arg("check").arg(&input));
    assert!(String::from_utf8_lossy(&failure.stderr).contains("source.vt:2:"));
}

#[test]
fn output_failures_and_source_aliases_are_safe() {
    let temp = Scratch::new();
    let input = temp.file("source.vt");
    let source = "fn run() -> u32 { return 42; }";
    fs::write(&input, source).unwrap();
    let directory = temp.file("directory");
    fs::create_dir(&directory).unwrap();
    let mut outputs = vec![input.clone(), directory, temp.file("missing/out.elf")];
    #[cfg(unix)]
    {
        let hard = temp.file("hard.vt");
        fs::hard_link(&input, &hard).unwrap();
        let symbolic = temp.file("symbolic.vt");
        std::os::unix::fs::symlink(&input, &symbolic).unwrap();
        outputs.extend([hard, symbolic]);
    }
    for output in outputs {
        let failure = run(Command::new(BIN)
            .arg("build")
            .arg(&input)
            .args(["--entry", "run", "-o"])
            .arg(output));
        assert!(!failure.status.success());
        assert_eq!(fs::read_to_string(&input).unwrap(), source);
        assert!(String::from_utf8_lossy(&failure.stderr).contains("I_IO"));
    }
    assert!(fs::read_dir(&temp.0).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".core-s-")));
}
