//! Explicit cross-tool invocation. No shell expansion or implicit host libraries.
use crate::diagnostic::{Diagnostic, Result};
use crate::source::Span;
use serde::Serialize;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Emit {
    Assembly,
    Object,
    Elf,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub clang: OsString,
    pub linker: OsString,
    pub sysroot: PathBuf,
    pub platform: Option<String>,
    pub script: Option<PathBuf>,
    pub objects: Vec<PathBuf>,
    pub output: PathBuf,
    pub emit: Emit,
    pub test: bool,
    pub keep_temporaries: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Invocation {
    pub program: String,
    pub arguments: Vec<String>,
    pub version: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct BuildReport {
    pub output: PathBuf,
    pub invocations: Vec<Invocation>,
    pub platform_inputs: Vec<PathBuf>,
    pub temporaries: Option<PathBuf>,
    pub linkage_sidecar: Option<PathBuf>,
}

fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(code, Span::default(), message)
}

fn io(error: std::io::Error) -> Diagnostic {
    self::error("I_IO", error.to_string())
}

fn invoke(program: &OsString, arguments: &[OsString], report: &mut BuildReport) -> Result<()> {
    let version = Command::new(program)
        .arg("--version")
        .output()
        .map_err(|e| error("I_TOOLCHAIN", format!("{}: {e}", program.to_string_lossy())))?;
    if !version.status.success() {
        return Err(error("I_TOOLCHAIN", "tool version query failed"));
    }
    let version = String::from_utf8_lossy(&version.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .to_owned();
    report.invocations.push(Invocation {
        program: program.to_string_lossy().into_owned(),
        arguments: arguments
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect(),
        version,
    });
    let result = Command::new(program).args(arguments).output().map_err(io)?;
    if !result.status.success() {
        return Err(error(
            "E_LINK",
            format!(
                "{} failed: {}",
                program.to_string_lossy(),
                String::from_utf8_lossy(&result.stderr)
            ),
        ));
    }
    Ok(())
}

struct Scratch {
    path: PathBuf,
    keep: bool,
}

impl Scratch {
    fn new(keep: bool) -> Result<Self> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        for _ in 0..1000 {
            let id = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("biomiya-{}-{id}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path, keep }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io(e)),
            }
        }
        Err(error("I_IO", "cannot allocate temporary build directory"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub fn build(
    assembly: &str,
    linkage: &crate::linkage::LinkageNames,
    options: &Options,
) -> Result<BuildReport> {
    if let Some(platform) = &options.platform
        && platform != "qemu-virt-core-v1"
    {
        return Err(error(
            "E_TARGET",
            format!("unsupported platform {platform}"),
        ));
    }
    if let Some(parent) = options
        .output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(io)?;
    }
    let scratch = Scratch::new(options.keep_temporaries)?;
    let assembly_path = scratch.path.join("program.s");
    fs::write(&assembly_path, assembly).map_err(io)?;
    let mut report = BuildReport {
        output: options.output.clone(),
        invocations: vec![],
        platform_inputs: vec![],
        temporaries: options.keep_temporaries.then(|| scratch.path.clone()),
        linkage_sidecar: None,
    };
    if options.emit == Emit::Assembly {
        if linkage.is_empty() {
            fs::copy(&assembly_path, &options.output).map_err(io)?;
        } else {
            let sidecar = linkage_sidecar(&options.output);
            fs::write(
                &sidecar,
                serde_json::to_vec_pretty(linkage).expect("linkage serialization"),
            )
            .map_err(io)?;
            fs::write(
                &options.output,
                format!("# biomiya-linkage-sidecar-required\n{assembly}"),
            )
            .map_err(io)?;
            report.linkage_sidecar = Some(sidecar);
        }
        return Ok(report);
    }
    let mut sources = vec![assembly_path];
    if options.emit == Emit::Elf && options.platform.is_some() {
        for relative in [
            "platforms/qemu-virt/start.S",
            "runtime/rv64/trap.S",
            "runtime/rv64/runtime.c",
        ] {
            let path = options.sysroot.join(relative);
            if !path.is_file() {
                return Err(error(
                    "E_LINK",
                    format!("missing platform input {}", path.display()),
                ));
            }
            report.platform_inputs.push(path.clone());
            sources.push(path);
        }
    }
    let mut objects = vec![];
    for (index, source) in sources.iter().enumerate() {
        let object = scratch.path.join(format!("input{index}.o"));
        let mut args: Vec<OsString> = [
            "--target=riscv64-unknown-elf",
            "-march=rv64ima_zicsr_zifencei",
            "-mabi=lp64",
            "-mno-relax",
            "-mcmodel=medany",
            "-ffreestanding",
            "-fno-builtin",
            "-fno-stack-protector",
            "-fno-unwind-tables",
            "-fno-asynchronous-unwind-tables",
            "-O1",
            "-I",
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        args.push(options.sysroot.join("runtime/rv64").into_os_string());
        args.extend([
            OsString::from("-c"),
            source.as_os_str().to_owned(),
            OsString::from("-o"),
            object.as_os_str().to_owned(),
        ]);
        invoke(&options.clang, &args, &mut report)?;
        if index == 0 && !linkage.is_empty() {
            let restored =
                crate::linkage::restore_object_names(&fs::read(&object).map_err(io)?, linkage)?;
            fs::write(&object, restored).map_err(io)?;
        }
        validate_elf(&fs::read(&object).map_err(io)?)?;
        objects.push(object);
    }
    if options.emit == Emit::Object {
        fs::copy(&objects[0], &options.output).map_err(io)?;
        return Ok(report);
    }
    let script = match &options.script {
        Some(script) => script.clone(),
        None if options.platform.is_some() => options.sysroot.join("platforms/qemu-virt/link.ld"),
        None => {
            return Err(error(
                "E_LINK",
                "ELF emission requires --platform or --link-script",
            ));
        }
    };
    if !script.is_file() {
        return Err(error(
            "E_LINK",
            format!("missing linker script {}", script.display()),
        ));
    }
    report.platform_inputs.push(script.clone());
    objects.extend(options.objects.iter().cloned());
    let elf = scratch.path.join("program.elf");
    let mut args: Vec<OsString> = [
        "-m",
        "elf64lriscv",
        "--no-relax",
        "--no-undefined",
        "--build-id=none",
        "-T",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    args.push(script.into_os_string());
    args.extend(objects.iter().map(|p| p.as_os_str().to_owned()));
    args.extend([OsString::from("-o"), elf.as_os_str().to_owned()]);
    let map = scratch.path.join("program.map");
    args.push(OsString::from(format!("--Map={}", map.display())));
    invoke(&options.linker, &args, &mut report)?;
    let bytes = fs::read(&elf).map_err(io)?;
    validate_elf(&bytes)?;
    if options.platform.is_some() {
        let required_entry = if options.test {
            "__biomiya_test_entry"
        } else {
            "kernel_main"
        };
        if !has_defined_symbol(&bytes, required_entry)? {
            return Err(error(
                "E_LINK",
                format!("platform requires defined entry {required_entry}"),
            ));
        }
    }
    fs::copy(&elf, &options.output).map_err(io)?;
    Ok(report)
}

fn bytes_at(bytes: &[u8], offset: usize, count: usize) -> Result<&[u8]> {
    let end = offset
        .checked_add(count)
        .ok_or_else(|| error("E_LINK", "ELF offset overflow"))?;
    bytes
        .get(offset..end)
        .ok_or_else(|| error("E_LINK", "truncated ELF"))
}
fn u16_at(bytes: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes_at(bytes, offset, 2)?.try_into().unwrap(),
    ))
}
fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes_at(bytes, offset, 4)?.try_into().unwrap(),
    ))
}
fn u64_at(bytes: &[u8], offset: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(
        bytes_at(bytes, offset, 8)?.try_into().unwrap(),
    ))
}
fn size_at(bytes: &[u8], offset: usize) -> Result<usize> {
    usize::try_from(u64_at(bytes, offset)?)
        .map_err(|_| error("E_LINK", "ELF offset exceeds host address space"))
}

pub fn validate_elf(bytes: &[u8]) -> Result<()> {
    bytes_at(bytes, 0, 64)?;
    if bytes.get(..7) != Some(&[0x7f, b'E', b'L', b'F', 2, 1, 1]) || u16_at(bytes, 18)? != 243 {
        return Err(error("E_LINK", "expected little-endian ELF64 for RISC-V"));
    }
    if u32_at(bytes, 48)? != 0 {
        return Err(error(
            "E_LINK",
            "ELF flags require unsupported compressed, floating ABI, RVE, TSO or unknown features",
        ));
    }
    validate_attributes(bytes)
}

/// Attribute metadata is an external-object contract, not instruction decoding.
/// Missing metadata is rejected; dishonest hand assembly remains the producer's
/// responsibility. Supported base versions include compatible I2.0 and A2.0.
fn validate_attributes(bytes: &[u8]) -> Result<()> {
    let table = size_at(bytes, 40)?;
    let stride = u16_at(bytes, 58)? as usize;
    if table == 0 {
        return Err(error("E_LINK", "missing RISC-V ISA attributes"));
    }
    if stride != 64 {
        return Err(error("E_LINK", "invalid ELF section stride"));
    }
    let mut count = u16_at(bytes, 60)? as usize;
    if count == 0 {
        count = size_at(bytes_at(bytes, table, 64)?, 32)?;
    }
    let total = count
        .checked_mul(stride)
        .ok_or_else(|| error("E_LINK", "ELF section overflow"))?;
    let sections = bytes_at(bytes, table, total)?;
    let mut arch = false;
    for section in sections.chunks_exact(stride) {
        if u32_at(section, 4)? != 0x7000_0003 {
            continue;
        }
        let data = bytes_at(bytes, size_at(section, 24)?, size_at(section, 32)?)?;
        if data.first() != Some(&b'A') {
            return Err(error("E_LINK", "invalid RISC-V attribute format"));
        }
        let mut cursor = 1;
        while cursor < data.len() {
            let start = cursor;
            let length = u32_at(data, cursor)? as usize;
            if length < 5 {
                return Err(error("E_LINK", "invalid attribute vendor length"));
            }
            let vendor = bytes_at(data, start, length)?;
            cursor += length;
            let mut offset = 4;
            let name = attribute_string(vendor, &mut offset)?;
            if name != b"riscv" {
                continue;
            }
            while offset < vendor.len() {
                let start = offset;
                let tag = attribute_uleb(vendor, &mut offset)?;
                let length = u32_at(vendor, offset)? as usize;
                offset += 4;
                if length < offset - start {
                    return Err(error("E_LINK", "invalid attribute subsection length"));
                }
                let subsection = bytes_at(vendor, start, length)?;
                let mut field = offset - start;
                offset = start + length;
                if tag != 1 {
                    return Err(error("E_LINK", "unsupported RISC-V attribute subsection"));
                }
                while field < subsection.len() {
                    let tag = attribute_uleb(subsection, &mut field)?;
                    if tag == 5 {
                        validate_arch(attribute_string(subsection, &mut field)?)?;
                        arch = true;
                    } else if tag == 32 {
                        attribute_uleb(subsection, &mut field)?;
                        attribute_string(subsection, &mut field)?;
                    } else if tag % 2 == 1 {
                        attribute_string(subsection, &mut field)?;
                    } else {
                        let value = attribute_uleb(subsection, &mut field)?;
                        if (tag == 4 && value != 16) || (tag == 6 && value != 0) {
                            return Err(error(
                                "E_LINK",
                                "incompatible RISC-V stack alignment or unaligned-access attributes",
                            ));
                        }
                    }
                }
            }
        }
    }
    if !arch {
        return Err(error("E_LINK", "missing RISC-V ISA attributes"));
    }
    Ok(())
}
fn attribute_string<'a>(data: &'a [u8], cursor: &mut usize) -> Result<&'a [u8]> {
    let tail = data
        .get(*cursor..)
        .ok_or_else(|| error("E_LINK", "truncated attribute string"))?;
    let length = tail
        .iter()
        .position(|b| *b == 0)
        .ok_or_else(|| error("E_LINK", "unterminated attribute string"))?;
    *cursor += length + 1;
    Ok(&tail[..length])
}
fn attribute_uleb(data: &[u8], cursor: &mut usize) -> Result<u64> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let byte = *data
            .get(*cursor)
            .ok_or_else(|| error("E_LINK", "truncated attribute integer"))?;
        *cursor += 1;
        if shift == 63 && byte > 1 {
            return Err(error("E_LINK", "attribute integer overflow"));
        }
        value |= u64::from(byte & 127) << shift;
        if byte & 128 == 0 {
            return Ok(value);
        }
    }
    Err(error("E_LINK", "attribute integer overflow"))
}
fn validate_arch(arch: &[u8]) -> Result<()> {
    let arch =
        std::str::from_utf8(arch).map_err(|_| error("E_LINK", "invalid ISA attribute encoding"))?;
    let allowed = [
        "rv64i2p1",
        "rv64i2p0",
        "m2p0",
        "a2p1",
        "a2p0",
        "zicsr2p0",
        "zifencei2p0",
        "zmmul1p0",
        "zaamo1p0",
        "zalrsc1p0",
    ];
    let parts = arch.split('_').collect::<Vec<_>>();
    if !matches!(parts.first(), Some(&"rv64i2p0" | &"rv64i2p1"))
        || parts.iter().any(|p| !allowed.contains(p))
        || parts.iter().skip(1).any(|p| p.starts_with("rv64"))
        || (parts.contains(&"a2p0") && parts.contains(&"a2p1"))
        || parts
            .iter()
            .enumerate()
            .any(|(i, p)| parts[..i].contains(p))
    {
        return Err(error(
            "E_LINK",
            format!("ISA attributes exceed rv64-core-v1: {arch}"),
        ));
    }
    Ok(())
}

/// Read the final symbol table so a weak runtime fallback cannot hide a missing
/// required program entry. All offsets are bounds checked before arithmetic.
pub fn has_defined_symbol(bytes: &[u8], wanted: &str) -> Result<bool> {
    validate_elf(bytes)?;
    let table = size_at(bytes, 40)?;
    let stride = u16_at(bytes, 58)? as usize;
    let count = u16_at(bytes, 60)? as usize;
    if stride < 64 {
        return Err(error("E_LINK", "invalid ELF section table"));
    }
    let table_size = count
        .checked_mul(stride)
        .ok_or_else(|| error("E_LINK", "ELF section overflow"))?;
    let sections = bytes_at(bytes, table, table_size)?;
    for section in sections.chunks_exact(stride) {
        if u32_at(section, 4)? != 2 {
            continue;
        }
        let offset = size_at(section, 24)?;
        let size = size_at(section, 32)?;
        let strings = u32_at(section, 40)? as usize;
        let symbol_stride = size_at(section, 56)?;
        if strings >= count || symbol_stride < 24 || !size.is_multiple_of(symbol_stride) {
            return Err(error("E_LINK", "invalid ELF symbol table"));
        }
        let symbols = bytes_at(bytes, offset, size)?;
        let string_section = &sections[strings * stride..][..stride];
        let string_data = bytes_at(
            bytes,
            size_at(string_section, 24)?,
            size_at(string_section, 32)?,
        )?;
        for symbol in symbols.chunks_exact(symbol_stride) {
            let name_offset = u32_at(symbol, 0)? as usize;
            if u16_at(symbol, 6)? == 0 {
                continue;
            }
            let name = string_data
                .get(name_offset..)
                .ok_or_else(|| error("E_LINK", "invalid symbol name"))?;
            let end = name
                .iter()
                .position(|b| *b == 0)
                .ok_or_else(|| error("E_LINK", "unterminated symbol name"))?;
            if &name[..end] == wanted.as_bytes() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn default_sysroot() -> PathBuf {
    std::env::var_os("BIOMIYA_SYSROOT").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).to_owned(),
        PathBuf::from,
    )
}

pub fn linkage_sidecar(assembly: &Path) -> PathBuf {
    let mut path = assembly.as_os_str().to_owned();
    path.push(".linkage.json");
    PathBuf::from(path)
}
