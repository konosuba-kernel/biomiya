use biomiya::toolchain::{has_defined_symbol, validate_elf};
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "biomiya-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn cli(args: &[&str]) -> (i32, Value) {
    let result = Command::new(env!("CARGO_BIN_EXE_biomiya"))
        .args(args)
        .args(["--diagnostics", "json"])
        .output()
        .unwrap();
    let report = serde_json::from_slice(&result.stdout)
        .unwrap_or_else(|_| panic!("invalid JSON: {:?}", result));
    (result.status.code().unwrap(), report)
}

#[test]
fn cli_error_classes_and_unavailable_verification() {
    let directory = Directory::new();
    let source = directory.0.join("valid.vt");
    fs::write(&source, "fn test()->i32{7}").unwrap();
    let source = source.to_str().unwrap();
    let (exit, report) = cli(&["check", source, source]);
    assert_eq!(exit, 2);
    assert_eq!(report["diagnostics"][0]["code"], "I_USAGE");
    let (exit, report) = cli(&["check", source, "--target", "unknown"]);
    assert_eq!(exit, 1);
    assert_eq!(report["diagnostics"][0]["code"], "E_TARGET");
    let output = directory.0.join("must-not-publish.s");
    let (exit, report) = cli(&[
        "build",
        source,
        "--emit",
        "asm",
        "-o",
        output.to_str().unwrap(),
        "--verify",
        "ownership",
    ]);
    assert_eq!(exit, 2);
    assert_eq!(report["diagnostics"][0]["code"], "I_UNSUPPORTED");
    assert!(!output.exists());
    let (exit, report) = cli(&["check", source]);
    assert_eq!(exit, 0);
    assert!(
        report["analyses"]
            .as_array()
            .unwrap()
            .iter()
            .all(|analysis| analysis["status"] == "NotEnabled")
    );
}

#[test]
fn invalid_utf8_diagnostic_points_to_actual_input() {
    let directory = Directory::new();
    let first = directory.0.join("a.vt");
    let second = directory.0.join("b.vt");
    fs::write(&first, "module a; fn f(){}").unwrap();
    fs::write(&second, b"module b;\n\xff").unwrap();
    let (exit, report) = cli(&["check", second.to_str().unwrap(), first.to_str().unwrap()]);
    assert_eq!(exit, 1);
    assert_eq!(report["diagnostics"][0]["code"], "E_LEX");
    assert_eq!(report["diagnostics"][0]["span"]["file"], 1);
    assert_eq!(report["diagnostics"][0]["span"]["start"], 10);
    assert_eq!(report["sources"][1]["path"], second.to_str().unwrap());
}

#[test]
fn malformed_elf_reports_link_error_without_host_panic() {
    for length in 0..64 {
        assert!(validate_elf(&vec![0; length]).is_err());
    }
    let mut elf = vec![0; 64];
    elf[..7].copy_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1]);
    elf[18..20].copy_from_slice(&243u16.to_le_bytes());
    elf[40..48].copy_from_slice(&u64::MAX.to_le_bytes());
    elf[58..60].copy_from_slice(&64u16.to_le_bytes());
    elf[60..62].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        has_defined_symbol(&elf, "kernel_main").unwrap_err().code,
        "E_LINK"
    );
    elf[48..52].copy_from_slice(&1u32.to_le_bytes());
    assert!(validate_elf(&elf).is_err());
}

fn attributed_elf(attributes: &[u8]) -> Vec<u8> {
    let mut elf = vec![0; 192];
    elf[..7].copy_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1]);
    elf[18..20].copy_from_slice(&243u16.to_le_bytes());
    elf[40..48].copy_from_slice(&64u64.to_le_bytes());
    elf[58..60].copy_from_slice(&64u16.to_le_bytes());
    elf[60..62].copy_from_slice(&2u16.to_le_bytes());
    elf[132..136].copy_from_slice(&0x70000003u32.to_le_bytes());
    elf[152..160].copy_from_slice(&192u64.to_le_bytes());
    elf[160..168].copy_from_slice(&(attributes.len() as u64).to_le_bytes());
    elf.extend_from_slice(attributes);
    elf
}
fn attributes(arch: &str) -> Vec<u8> {
    let mut file = vec![1];
    file.extend_from_slice(&(6u32 + arch.len() as u32 + 1).to_le_bytes());
    file.push(5);
    file.extend_from_slice(arch.as_bytes());
    file.push(0);
    let mut data = vec![b'A'];
    data.extend_from_slice(&(10u32 + file.len() as u32).to_le_bytes());
    data.extend_from_slice(b"riscv\0");
    data.extend(file);
    data
}
#[test]
fn bounded_isa_attributes_and_flags() {
    let valid = attributes("rv64i2p1_m2p0_a2p1_zicsr2p0_zifencei2p0_zmmul1p0_zaamo1p0_zalrsc1p0");
    assert!(validate_elf(&attributed_elf(&valid)).is_ok());
    assert!(
        validate_elf(&attributed_elf(&attributes(
            "rv64i2p0_m2p0_a2p0_zicsr2p0_zifencei2p0"
        )))
        .is_ok()
    );
    for arch in [
        "rv64i2p1_zbb1p0",
        "rv64i2p1_f2p2",
        "rv64i2p1_v1p0",
        "rv64i9p0",
        "rv64i2p1_m99p0",
        "rv64i2p1_m2p0_m2p0",
        "rv64i2p1_rv64i2p0",
        "rv64i2p1_a2p0_a2p1",
    ] {
        assert_eq!(
            validate_elf(&attributed_elf(&attributes(arch)))
                .unwrap_err()
                .code,
            "E_LINK"
        );
    }
    for end in 0..valid.len() {
        assert!(validate_elf(&attributed_elf(&valid[..end])).is_err());
    }
    let mut huge = valid.clone();
    huge[1..5].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(validate_elf(&attributed_elf(&huge)).is_err());
    let mut overflow = valid.clone();
    overflow[11..].fill(0xff);
    assert!(validate_elf(&attributed_elf(&overflow)).is_err());
    let mut huge_sections = attributed_elf(&valid);
    huge_sections[60..62].fill(0);
    huge_sections[96..104].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(validate_elf(&huge_sections).is_err());
    for flag in [1u32, 2, 4, 8, 16, 0x80000000] {
        let mut elf = attributed_elf(&valid);
        elf[48..52].copy_from_slice(&flag.to_le_bytes());
        assert!(validate_elf(&elf).is_err());
    }
}
#[test]
fn external_zbb_object_rejected_and_normal_artifacts_validated() {
    let directory = Directory::new();
    let assembly = directory.0.join("zbb.s");
    let object = directory.0.join("zbb.o");
    let source = directory.0.join("test.vt");
    let output = directory.0.join("test.elf");
    fs::write(
        &assembly,
        ".text\n.globl zbb_feature\nzbb_feature:\n andn a0,a0,a1\n ret\n",
    )
    .unwrap();
    assert!(
        Command::new("clang")
            .args([
                "--target=riscv64-unknown-elf",
                "-march=rv64ima_zicsr_zifencei_zbb",
                "-mabi=lp64",
                "-c"
            ])
            .arg(&assembly)
            .arg("-o")
            .arg(&object)
            .status()
            .unwrap()
            .success()
    );
    fs::write(&source,"extern abi(C) fn zbb_feature(x:u64,y:u64)->u64;pub fn test()->i32{unsafe{zbb_feature(3,1) as i32}}").unwrap();
    let (exit, report) = cli(&[
        "build",
        source.to_str().unwrap(),
        "--platform",
        "qemu-virt-core-v1",
        "--test",
        "test",
        "--emit",
        "elf",
        "--object",
        object.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(exit, 1);
    assert_eq!(report["diagnostics"][0]["code"], "E_LINK");
    assert!(!output.exists());
    fs::write(&source, "pub fn test()->i32{7}").unwrap();
    let (exit, report) = cli(&[
        "build",
        source.to_str().unwrap(),
        "--platform",
        "qemu-virt-core-v1",
        "--test",
        "test",
        "--emit",
        "elf",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(exit, 0, "{report}");
    assert!(validate_elf(&fs::read(&output).unwrap()).is_ok());
    let object = directory.0.join("normal.o");
    let (exit, report) = cli(&[
        "build",
        source.to_str().unwrap(),
        "--emit",
        "obj",
        "-o",
        object.to_str().unwrap(),
    ]);
    assert_eq!(exit, 0, "{report}");
    assert!(validate_elf(&fs::read(&object).unwrap()).is_ok());
    fs::write(&assembly,".attribute arch,\"rv64i2p1_m2p0_a2p1_zicsr2p0_zifencei2p0_zbb1p0\"\n.text\n.globl zbb_feature\nzbb_feature:\n andn a0,a0,a1\n ret\n").unwrap();
    let rejected = directory.0.join("rejected.o");
    let (exit, report) = cli(&[
        "assemble",
        assembly.to_str().unwrap(),
        "-o",
        rejected.to_str().unwrap(),
    ]);
    assert_eq!(exit, 1);
    assert_eq!(report["diagnostics"][0]["code"], "E_LINK");
    assert!(!rejected.exists());
}
