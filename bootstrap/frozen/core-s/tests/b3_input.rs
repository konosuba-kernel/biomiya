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
            "core-s-b3-input-{}-{}",
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
const PROBE: &str = r#"module probe;
import bootstrap::model;
import bootstrap::lex;
import bootstrap::input;
import runtime::linux;
pub fn run() -> u32 {
 model::reset_all();
 var path:[]const u8 = "";
 match linux::argument(1) { Result::<[]const u8,u32>::Ok(p:[]const u8)=>{path=p;},Result::<[]const u8,u32>::Err(e:u32)=>{return 2;}, }
 input::load(path);
 unsafe {
  if !model::equal(input::entry,"probe::run") {return 3;}
  var file:u32 = 1;
  while file <= model::source_count {
   var offset:usize = 0;
   loop {let t:model::Token=lex::next(file,offset);if t.kind==0_u32 {break;} offset=t.end as usize;}
   file=file+1_u32;
  }
  input::save(model::sources[1].bytes);
 }
 return 0;
}
pub fn tokens() -> u32 {
 model::reset_all();
 unsafe {
  model::source_count=1;
  model::sources[1]=model::Source {path:"test",bytes:"/*a /*b*/ */ 0xff_u8 123_456_usize abc \"a\\x00\" <<",module_id:0,import_first:0,import_count:0};
  let a:model::Token=lex::next(1,0);
  if a.kind!=2_u32 || a.number!=255_usize || a.suffix!=3_u32 || a.start!=13_u32 || a.end!=20_u32 {return 1;}
  let b:model::Token=lex::next(1,a.end as usize);
  if b.number!=123456_usize || b.suffix!=5_u32 {return 2;}
  let c:model::Token=lex::next(1,b.end as usize);
  if !lex::is(c,"abc") {return 3;}
  let d:model::Token=lex::next(1,c.end as usize);
  var out:[4]u8=[0;4];let n:usize=lex::decode(d,core::slice_of_mut(out));
  if n!=2_usize || out[0]!=97_u8 || out[1]!=0_u8 {return 4;}
  let e:model::Token=lex::next(1,d.end as usize);if !lex::is(e,"<<") {return 5;}
  let f:model::Token=lex::next(1,e.end as usize);if f.kind!=0_u32 {return 6;}
 }
 return 0;
}
"#;
fn build(entry: &str, short: bool) -> Vec<u8> {
    let t = Scratch::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = [
        "bootstrap/model.vt",
        "bootstrap/lex.vt",
        "bootstrap/input.vt",
        "runtime/linux.vt",
        "runtime/bytes.vt",
    ];
    let mut manifest = format!("format 1\nprofile linux-user\nentry {entry}\n");
    for name in files {
        let p = t.file(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::copy(root.join(name), p).unwrap();
        manifest.push_str(&format!("source {name}\n"));
    }
    let mut asm = fs::read_to_string(root.join("runtime/linux.s0")).unwrap();
    let probe = PROBE.to_owned();
    if short {
        // Clamp requests to seven bytes. The established B2 suite independently injects EINTR.
        asm = asm.replace("__b2_syscall6:", "__b2_real:");
        asm="__b2_syscall6:\naddi x5,x0,63\nbeq x10,x5,.Lshort\naddi x5,x0,64\nbne x10,x5,.Lreal\n.Lshort:\nsltiu x5,x13,8\nbne x5,x0,.Lreal\naddi x13,x0,7\n.Lreal:\njump __b2_real\n".to_owned()+&asm;
    }
    fs::write(t.file("probe.vt"), probe).unwrap();
    fs::write(t.file("linux.s0"), asm).unwrap();
    manifest.push_str("source probe.vt\nassembly linux.s0\noutput probe.elf\n");
    fs::write(t.file("build"), manifest).unwrap();
    success(
        Command::new(BIN)
            .env("PATH", "")
            .arg("project")
            .arg(t.file("build"))
            .arg("--root")
            .arg(&t.0),
    );
    fs::read(t.file("probe.elf")).unwrap()
}
fn image() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| build("probe::run", false))
}
fn project() -> String {
    "format 1\nprofile linux-user\nentry probe::run\nsource input.vt\nassembly adapter.s0\noutput copied.vt\n".into()
}
fn execute(t: &Scratch, bytes: &[u8]) -> Output {
    executable(&t.file("probe.elf"), bytes);
    run(Command::new("qemu-riscv64")
        .arg(t.file("probe.elf"))
        .arg("project")
        .current_dir(&t.0))
}
fn prepare(t: &Scratch, source: &[u8], manifest: &str) {
    fs::write(t.file("input.vt"), source).unwrap();
    fs::write(t.file("adapter.s0"), "# retained assembly snapshot\n").unwrap();
    fs::write(t.file("project"), manifest).unwrap();
}
#[test]
fn target_lexer_decodes_numbers_strings_ranges_and_nested_comments() {
    let t = Scratch::new();
    let o = execute(&t, &build("probe::tokens", false));
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}
#[test]
fn target_input_keeps_manifest_snapshots_and_handles_short_io() {
    for short in [false, true] {
        let t = Scratch::new();
        let text = "/* 注释 */ fn example()->u32{return 0xff_u8 as u32;}\n".repeat(180);
        prepare(&t, text.as_bytes(), &(project() + "source empty.vt\n"));
        fs::write(t.file("empty.vt"), b"").unwrap();
        let custom;
        if short {
            custom = build("probe::run", true);
        } else {
            custom = image().to_vec();
        }
        let o = execute(&t, &custom);
        assert_eq!(
            o.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert_eq!(fs::read(t.file("copied.vt")).unwrap(), text.as_bytes());
        let o = execute(&t, &custom);
        assert_eq!(o.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&o.stderr).contains("E_IO"));
        assert_eq!(fs::read(t.file("copied.vt")).unwrap(), text.as_bytes());
    }
}
#[test]
fn target_rejects_bad_numeric_tokens_utf8_and_unsupported_identifiers() {
    for (text, code, offset) in [
        (&b"0x"[..], "E_PARSE", 0),
        (&b"1__2"[..], "E_PARSE", 2),
        (&b"256_u8"[..], "E_TYPE", 0),
        (&b"18446744073709551616"[..], "E_TYPE", 0),
        (&b"0b2"[..], "E_PARSE", 2),
        (&b"1_u64"[..], "I_UNSUPPORTED", 1),
        (&b"\"\\q\""[..], "E_PARSE", 2),
        (&b"/*bad"[..], "E_PARSE", 0),
        (&b"\xff"[..], "E_PARSE", 0),
        ("变量".as_bytes(), "I_UNSUPPORTED", 0),
    ] {
        let t = Scratch::new();
        prepare(&t, text, &project());
        let o = execute(&t, image());
        assert_eq!(o.status.code(), Some(1));
        let e = String::from_utf8_lossy(&o.stderr);
        assert!(e.contains(code), "{text:?}: {e}");
        assert!(e.contains(&format!("file=1 byte={offset}\n")), "{e}");
        assert!(!t.file("copied.vt").exists());
    }
}
#[test]
fn target_manifest_limits_and_source_aliases_preserve_inputs() {
    for m in [
        project() + "entry twice\n",
        project().replace("source input.vt", "source ../input.vt"),
        project() + "assembly input.vt\n",
        project().replace("linux-user", "other"),
        project() + "unknown x\n",
        project().replace("input.vt", "missing.vt"),
    ] {
        let t = Scratch::new();
        prepare(&t, b"fn f()->u32{return 1;}", &m);
        assert_eq!(execute(&t, image()).status.code(), Some(1));
        assert!(!t.file("copied.vt").exists());
    }
    let t = Scratch::new();
    prepare(
        &t,
        b"fn f()->u32{return 1;}",
        &project().replace("output copied.vt", "output input.vt"),
    );
    assert_eq!(execute(&t, image()).status.code(), Some(1));
    assert_eq!(
        fs::read(t.file("input.vt")).unwrap(),
        b"fn f()->u32{return 1;}"
    );
    let t = Scratch::new();
    prepare(&t, &vec![32; 4 * 1024 * 1024 + 1], &project());
    let o = execute(&t, image());
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("actual=4194305 limit=4194304 I_LIMIT file=1 byte=4194304"));
    assert!(!t.file("copied.vt").exists());
}
