//! Interface probes only: these do not implement Ownership/Borrow/Kernel.
use biomiya::{TARGET, extend::*, ir, types::TypeContext};
use std::collections::BTreeSet;
struct Probe {
    name: &'static str,
    deps: Vec<String>,
    phase: Phase,
    status: Status,
    test_only: bool,
}
impl Analysis for Probe {
    fn descriptor(&self) -> ExtensionDescriptor {
        ExtensionDescriptor {
            name: self.name.into(),
            version: "test-1".into(),
            dependencies: self.deps.clone(),
            supported_targets: BTreeSet::from([TARGET.into()]),
            phases: BTreeSet::from([self.phase]),
            test_only: self.test_only,
        }
    }
    fn analyze(&self, input: AnalysisInput<'_>) -> AnalysisReport {
        let mut r = AnalysisReport::new(input.semantic.session, &self.descriptor(), self.status);
        r.assumptions.push("interface test only".into());
        for (name, dep) in input.dependencies {
            r.facts.push(Fact {
                subject: SemanticId::Function(0),
                key: name.into(),
                value: format!("{:?}", dep.status),
                source: None,
            });
        }
        r
    }
}
fn probe(name: &'static str, status: Status) -> Box<dyn Analysis> {
    Box::new(Probe {
        name,
        deps: vec![],
        phase: Phase::Semantic,
        status,
        test_only: false,
    })
}
#[test]
fn real_validators_are_explicitly_unavailable() {
    let r = Registry::default();
    assert_eq!(
        r.prepare(&["ownership".into()], TARGET).unwrap_err(),
        ConfigurationError::Unavailable("ownership".into())
    );
    assert!(r.prepare(&[], TARGET).is_ok());
    let types = TypeContext::new();
    let module = ir::Module::default();
    let view = SemanticView::new(SessionId(1), &types, &module);
    let reports = r.run(&r.prepare(&[], TARGET).unwrap(), &view, None);
    assert!(reports.iter().all(|r| r.status == Status::NotEnabled));
    assert!(strict_gate(&reports).is_ok());
}
#[test]
fn configuration_dependencies_and_cycles() {
    let mut r = Registry::empty();
    r.register(probe("a", Status::Verified)).unwrap();
    r.register(Box::new(Probe {
        name: "b",
        deps: vec!["a".into()],
        phase: Phase::Semantic,
        status: Status::Verified,
        test_only: false,
    }))
    .unwrap();
    assert!(matches!(
        r.prepare(&["b".into()], TARGET),
        Err(ConfigurationError::MissingDependency { .. })
    ));
    assert!(matches!(
        r.prepare(&["a".into(), "a".into()], TARGET),
        Err(ConfigurationError::Duplicate(_))
    ));
    assert!(matches!(
        r.prepare(&["a".into()], "wrong"),
        Err(ConfigurationError::UnsupportedTarget { .. })
    ));
    assert!(matches!(
        r.prepare(&["unknown".into()], TARGET),
        Err(ConfigurationError::UnknownAnalysis(_))
    ));
    let mut c = Registry::empty();
    for (name, dep) in [("a", "b"), ("b", "a")] {
        c.register(Box::new(Probe {
            name,
            deps: vec![dep.into()],
            phase: Phase::Semantic,
            status: Status::Verified,
            test_only: false,
        }))
        .unwrap();
    }
    assert!(matches!(
        c.prepare(&["a".into(), "b".into()], TARGET),
        Err(ConfigurationError::DependencyCycle(_))
    ));
}
#[test]
fn four_states_and_strict_gate() {
    let types = TypeContext::new();
    let module = ir::Module::default();
    let view = SemanticView::new(SessionId(2), &types, &module);
    for status in [Status::Verified, Status::Violation, Status::Unknown] {
        let mut r = Registry::empty();
        r.register(probe("probe", status)).unwrap();
        r.register(probe("off", Status::Verified)).unwrap();
        let reports = r.run(&r.prepare(&["probe".into()], TARGET).unwrap(), &view, None);
        assert_eq!(
            reports
                .iter()
                .find(|r| r.analysis == "probe")
                .unwrap()
                .status,
            status
        );
        assert_eq!(
            reports.iter().find(|r| r.analysis == "off").unwrap().status,
            Status::NotEnabled
        );
        assert_eq!(strict_gate(&reports).is_ok(), status == Status::Verified);
    }
}
#[test]
fn required_target_facts_remain_unknown_at_check() {
    let mut r = Registry::empty();
    r.register(Box::new(Probe {
        name: "machine",
        deps: vec![],
        phase: Phase::Target,
        status: Status::Verified,
        test_only: false,
    }))
    .unwrap();
    let types = TypeContext::new();
    let module = ir::Module::default();
    let view = SemanticView::new(SessionId(3), &types, &module);
    let plan = r.prepare(&["machine".into()], TARGET).unwrap();
    let reports = r.run(&plan, &view, None);
    assert_eq!(reports[0].status, Status::Unknown);
    assert!(strict_gate(&reports).is_err());
    let target = TargetView {
        target: TARGET,
        frames: &[],
        linked_regions: &[],
        linked_image: Some(b"ELF"),
        abi_plans: &[],
        platform_contracts: &[],
    };
    assert_eq!(
        r.run(&plan, &view, Some(&target))[0].status,
        Status::Verified
    );
}
#[test]
fn probes_cannot_claim_production_verification() {
    let mut r = Registry::empty();
    r.register(Box::new(Probe {
        name: "test",
        deps: vec![],
        phase: Phase::Semantic,
        status: Status::Verified,
        test_only: true,
    }))
    .unwrap();
    let types = TypeContext::new();
    let module = ir::Module::default();
    let view = SemanticView::new(SessionId(4), &types, &module);
    let reports = r.run(&r.prepare(&["test".into()], TARGET).unwrap(), &view, None);
    assert!(reports[0].test_only);
    assert!(strict_gate(&reports).is_err());
}
#[test]
fn dependency_reports_preserve_provenance() {
    let mut r = Registry::empty();
    r.register(probe("a", Status::Unknown)).unwrap();
    r.register(Box::new(Probe {
        name: "b",
        deps: vec!["a".into()],
        phase: Phase::Semantic,
        status: Status::Verified,
        test_only: false,
    }))
    .unwrap();
    let types = TypeContext::new();
    let module = ir::Module::default();
    let view = SemanticView::new(SessionId(5), &types, &module);
    let reports = r.run(
        &r.prepare(&["b".into(), "a".into()], TARGET).unwrap(),
        &view,
        None,
    );
    assert_eq!(reports[1].facts[0].value, "Unknown");
    assert!(strict_gate(&reports).is_err());
}
#[test]
fn read_only_semantic_events_and_machine_metadata_are_reachable() {
    use biomiya::{
        ir::*,
        source::{FileId, SourceMap, Span},
        types::{Abi, DefId, FnType, TypeKind},
    };
    let mut types = TypeContext::new();
    let unit = types.intern(TypeKind::Unit);
    let span = Span::new(FileId(0), 0, 5);
    let place = Place {
        base: PlaceBase::Slot(SlotId(0)),
        offset: 0,
        ty: unit,
        align: 1,
    };
    let events = vec![
        SemanticEvent::Defer {
            source: 7,
            scope: ScopeId(0),
            registration: true,
        },
        SemanticEvent::Defer {
            source: 7,
            scope: ScopeId(0),
            registration: false,
        },
        SemanticEvent::RawAddress {
            place: place.clone(),
            result: SlotId(0),
        },
        SemanticEvent::Borrow {
            place,
            mutable: true,
            reborrow: true,
            result: SlotId(0),
        },
        SemanticEvent::Conversion {
            from: unit,
            to: unit,
            result: SlotId(0),
        },
        SemanticEvent::SplitSlice {
            input: Operand::Slot(SlotId(0)),
            result: SlotId(0),
        },
    ];
    let module = Module {
        functions: vec![Function {
            definition: DefId(0),
            symbol: "probe".into(),
            signature: FnType {
                params: vec![],
                result: unit,
                abi: Abi::Biomiya,
                unsafe_: false,
                effects: BTreeSet::from(["may_block".into()]),
            },
            parameters: vec![],
            slots: vec![Slot {
                ty: unit,
                name: None,
                span,
                scope: ScopeId(0),
            }],
            blocks: vec![Block {
                instructions: events
                    .into_iter()
                    .map(|event| Instruction {
                        kind: InstructionKind::Event(event),
                        span,
                    })
                    .collect(),
                terminator: Terminator::Return(None),
                span,
            }],
            entry: BlockId(0),
            span,
            special: None,
            exported: false,
            section: None,
        }],
        ..Module::default()
    };
    let mut sources = SourceMap::default();
    sources.add("probe.vt", "probe".into());
    let origins = [InstanceOrigin {
        definition: DefId(0),
        arguments: vec![unit],
        source: span,
    }];
    let mut view = SemanticView::new(SessionId(7), &types, &module);
    view.sources = Some(&sources);
    view.instances = &origins;
    let captured: Vec<_> = view.events().collect();
    assert_eq!(captured.len(), 6);
    assert!(captured.iter().all(|(_, _, s)| *s == span));
    assert!(matches!(captured[2].1, SemanticEvent::RawAddress { .. }));
    assert_eq!(view.instances[0].arguments, [unit]);
    assert_eq!(view.sources.unwrap().get(FileId(0)).text, "probe");
    assert!(
        view.mir.functions[0]
            .signature
            .effects
            .contains("may_block")
    );
    let frames = [MachineFrame {
        symbol: "probe".into(),
        bytes: 64,
        local_bytes: 32,
        realignment_slack: 0,
        transient_bytes: 0,
        alignment: 16,
        outgoing_bytes: 16,
        saved_registers: vec!["ra".into()],
        callees: vec!["external".into()],
        unresolved_indirect_calls: true,
        calls: vec![],
        asm_clobbers: vec![],
    }];
    let regions = [LinkedRegion {
        name: "text".into(),
        name_bytes: b"text".to_vec(),
        address: 0x80000000,
        size: 4096,
        writable: false,
        executable: true,
    }];
    let target = TargetView {
        target: TARGET,
        frames: &frames,
        linked_regions: &regions,
        linked_image: Some(b"image"),
        abi_plans: &[],
        platform_contracts: &[],
    };
    assert_eq!(target.frames[0].bytes, 64);
    assert!(target.frames[0].unresolved_indirect_calls);
    assert!(!target.linked_regions[0].writable);
}
#[test]
fn actual_slice_reborrow_retains_typed_source_and_result_in_view() {
    use biomiya::{
        check,
        ir::{Operand, SemanticEvent},
        lower,
        source::FileId,
        syntax,
        types::TypeKind,
    };
    let ast=syntax::parse(FileId(0),"pub fn test()->i32{var data=[3,4];let mutable=data[0..data.len];let readonly:[]const i32=mutable;readonly[0]}").unwrap();
    let hir = check::check(&[ast]).unwrap();
    let compilation = lower::lower(&hir).unwrap();
    let mut view = SemanticView::new(SessionId(8), &compilation.types, &compilation.module);
    view.frontend = Some(&hir);
    let reborrows: Vec<_> = view
        .events()
        .filter_map(|(_, event, _)| match event {
            SemanticEvent::ReborrowView {
                source,
                result,
                from,
                to,
            } => Some((source, *result, *from, *to)),
            _ => None,
        })
        .collect();
    assert_eq!(reborrows.len(), 1);
    let (source, result, from, to) = reborrows[0];
    assert!(matches!(source, Operand::Slot(_)));
    assert!(matches!(
        view.types.kind(from),
        TypeKind::Slice { const_: false, .. }
    ));
    assert!(matches!(
        view.types.kind(to),
        TypeKind::Slice { const_: true, .. }
    ));
    assert_eq!(view.mir.functions[0].slots[result.0 as usize].ty, to);
    assert!(view.frontend.is_some());
}
struct ActualTargetProbe;
impl Analysis for ActualTargetProbe {
    fn descriptor(&self) -> ExtensionDescriptor {
        ExtensionDescriptor {
            name: "test.actual-machine-view".into(),
            version: "1".into(),
            dependencies: vec![],
            supported_targets: BTreeSet::from([TARGET.into()]),
            phases: BTreeSet::from([Phase::Target]),
            test_only: true,
        }
    }
    fn analyze(&self, input: AnalysisInput<'_>) -> AnalysisReport {
        let target = input.target.unwrap();
        assert!(target.linked_image.is_some());
        assert!(
            target
                .frames
                .iter()
                .any(|f| f.local_bytes > 0 && f.bytes >= f.local_bytes && f.alignment >= 16)
        );
        assert!(
            target
                .linked_regions
                .iter()
                .any(|r| r.executable && !r.writable)
        );
        assert!(
            target
                .linked_regions
                .iter()
                .any(|r| r.writable && !r.executable)
        );
        assert!(target.abi_plans.iter().any(|p| p.symbol == "kernel_main"));
        assert!(input.semantic.declarations.unwrap().declarations.values().any(|r|matches!(&r.decl.kind,biomiya::syntax::DeclKind::Function(f) if !f.generics.is_empty())));
        AnalysisReport::new(input.semantic.session, &self.descriptor(), Status::Verified)
    }
}
#[test]
fn actual_source_emission_and_linked_elf_supply_machine_view() {
    let text = "fn untouched<T>(x:T)->T{x} fn callee(x:i32)->i32{x+1} @export(\"kernel_main\") extern abi(C) fn kernel_main()->(){let result=callee(6);unsafe{arch::asm(\"nop\",clobber(\"t0\"));}if result!=7{panic(\"bad\");}}";
    let ast = biomiya::syntax::parse(biomiya::source::FileId(0), text).unwrap();
    let hir = biomiya::check::check(&[ast]).unwrap();
    let mut compilation = biomiya::lower::lower(&hir).unwrap();
    let emission =
        biomiya::target::rv64::emit(&mut compilation.types, &compilation.module).unwrap();
    let directory = std::env::temp_dir().join(format!(
        "biomiya-extend-target-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let elf = directory.join("kernel.elf");
    biomiya::toolchain::build(
        &emission.assembly,
        &emission.linkage,
        &biomiya::toolchain::Options {
            clang: "clang".into(),
            linker: "ld.lld".into(),
            sysroot: biomiya::toolchain::default_sysroot(),
            platform: Some("qemu-virt-core-v1".into()),
            script: None,
            objects: vec![],
            output: elf.clone(),
            emit: biomiya::toolchain::Emit::Elf,
            test: false,
            keep_temporaries: false,
        },
    )
    .unwrap();
    let image = std::fs::read(&elf).unwrap();
    let contracts =
        vec!["bare supervisor platform; external runtime frames are trusted contracts".into()];
    let snapshot = TargetSnapshot::from_compilation(
        &compilation.types,
        &compilation.module,
        &emission,
        Some(&image),
        &contracts,
    )
    .unwrap();
    let kernel = snapshot
        .frames
        .iter()
        .find(|f| f.symbol == "kernel_main")
        .unwrap();
    assert_eq!(kernel.saved_registers, ["ra", "s0"]);
    let callee_symbol = &hir
        .functions
        .iter()
        .find(|f| f.name == "callee")
        .unwrap()
        .symbol;
    assert!(
        snapshot
            .frames
            .iter()
            .find(|f| &f.symbol == callee_symbol)
            .unwrap()
            .callees
            .iter()
            .any(|s| s == "__biomiya_panic")
    );
    assert!(!kernel.calls.is_empty());
    assert!(kernel.asm_clobbers.iter().any(|c| c.registers == ["t0"]));
    assert!(
        snapshot
            .frames
            .iter()
            .any(|f| f.callees.iter().any(|s| s == "__biomiya_panic"))
    );
    assert_eq!(snapshot.linked_image.as_deref(), Some(image.as_slice()));
    let mut semantic = SemanticView::new(SessionId(9), &compilation.types, &compilation.module);
    semantic.frontend = Some(&hir);
    semantic.declarations = hir.declarations.as_ref();
    let mut registry = Registry::empty();
    registry.register(Box::new(ActualTargetProbe)).unwrap();
    let plan = registry
        .prepare(&["test.actual-machine-view".into()], TARGET)
        .unwrap();
    let reports = registry.run(&plan, &semantic, Some(&snapshot.view()));
    assert_eq!(reports[0].status, Status::Verified);
    assert!(reports[0].test_only);
    assert!(strict_gate(&reports).is_err());
    for malformed in [&image[..8], &image[..63]] {
        assert!(
            TargetSnapshot::from_compilation(
                &compilation.types,
                &compilation.module,
                &emission,
                Some(malformed),
                &[]
            )
            .is_err()
        )
    }
    let mut malformed = image.clone();
    malformed[40..48].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(
        TargetSnapshot::from_compilation(
            &compilation.types,
            &compilation.module,
            &emission,
            Some(&malformed),
            &[]
        )
        .is_err()
    );
    let section_table = u64::from_le_bytes(image[40..48].try_into().unwrap()) as usize;
    let names_index = u16::from_le_bytes(image[62..64].try_into().unwrap()) as usize;
    let mut wrong_names = image.clone();
    wrong_names[section_table + names_index * 64 + 4..section_table + names_index * 64 + 8]
        .copy_from_slice(&1u32.to_le_bytes());
    let mut wrong_abi = image.clone();
    wrong_abi[48] = 1;
    for malformed in [&wrong_names, &wrong_abi] {
        assert!(
            TargetSnapshot::from_compilation(
                &compilation.types,
                &compilation.module,
                &emission,
                Some(malformed),
                &[]
            )
            .is_err()
        );
    }
    let output=std::process::Command::new("python3").arg("-c").arg("import sys,pathlib,json;sys.path.insert(0,'tools');import test_runtime as t;print(json.dumps(t.run(pathlib.Path(sys.argv[1]))))").arg(&elf).current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
    assert!(output.status.success());
    let executed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(executed["status"], "pass");
    assert_eq!(executed["value"], 0);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn actual_nested_effect_conversion_retains_both_semantic_types() {
    use biomiya::{check, ir::SemanticEvent, lower, source::FileId, syntax, types::TypeKind};
    let text = "fn source() effects{may_block}{} pub fn test()->i32{let original:(fn() effects{may_block},i32)=(source,1);let weakened:(fn(),i32)=original;(weakened.0)();7}";
    let ast = syntax::parse(FileId(0), text).unwrap();
    let hir = check::check(&[ast]).unwrap();
    let compilation = lower::lower(&hir).unwrap();
    let mut view = SemanticView::new(SessionId(10), &compilation.types, &compilation.module);
    view.frontend = Some(&hir);
    view.declarations = hir.declarations.as_ref();
    let conversions: Vec<_> = view
        .events()
        .filter_map(|(_, event, _)| match event {
            SemanticEvent::Conversion { from, to, result } => Some((*from, *to, *result)),
            _ => None,
        })
        .collect();
    let (from, to, result) = conversions
        .iter()
        .find(|(from, to, _)| {
            matches!(view.types.kind(*from), TypeKind::Tuple(_))
                && matches!(view.types.kind(*to), TypeKind::Tuple(_))
        })
        .copied()
        .unwrap();
    let TypeKind::Tuple(source_fields) = view.types.kind(from) else {
        unreachable!()
    };
    let TypeKind::Tuple(target_fields) = view.types.kind(to) else {
        unreachable!()
    };
    let TypeKind::Function(source) = view.types.kind(source_fields[0]) else {
        unreachable!()
    };
    let TypeKind::Function(target) = view.types.kind(target_fields[0]) else {
        unreachable!()
    };
    assert!(source.effects.contains("may_block"));
    assert!(target.effects.is_empty());
    let function = view
        .mir
        .functions
        .iter()
        .find(|f| {
            f.symbol
                == hir
                    .functions
                    .iter()
                    .find(|f| f.name == "test")
                    .unwrap()
                    .symbol
        })
        .unwrap();
    assert_eq!(function.slots[result.0 as usize].ty, to);
    assert!(
        view.frontend
            .unwrap()
            .functions
            .iter()
            .find(|f| f.name == "source")
            .unwrap()
            .signature
            .effects
            .contains("may_block")
    );
}

#[test]
fn checked_generic_lengths_survive_lowering_and_target_snapshot_clones() {
    let text = include_str!("audit/generic_dependent_instantiation.vt");
    let ast = biomiya::syntax::parse(biomiya::source::FileId(0), text).unwrap();
    let hir = biomiya::check::check(&[ast]).unwrap();
    let mut compilation = biomiya::lower::lower(&hir).unwrap();
    let emission =
        biomiya::target::rv64::emit(&mut compilation.types, &compilation.module).unwrap();
    let snapshot = TargetSnapshot::from_compilation(
        &compilation.types,
        &compilation.module,
        &emission,
        None,
        &[],
    )
    .unwrap();
    assert_eq!(snapshot.frames.len(), compilation.module.functions.len());
    assert_eq!(snapshot.abi_plans.len(), compilation.module.functions.len());
    assert!(snapshot.frames.iter().any(|frame| frame.local_bytes >= 258));
}
