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
static SERIAL: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "core-s-project-{}-{}",
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
        .expect("required test tool");
    let deadline = Instant::now() + Duration::from_secs(20);
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
fn manifest(output: &str) -> String {
    format!(
        "format 1\nprofile linux-user\nentry app::start\nsource app.vt\nsource helper.vt\nassembly linux.s0\noutput {output}\n"
    )
}
fn simple(t: &Scratch) {
    fs::write(
        t.file("app.vt"),
        "module app; import helpers; pub fn start() -> u32 { return helpers::answer(); }",
    )
    .unwrap();
    fs::write(t.file("helper.vt"),"module helpers; extern abi(C) fn external_answer() -> u32; pub fn answer() -> u32 { unsafe { return external_answer(); } }").unwrap();
    fs::write(
        t.file("linux.s0"),
        "external_answer:\naddi x10, x0, 42\njalr x0, 0(x1)\n",
    )
    .unwrap();
    fs::write(t.file("project"), manifest("out.elf")).unwrap();
}
#[test]
fn project_builds_without_tools_is_deterministic_and_resolves_paths_from_root() {
    let t = Scratch::new();
    simple(&t);
    fs::create_dir(t.file("nested")).unwrap();
    fs::rename(t.file("project"), t.file("nested/project")).unwrap();
    success(
        Command::new(BIN)
            .env("PATH", "")
            .arg("project")
            .arg(t.file("nested/project"))
            .current_dir(&t.0),
    );
    let first = fs::read(t.file("out.elf")).unwrap();
    assert_eq!(
        run(Command::new("qemu-riscv64").arg(t.file("out.elf")))
            .status
            .code(),
        Some(42)
    );
    success(
        Command::new(BIN)
            .env("PATH", "")
            .arg("project")
            .arg(t.file("nested/project"))
            .arg("--root")
            .arg(&t.0),
    );
    assert_eq!(fs::read(t.file("out.elf")).unwrap(), first);
}
#[test]
fn project_errors_keep_outputs_and_report_the_correct_source() {
    let t = Scratch::new();
    simple(&t);
    fs::write(t.file("out.elf"), b"previous output").unwrap();
    for (file, text, code) in [
        (
            "helper.vt",
            "module helpers;\npub fn answer() -> u32 { return false; }",
            "helper.vt:2:",
        ),
        (
            "helper.vt",
            "module helpers; fn answer() -> u32 { return 0; }",
            "E_NAME",
        ),
        (
            "helper.vt",
            "module helpers; extern abi(C) fn unused() -> u32; pub fn answer() -> u32 { return 0; }",
            "I_LINK",
        ),
        ("linux.s0", "# line one\nli x10, 42\n", "linux.s0:2:"),
        (
            "linux.s0",
            "external_answer:\naddi x10,x0,42\nexternal_answer:\njalr x0,0(x1)\n",
            "E_PARSE",
        ),
    ] {
        simple(&t);
        fs::write(t.file(file), text).unwrap();
        let o = run(Command::new(BIN)
            .arg("project")
            .arg(t.file("project"))
            .arg("--root")
            .arg(&t.0));
        assert!(!o.status.success());
        assert!(
            String::from_utf8_lossy(&o.stderr).contains(code),
            "{file}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert_eq!(fs::read(t.file("out.elf")).unwrap(), b"previous output");
    }
    simple(&t);
    fs::remove_file(t.file("linux.s0")).unwrap();
    let o = run(Command::new(BIN)
        .arg("project")
        .arg(t.file("project"))
        .arg("--root")
        .arg(&t.0));
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("I_IO"));
    assert_eq!(fs::read(t.file("out.elf")).unwrap(), b"previous output");
}
#[test]
fn project_cannot_publish_over_any_input_alias() {
    for input in ["app.vt", "helper.vt", "linux.s0", "project"] {
        let t = Scratch::new();
        simple(&t);
        let original = fs::read(t.file(input)).unwrap();
        #[cfg(unix)]
        fs::hard_link(t.file(input), t.file("out.elf")).unwrap();
        #[cfg(not(unix))]
        {
            fs::write(t.file("project"), manifest(input)).unwrap();
        }
        let o = run(Command::new(BIN)
            .arg("project")
            .arg(t.file("project"))
            .arg("--root")
            .arg(&t.0));
        assert!(!o.status.success());
        assert!(
            String::from_utf8_lossy(&o.stderr).contains("I_IO"),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert_eq!(fs::read(t.file(input)).unwrap(), original);
    }
}
#[test]
fn cli_builds_the_target_language_manifest_scanner_and_copy_app() {
    let t = Scratch::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let names = [
        "runtime/bytes.vt",
        "runtime/linux.vt",
        "tests/fixtures/b2-input/source.vt",
        "tests/fixtures/b2-input/scan.vt",
        "tests/fixtures/b2-input/manifest.vt",
        "tests/fixtures/b2-input/copy.vt",
    ];
    let mut project = String::from("format 1\nprofile linux-user\nentry app::start\n");
    for name in names {
        let dest = t.file(name);
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::copy(root.join(name), dest).unwrap();
        project.push_str(&format!("source {name}\n"));
    }
    fs::copy(root.join("runtime/linux.s0"), t.file("runtime/linux.s0")).unwrap();
    project.push_str("assembly runtime/linux.s0\noutput copy.elf\n");
    fs::write(t.file("project"), project).unwrap();
    success(
        Command::new(BIN)
            .env("PATH", "")
            .arg("project")
            .arg(t.file("project"))
            .arg("--root")
            .arg(&t.0),
    );
    let source = b"fn answer() -> u32 { return 42; }\n";
    fs::write(t.file("input.vt"), source).unwrap();
    fs::write(
        t.file("copy.manifest"),
        "format 1\nprofile linux-user\nentry unused\nsource input.vt\noutput copied.vt\n",
    )
    .unwrap();
    success(
        Command::new("qemu-riscv64")
            .arg(t.file("copy.elf"))
            .arg("copy.manifest")
            .current_dir(&t.0),
    );
    assert_eq!(fs::read(t.file("copied.vt")).unwrap(), source);
}
