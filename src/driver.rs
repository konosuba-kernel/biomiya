//! The CLI orchestrates the same front end, IR and backend exported by the library.
use crate::{
    diagnostic::{Diagnostic, Result},
    extend, hir, ir,
    source::{SourceMap, Span},
    toolchain::{self, Emit},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, ffi::OsString, fs, path::PathBuf};

#[derive(Clone, Debug)]
struct Request {
    build: bool,
    assemble: bool,
    linkage: Option<PathBuf>,
    json: bool,
    sources: Vec<PathBuf>,
    target: String,
    verify: Vec<String>,
    test: Option<String>,
    trap_entry: Option<String>,
    dump_ir: Option<PathBuf>,
    report: Option<PathBuf>,
    tools: toolchain::Options,
}

fn diagnostic(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(code, Span::default(), message)
}

fn parse(args: &[OsString]) -> Result<Request> {
    let command = args.first().and_then(|s| s.to_str()).unwrap_or("");
    if !matches!(command, "check" | "build" | "assemble") {
        return Err(diagnostic(
            "I_USAGE",
            "expected check, build or assemble; use --help",
        ));
    }
    let mut request = Request {
        build: command == "build",
        assemble: command == "assemble",
        linkage: None,
        json: false,
        sources: vec![],
        target: crate::TARGET.into(),
        verify: vec![],
        test: None,
        trap_entry: None,
        dump_ir: None,
        report: None,
        tools: toolchain::Options {
            clang: "clang".into(),
            linker: "ld.lld".into(),
            sysroot: toolchain::default_sysroot(),
            platform: None,
            script: None,
            objects: vec![],
            output: if command == "assemble" {
                "a.o"
            } else {
                "a.elf"
            }
            .into(),
            emit: if command == "assemble" {
                Emit::Object
            } else {
                Emit::Elf
            },
            test: false,
            keep_temporaries: false,
        },
    };
    let mut i = 1;
    while i < args.len() {
        let argument = args[i].to_string_lossy();
        if argument == "--" {
            request
                .sources
                .extend(args[i + 1..].iter().map(PathBuf::from));
            break;
        }
        if argument == "--keep-temporaries" {
            request.tools.keep_temporaries = true;
            i += 1;
            continue;
        }
        if !argument.starts_with('-') {
            request.sources.push(PathBuf::from(&args[i]));
            i += 1;
            continue;
        }
        let (option, inline) = argument
            .split_once('=')
            .map_or((argument.as_ref(), None), |(a, b)| (a, Some(b)));
        let value = if let Some(inline) = inline {
            OsString::from(inline)
        } else {
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| diagnostic("I_USAGE", format!("{option} requires a value")))?
        };
        let text = value.to_string_lossy();
        match option {
            "--target" => request.target = text.into_owned(),
            "--diagnostics" => {
                request.json = match text.as_ref() {
                    "json" => true,
                    "human" => false,
                    _ => return Err(diagnostic("I_USAGE", "diagnostics must be json or human")),
                }
            }
            "--platform" => request.tools.platform = Some(text.into_owned()),
            "--emit" => {
                request.tools.emit = match text.as_ref() {
                    "asm" => Emit::Assembly,
                    "obj" => Emit::Object,
                    "elf" => Emit::Elf,
                    _ => return Err(diagnostic("I_USAGE", "emit must be asm, obj or elf")),
                }
            }
            "--test" => {
                request.test = Some(text.into_owned());
                request.tools.test = true;
            }
            "--trap-entry" => request.trap_entry = Some(text.into_owned()),
            "--verify" => request
                .verify
                .extend(text.split(',').map(|s| s.to_ascii_lowercase())),
            "--clang" => request.tools.clang = value,
            "--linker" => request.tools.linker = value,
            "--sysroot" => request.tools.sysroot = value.into(),
            "--link-script" => request.tools.script = Some(value.into()),
            "--object" => request.tools.objects.push(value.into()),
            "-o" | "--output" => request.tools.output = value.into(),
            "--dump-ir" => request.dump_ir = Some(value.into()),
            "--report" => request.report = Some(value.into()),
            "--linkage" => request.linkage = Some(value.into()),
            _ => return Err(diagnostic("I_USAGE", format!("unknown option {option}"))),
        }
        i += 1;
    }
    if request.sources.is_empty() {
        return Err(diagnostic("I_USAGE", "no source files supplied"));
    }
    if request.target != crate::TARGET {
        return Err(diagnostic(
            "E_TARGET",
            format!("unsupported target {}", request.target),
        ));
    }
    if request.assemble
        && (request.sources.len() != 1
            || request.tools.emit != Emit::Object
            || !request.verify.is_empty())
    {
        return Err(diagnostic(
            "I_USAGE",
            "assemble requires one assembly input, --emit obj and no --verify",
        ));
    }
    if !request.assemble && request.linkage.is_some() {
        return Err(diagnostic(
            "I_USAGE",
            "--linkage is only used with assemble",
        ));
    }
    Ok(request)
}

fn assemble(request: &Request) -> Result<Value> {
    let path = &request.sources[0];
    let text = fs::read_to_string(path)
        .map_err(|e| diagnostic("I_IO", format!("{}: {e}", path.display())))?;
    let sidecar = request
        .linkage
        .clone()
        .unwrap_or_else(|| toolchain::linkage_sidecar(path));
    let mut inputs =
        vec![json!({"path":path,"sha256":format!("{:x}", Sha256::digest(text.as_bytes()))})];
    let names = if request.linkage.is_some() || sidecar.is_file() {
        let bytes = fs::read(&sidecar).map_err(|e| diagnostic("I_IO", e.to_string()))?;
        let names = serde_json::from_slice(&bytes)
            .map_err(|e| diagnostic("E_LINK", format!("invalid linkage sidecar: {e}")))?;
        inputs.push(json!({"path":sidecar,"sha256":format!("{:x}", Sha256::digest(&bytes))}));
        names
    } else if text.starts_with("# biomiya-linkage-sidecar-required\n") {
        return Err(diagnostic(
            "E_LINK",
            format!("assembly requires linkage sidecar {}", sidecar.display()),
        ));
    } else {
        crate::linkage::LinkageNames::default()
    };
    let built = toolchain::build(&text, &names, &request.tools)?;
    let output = fs::read(&request.tools.output).map_err(|e| diagnostic("I_IO", e.to_string()))?;
    Ok(
        json!({"schema":1,"status":"ok","diagnostics":[],"target":crate::TARGET,"reference":crate::REFERENCE_VERSION,"build":built,"inputs":inputs,"output_sha256":format!("{:x}", Sha256::digest(&output))}),
    )
}

fn analysis_reports(
    registry: &extend::Registry,
    plan: &extend::Plan,
    sources: &SourceMap,
    program: &hir::Program,
    types: &crate::types::TypeContext,
    module: &ir::Module,
    target: Option<&extend::TargetView<'_>>,
) -> Result<Value> {
    let instances = program
        .functions
        .iter()
        .map(|f| extend::InstanceOrigin {
            definition: f.definition,
            arguments: f.type_arguments.clone(),
            source: f.span,
        })
        .collect::<Vec<_>>();
    let mut view = extend::SemanticView::new(extend::SessionId(0), types, module);
    view.sources = Some(sources);
    view.instances = &instances;
    view.frontend = Some(program);
    view.declarations = program.declarations.as_ref();
    let reports = registry.run(plan, &view, target);
    extend::strict_gate(&reports).map_err(|e| {
        diagnostic(
            "V_UNKNOWN",
            format!("requested verification did not pass: {e:?}"),
        )
    })?;
    Ok(json!(reports.iter().map(|r| json!({"analysis":r.analysis,"version":r.version,"status":format!("{:?}",r.status),"test_only":r.test_only})).collect::<Vec<_>>()))
}

fn execute(request: &Request, sources: &mut SourceMap) -> Result<Value> {
    if request.assemble {
        return assemble(request);
    }
    let registry = extend::Registry::default();
    let plan = registry
        .prepare(&request.verify, &request.target)
        .map_err(|e| diagnostic("I_UNSUPPORTED", format!("extension configuration: {e}")))?;
    let mut paths = Vec::new();
    for path in &request.sources {
        paths.push(
            fs::canonicalize(path)
                .map_err(|e| diagnostic("I_IO", format!("{}: {e}", path.display())))?,
        );
    }
    paths.sort();
    if paths.windows(2).any(|p| p[0] == p[1]) {
        return Err(diagnostic("I_USAGE", "duplicate input file"));
    }
    let mut asts = vec![];
    let mut inputs = vec![];
    for path in paths {
        let bytes = fs::read(&path).map_err(|e| diagnostic("I_IO", e.to_string()))?;
        if bytes.len() > u32::MAX as usize {
            return Err(diagnostic(
                "I_LIMIT",
                "source file exceeds span address space",
            ));
        }
        inputs.push(json!({"path":path,"sha256":format!("{:x}",Sha256::digest(&bytes))}));
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(error) => {
                let offset = error.utf8_error().valid_up_to();
                let file =
                    sources.add(path, String::from_utf8_lossy(error.as_bytes()).into_owned());
                return Err(Diagnostic::new(
                    "E_LEX",
                    Span::new(file, offset, offset),
                    "source is not UTF-8",
                ));
            }
        };
        let file = sources.add(path, text);
        asts.push(crate::syntax::parse(file, &sources.get(file).text)?);
    }
    let program = crate::check::check(&asts)?;
    let mut report = json!({"schema":1,"status":"ok","diagnostics":[],"compiler":env!("CARGO_PKG_VERSION"),"reference":crate::REFERENCE_VERSION,"target":request.target,"inputs":inputs,"functions":program.functions.len()});
    if !request.build {
        report["analyses"] = analysis_reports(
            &registry,
            &plan,
            sources,
            &program,
            &program.types,
            &ir::Module::default(),
            None,
        )?;
        if let Some(path) = &request.dump_ir {
            fs::write(path, format!("{program:#?}\n"))
                .map_err(|e| diagnostic("I_IO", e.to_string()))?;
        }
        return Ok(report);
    }
    let mut compilation = crate::lower::lower(&program)?;
    if let Some(name) = &request.test {
        add_test_adapter(&program, &mut compilation, name)?;
    }
    if let Some(path) = &request.dump_ir {
        fs::write(path, format!("HIR\n{program:#?}\nMIR\n{compilation:#?}\n"))
            .map_err(|e| diagnostic("I_IO", e.to_string()))?;
    }
    let mut emitted = crate::target::rv64::emit(&mut compilation.types, &compilation.module)?;
    add_trap_adapter(
        &program,
        request.trap_entry.as_deref(),
        &mut emitted.assembly,
    )?;
    let defined: BTreeSet<_> = compilation
        .module
        .functions
        .iter()
        .map(|f| &f.symbol)
        .chain(compilation.module.data.iter().map(|d| &d.symbol))
        .collect();
    report["required_symbols"] = json!(
        emitted
            .required_symbols
            .iter()
            .filter(|s| !defined.contains(s))
            .collect::<Vec<_>>()
    );
    report["frames"] = json!(emitted.frames.iter().map(|f| json!({"symbol":f.symbol,"local_bytes":f.local_bytes,"outgoing_bytes":f.outgoing_bytes,"alignment":f.alignment,"realignment_slack":f.realignment_slack,"transient_bytes":f.transient_bytes,"frame_bytes":f.frame_bytes})).collect::<Vec<_>>());
    let built = toolchain::build(&emitted.assembly, &emitted.linkage, &request.tools)?;
    report["linkage"] = serde_json::to_value(&emitted.linkage).expect("linkage serialization");
    let mut platform_inputs = vec![];
    for path in built
        .platform_inputs
        .iter()
        .chain(request.tools.objects.iter())
    {
        let bytes = fs::read(path).map_err(|e| diagnostic("I_IO", e.to_string()))?;
        platform_inputs.push(json!({"path":path,"sha256":format!("{:x}",Sha256::digest(&bytes))}));
    }
    report["build"] = serde_json::to_value(built).expect("serializable build report");
    report["platform_inputs"] = json!(platform_inputs);
    let bytes = fs::read(&request.tools.output).map_err(|e| diagnostic("I_IO", e.to_string()))?;
    report["output_sha256"] = json!(format!("{:x}", Sha256::digest(&bytes)));
    let contracts = if request.tools.platform.as_deref() == Some("qemu-virt-core-v1") {
        vec!["qemu-virt-core-v1: RV64IMA S-mode, bare addressing, 128 MiB RAM; trap handlers cannot sleep, schedule or enable SIE; nested faults terminate with panic".to_owned()]
    } else {
        vec!["External platform contracts must be supplied by the caller; no platform safety verification is implied".to_owned()]
    };
    let target = extend::TargetSnapshot::from_compilation(
        &compilation.types,
        &compilation.module,
        &emitted,
        (request.tools.emit == Emit::Elf).then_some(bytes.as_slice()),
        &contracts,
    )?;
    report["target_facts"] = json!({"frames":target.frames.len(),"abi_plans":target.abi_plans.len(),"linked_regions":target.linked_regions.len(),"final_image_available":target.linked_image.is_some(),"platform_contracts":target.platform_contracts});
    report["analyses"] = analysis_reports(
        &registry,
        &plan,
        sources,
        &program,
        &compilation.types,
        &compilation.module,
        Some(&target.view()),
    )?;
    Ok(report)
}

fn source_function<'a>(program: &'a hir::Program, name: &str) -> Result<&'a hir::Function> {
    let matches = program
        .functions
        .iter()
        .filter(|f| {
            f.name == name || (!name.contains("::") && f.name.rsplit("::").next() == Some(name))
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(diagnostic(
            "E_NAME",
            format!("entry {name} must select exactly one concrete function"),
        ));
    }
    Ok(matches[0])
}

fn add_test_adapter(
    program: &hir::Program,
    compilation: &mut ir::Compilation,
    name: &str,
) -> Result<()> {
    use crate::types::{Abi, DefId, FnType, Integer, TypeKind};
    let selected = source_function(program, name)?;
    if !selected.signature.params.is_empty()
        || selected.special.is_some()
        || !matches!(
            program.types.kind(selected.signature.result),
            TypeKind::Int(Integer::I32)
        )
    {
        return Err(Diagnostic::new(
            "E_TYPE",
            selected.span,
            "test entry must be an ordinary fn() -> i32",
        ));
    }
    let signature = FnType {
        params: vec![],
        result: selected.signature.result,
        abi: Abi::C,
        unsafe_: false,
        effects: Default::default(),
    };
    compilation.module.functions.push(ir::Function {
        definition: DefId(u32::MAX),
        symbol: "__biomiya_test_entry".into(),
        signature,
        parameters: vec![],
        slots: vec![ir::Slot {
            ty: selected.signature.result,
            name: None,
            span: selected.span,
            scope: ir::ScopeId(0),
        }],
        blocks: vec![ir::Block {
            instructions: vec![ir::Instruction {
                kind: ir::InstructionKind::Call {
                    target: ir::CallTarget::Direct(selected.symbol.clone()),
                    signature: selected.signature.clone(),
                    arguments: vec![],
                    result: Some(ir::SlotId(0)),
                },
                span: selected.span,
            }],
            terminator: ir::Terminator::Return(Some(ir::Operand::Slot(ir::SlotId(0)))),
            span: selected.span,
        }],
        entry: ir::BlockId(0),
        span: selected.span,
        special: None,
        exported: true,
        section: None,
    });
    Ok(())
}

fn add_trap_adapter(
    program: &hir::Program,
    requested: Option<&str>,
    assembly: &mut String,
) -> Result<()> {
    let entries = program
        .functions
        .iter()
        .filter(|f| f.special.as_deref() == Some("trap_entry"))
        .collect::<Vec<_>>();
    let chosen = match requested {
        Some(name) => {
            let f = source_function(program, name)?;
            if f.special.as_deref() != Some("trap_entry") {
                return Err(Diagnostic::new(
                    "E_CONTEXT",
                    f.span,
                    "selected trap entry is not @trap_entry",
                ));
            }
            Some(f)
        }
        None if entries.len() == 1 => Some(entries[0]),
        None if entries.is_empty() => None,
        None => {
            return Err(diagnostic(
                "I_USAGE",
                "multiple @trap_entry functions require --trap-entry",
            ));
        }
    };
    if let Some(f) = chosen {
        let quoted = crate::linkage::asm_symbol(&f.symbol);
        assembly.push_str(&format!("\n.section .text,\"ax\",@progbits\n.globl __biomiya_trap_entry\n.balign 4\n__biomiya_trap_entry:\n  la t0, {quoted}\n  jr t0\n"));
    }
    Ok(())
}

pub fn main_entry(args: Vec<OsString>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "biomiya check|build [OPTIONS] <explicit .vt files>\nbiomiya assemble [--linkage PATH] -o output.o input.s\n  --target rv64-core-v1\n  --diagnostics human|json\n  --emit asm|obj|elf -o PATH\n  --platform qemu-virt-core-v1 | --link-script PATH\n  --object PATH              additional target object/archive (repeatable)\n  --test NAME                generate conformance entry fn() -> i32\n  --trap-entry NAME          select a special hardware trap entry\n  --verify NAME[,NAME]       request available optional analyses\n  --dump-ir PATH --report PATH --keep-temporaries\n  --clang PATH --linker PATH --sysroot PATH"
        );
        return 0;
    }
    if args.len() == 1 && args[0] == "--version" {
        println!(
            "biomiya {} (reference {}, {})",
            env!("CARGO_PKG_VERSION"),
            crate::REFERENCE_VERSION,
            crate::TARGET
        );
        return 0;
    }
    let wants_json = args
        .windows(2)
        .any(|v| v[0] == "--diagnostics" && v[1] == "json")
        || args.iter().any(|a| a == "--diagnostics=json");
    let mut sources = SourceMap::default();
    let mut report_path = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let request = parse(&args)?;
        report_path = request.report.clone();
        execute(&request, &mut sources)
    }))
    .unwrap_or_else(|_| Err(diagnostic("I_BUG", "internal compiler failure")));
    let (report, exit) = match result {
        Ok(value) => (value, 0),
        Err(d) => {
            if !wants_json {
                if let Some(file) = sources.files().get(d.span.file.0 as usize) {
                    let (line, column) = sources.line_column(d.span);
                    eprintln!("{}:{line}:{column}: {d}", file.path.display());
                } else {
                    eprintln!("{d}");
                }
                for note in &d.notes {
                    eprintln!("  note: {}", note.message);
                }
            }
            let exit = if d.code.starts_with("I_") { 2 } else { 1 };
            let files = sources
                .files()
                .iter()
                .map(|f| json!({"id":f.id,"path":f.path}))
                .collect::<Vec<_>>();
            (
                json!({"schema":1,"status":"error","diagnostics":[d],"sources":files,"reference":crate::REFERENCE_VERSION,"target":crate::TARGET}),
                exit,
            )
        }
    };
    if let Some(path) = report_path
        && let Err(e) = fs::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&report).unwrap()),
        )
    {
        eprintln!("cannot write report {}: {e}", path.display());
        return 2;
    }
    if wants_json {
        println!("{}", serde_json::to_string(&report).unwrap());
    } else if exit == 0 {
        println!(
            "{}",
            if report.get("build").is_some() {
                "build succeeded"
            } else {
                "Core checks passed"
            }
        );
    }
    exit
}
