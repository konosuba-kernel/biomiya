//! Host seed driver: real Core S source to a standalone RV64 Linux executable.
use biomiya_core_s::{
    backend,
    diagnostic::{Diagnostic, Result as CompileResult},
    image,
    machine::{IOp, Inst, Program, RA, SP},
    model::{Compilation, Integer, TypeKind},
    pipeline, project, s0,
    source::{FileId, SourceMap, Span},
};
use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const USAGE: &str = "usage: core-s check INPUT\n       core-s build INPUT --entry FUNCTION -o OUTPUT\n       core-s asm INPUT --entry FUNCTION -o OUTPUT\n       core-s project MANIFEST [--root DIRECTORY]";

struct Arguments {
    command: String,
    input: PathBuf,
    entry: Option<String>,
    output: Option<PathBuf>,
    root: PathBuf,
}

fn arguments() -> Result<Arguments, String> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or(USAGE)?;
    if command == "--help" && args.next().is_none() {
        println!("{USAGE}");
        std::process::exit(0);
    }
    if !["check", "build", "asm", "project"].contains(&command.as_str()) {
        return Err(USAGE.into());
    }
    let input = args.next().ok_or(USAGE)?.into();
    let mut entry = None;
    let mut output = None;
    let mut root = None;
    while let Some(option) = args.next() {
        match option.as_str() {
            "--root" if command == "project" && root.is_none() => {
                root = Some(PathBuf::from(args.next().ok_or(USAGE)?))
            }
            "--entry" if entry.is_none() => entry = Some(args.next().ok_or(USAGE)?),
            "-o" if output.is_none() => output = Some(PathBuf::from(args.next().ok_or(USAGE)?)),
            _ => return Err(USAGE.into()),
        }
    }
    if (matches!(command.as_str(), "check" | "project") && (entry.is_some() || output.is_some()))
        || (matches!(command.as_str(), "build" | "asm") && (entry.is_none() || output.is_none()))
    {
        return Err(USAGE.into());
    }
    Ok(Arguments {
        command,
        input,
        entry,
        output,
        root: root.unwrap_or_else(|| PathBuf::from(".")),
    })
}

fn runtime(compilation: &Compilation, entry: &str) -> CompileResult<Program> {
    let entry = entry.strip_prefix("root::").unwrap_or(entry);
    let function = compilation
        .functions
        .iter()
        .find(|f| f.name == entry)
        .ok_or_else(|| {
            Diagnostic::new(
                "E_ENTRY",
                Span::default(),
                format!("unknown entry function {entry}"),
            )
        })?;
    if !function.signature.parameters.is_empty()
        || function.signature.unsafe_
        || compilation.types.entries[function.signature.result.0 as usize].kind
            != TypeKind::Int(Integer::U32)
    {
        return Err(Diagnostic::new(
            "E_ENTRY",
            function.span,
            "entry must be a safe fn() -> u32",
        ));
    }
    let mut program = Program {
        instructions: vec![
            Inst::Label("_start".into()),
            Inst::I {
                op: IOp::Addi,
                rd: 27,
                rs1: SP,
                imm: 0,
            },
            Inst::Jump {
                rd: RA,
                target: function.symbol.clone(),
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
        data: Vec::new(),
    };
    let body = backend::machine(compilation)?;
    program.instructions.extend(body.instructions);
    program.data = body.data;
    Ok(program)
}

fn same_file(input: &Path, output: &Path) -> std::io::Result<bool> {
    let original = fs::metadata(input)?;
    let destination = match fs::metadata(output) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(original.dev() == destination.dev() && original.ino() == destination.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = (original, destination);
        Ok(fs::canonicalize(input)? == fs::canonicalize(output)?)
    }
}

fn publish(path: &Path, bytes: &[u8], executable: bool) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // create_new and a bounded retry never truncate a file owned by another run.
    for serial in 0..64 {
        let temporary = parent.join(format!(".core-s-{}-{serial}.tmp", std::process::id()));
        let mut file = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let result = (|| {
            file.write_all(bytes)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(fs::Permissions::from_mode(if executable {
                    0o755
                } else {
                    0o644
                }))?;
            }
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        return result;
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "temporary output names exhausted",
    ))
}

fn read_text(path: &Path, limit: usize) -> Result<String, String> {
    let io_error = |e: std::io::Error| format!("I_IO: {}: {e}", path.display());
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(io_error)?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > limit {
        return Err(format!("I_LIMIT: {} exceeds {limit} bytes", path.display()));
    }
    String::from_utf8(bytes).map_err(|_| format!("E_UTF8: {}: invalid UTF-8 input", path.display()))
}

fn diagnostic(sources: &SourceMap, error: Diagnostic) -> String {
    let (line, column) = sources.line_column(error.span);
    format!(
        "{}:{line}:{column}: {error}",
        sources.get(error.span.file).path.display()
    )
}

fn run() -> Result<(), String> {
    let args = arguments()?;
    let mut sources = SourceMap::default();
    let mut inputs = vec![args.input.clone()];
    let mut source_ids = Vec::new();
    let mut assembly_ids = Vec::new();
    let (entry, output) = if args.command == "project" {
        let manifest = project::parse(&read_text(&args.input, project::MAX_MANIFEST)?)?;
        for name in &manifest.sources {
            let path = args.root.join(name);
            let text = read_text(&path, pipeline::MAX_SOURCE_BYTES)?;
            source_ids.push(sources.add(&path, text));
            inputs.push(path);
        }
        for name in &manifest.assemblies {
            let path = args.root.join(name);
            let text = read_text(&path, pipeline::MAX_SOURCE_BYTES)?;
            assembly_ids.push(sources.add(&path, text));
            inputs.push(path);
        }
        (Some(manifest.entry), Some(args.root.join(manifest.output)))
    } else {
        source_ids.push(sources.add(
            &args.input,
            read_text(&args.input, pipeline::MAX_SOURCE_BYTES)?,
        ));
        (args.entry, args.output)
    };
    let files: Vec<(FileId, &str)> = source_ids
        .iter()
        .map(|id| (*id, sources.get(*id).text.as_str()))
        .collect();
    let compilation = pipeline::compile_files(&files).map_err(|e| diagnostic(&sources, e))?;
    if args.command == "check" {
        return Ok(());
    }
    let output = output.as_ref().unwrap();
    for input in &inputs {
        if same_file(input, output).map_err(|e| format!("I_IO: {}: {e}", output.display()))? {
            return Err(format!("I_IO: output aliases input {}", input.display()));
        }
    }
    let mut assembly = Program::default();
    for id in assembly_ids {
        let part = s0::parse(id, &sources.get(id).text).map_err(|e| diagnostic(&sources, e))?;
        assembly.instructions.extend(part.instructions);
        assembly.data.extend(part.data);
    }
    // Declarations require providers even when no call happens to reference them.
    for symbol in compilation.externs.values() {
        if !assembly
            .instructions
            .iter()
            .any(|i| matches!(i, Inst::Label(name) if name == symbol))
        {
            return Err(format!(
                "I_LINK: external symbol {symbol} has no .s0 provider"
            ));
        }
    }
    let mut program =
        runtime(&compilation, entry.as_deref().unwrap()).map_err(|e| diagnostic(&sources, e))?;
    program.instructions.extend(assembly.instructions);
    program.data.extend(assembly.data);
    let executable = args.command != "asm";
    let bytes = if executable {
        image::executable(&program, "_start").map_err(|e| e.to_string())?
    } else {
        backend::assembly(&program)
            .map_err(|e| e.to_string())?
            .into_bytes()
    };
    publish(output, &bytes, executable).map_err(|e| format!("I_IO: {}: {e}", output.display()))
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
