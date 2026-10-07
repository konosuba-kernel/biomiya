//! Read-only, version-local optional analysis interfaces. No production validators
//! are implemented here. Test probes exercise transport and gating only.
use crate::{
    ir,
    source::{SourceMap, Span},
    types::{DefId, TyId, TypeContext},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    Semantic,
    Target,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    NotEnabled,
    Verified,
    Violation,
    Unknown,
}
#[derive(Clone, Debug)]
pub struct ExtensionDescriptor {
    pub name: String,
    pub version: String,
    pub dependencies: Vec<String>,
    pub supported_targets: BTreeSet<String>,
    pub phases: BTreeSet<Phase>,
    /// Test-only analyses must never be presented as real verification.
    pub test_only: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SessionId(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SemanticId {
    Definition(DefId),
    Type(TyId),
    Function(u32),
    Instruction {
        function: u32,
        block: u32,
        instruction: u32,
    },
}
#[derive(Clone, Debug)]
pub struct Fact {
    pub subject: SemanticId,
    pub key: String,
    pub value: String,
    pub source: Option<Span>,
}
#[derive(Clone, Debug)]
pub struct Scope {
    pub description: String,
    pub objects: Vec<SemanticId>,
    pub external_contracts: Vec<String>,
    pub unresolved_calls: Vec<String>,
}
impl Default for Scope {
    fn default() -> Self {
        Self {
            description: "compilation".into(),
            objects: vec![],
            external_contracts: vec![],
            unresolved_calls: vec![],
        }
    }
}
#[derive(Clone, Debug)]
pub struct AnalysisDiagnostic {
    pub message: String,
    pub source: Option<Span>,
}
#[derive(Clone, Debug)]
pub struct AnalysisReport {
    pub session: SessionId,
    pub analysis: String,
    pub version: String,
    pub test_only: bool,
    pub scope: Scope,
    pub status: Status,
    pub diagnostics: Vec<AnalysisDiagnostic>,
    pub assumptions: Vec<String>,
    pub facts: Vec<Fact>,
}
impl AnalysisReport {
    pub fn new(session: SessionId, descriptor: &ExtensionDescriptor, status: Status) -> Self {
        Self {
            session,
            analysis: descriptor.name.clone(),
            version: descriptor.version.clone(),
            test_only: descriptor.test_only,
            scope: Scope::default(),
            status,
            diagnostics: vec![],
            assumptions: vec![],
            facts: vec![],
        }
    }
}
/// Optional typed/source metadata is borrowed; observers cannot mutate it.
/// The compiler can extend this version-local record as HIR becomes richer.
#[derive(Clone, Debug)]
pub struct InstanceOrigin {
    pub definition: DefId,
    pub arguments: Vec<TyId>,
    pub source: Span,
}
pub struct SemanticView<'a> {
    pub session: SessionId,
    pub types: &'a TypeContext,
    pub mir: &'a ir::Module,
    pub sources: Option<&'a SourceMap>,
    pub instances: &'a [InstanceOrigin],
    /// Typed HIR retains source semantics alongside MIR.
    pub frontend: Option<&'a crate::hir::Program>,
    pub declarations: Option<&'a crate::resolve::Resolver>,
}
impl<'a> SemanticView<'a> {
    pub fn new(session: SessionId, types: &'a TypeContext, mir: &'a ir::Module) -> Self {
        Self {
            session,
            types,
            mir,
            sources: None,
            instances: &[],
            frontend: None,
            declarations: None,
        }
    }
    pub fn events(&self) -> impl Iterator<Item = (SemanticId, &'a ir::SemanticEvent, Span)> + '_ {
        self.mir
            .functions
            .iter()
            .enumerate()
            .flat_map(|(f, function)| {
                function
                    .blocks
                    .iter()
                    .enumerate()
                    .flat_map(move |(b, block)| {
                        block
                            .instructions
                            .iter()
                            .enumerate()
                            .filter_map(move |(i, instruction)| match &instruction.kind {
                                ir::InstructionKind::Event(event) => Some((
                                    SemanticId::Instruction {
                                        function: f as u32,
                                        block: b as u32,
                                        instruction: i as u32,
                                    },
                                    event,
                                    instruction.span,
                                )),
                                _ => None,
                            })
                    })
            })
    }
}
#[derive(Clone, Debug)]
pub struct MachineCall {
    pub target: Option<String>,
    pub signature: crate::types::FnType,
    pub abi: crate::abi::CallPlan,
    pub source: Span,
}
#[derive(Clone, Debug)]
pub struct AsmClobbers {
    pub source: Span,
    pub registers: Vec<String>,
    pub may_block: bool,
}
#[derive(Clone, Debug)]
pub struct AbiFact {
    pub symbol: String,
    pub signature: crate::types::FnType,
    pub plan: crate::abi::CallPlan,
}
#[derive(Clone, Debug)]
pub struct MachineFrame {
    pub symbol: String,
    pub bytes: u64,
    pub local_bytes: u64,
    pub realignment_slack: u64,
    pub transient_bytes: u64,
    pub alignment: u64,
    pub outgoing_bytes: u64,
    pub saved_registers: Vec<String>,
    pub callees: Vec<String>,
    pub unresolved_indirect_calls: bool,
    pub calls: Vec<MachineCall>,
    pub asm_clobbers: Vec<AsmClobbers>,
}
#[derive(Clone, Debug)]
pub struct LinkedRegion {
    pub name: String,
    pub name_bytes: Vec<u8>,
    pub address: u64,
    pub size: u64,
    pub writable: bool,
    pub executable: bool,
}
pub struct TargetView<'a> {
    pub target: &'a str,
    pub frames: &'a [MachineFrame],
    pub linked_regions: &'a [LinkedRegion],
    pub linked_image: Option<&'a [u8]>,
    pub abi_plans: &'a [AbiFact],
    pub platform_contracts: &'a [String],
}
pub struct AnalysisInput<'a> {
    pub semantic: &'a SemanticView<'a>,
    pub target: Option<&'a TargetView<'a>>,
    /// Only requested dependencies are present, with their real status and scope.
    pub dependencies: BTreeMap<&'a str, &'a AnalysisReport>,
}
pub trait Analysis: Send + Sync {
    fn descriptor(&self) -> ExtensionDescriptor;
    fn analyze(&self, input: AnalysisInput<'_>) -> AnalysisReport;
}
struct Registration {
    descriptor: ExtensionDescriptor,
    implementation: Option<Box<dyn Analysis>>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigurationError {
    UnknownAnalysis(String),
    Unavailable(String),
    Duplicate(String),
    MissingDependency {
        analysis: String,
        dependency: String,
    },
    DependencyCycle(String),
    UnsupportedTarget {
        analysis: String,
        target: String,
    },
    InvalidDescriptor(String),
}
impl std::fmt::Display for ConfigurationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ConfigurationError {}
/// Prepared before parsing source files. Registry order cannot affect execution.
#[derive(Clone, Debug)]
pub struct Plan {
    requested: Vec<String>,
    target: String,
}
pub struct Registry {
    registrations: BTreeMap<String, Registration>,
}
impl Default for Registry {
    fn default() -> Self {
        let mut r = Self::empty();
        for name in ["ownership", "borrow", "blocking", "concurrency", "kernel"] {
            let dependencies = match name {
                "borrow" => vec!["ownership".into()],
                "concurrency" => vec!["ownership".into(), "borrow".into()],
                _ => vec![],
            };
            let descriptor = ExtensionDescriptor {
                name: name.into(),
                version: env!("CARGO_PKG_VERSION").into(),
                dependencies,
                supported_targets: BTreeSet::from([crate::TARGET.into()]),
                phases: BTreeSet::from([if name == "kernel" {
                    Phase::Target
                } else {
                    Phase::Semantic
                }]),
                test_only: false,
            };
            r.registrations.insert(
                name.into(),
                Registration {
                    descriptor,
                    implementation: None,
                },
            );
        }
        r
    }
}
impl Registry {
    pub fn empty() -> Self {
        Self {
            registrations: BTreeMap::new(),
        }
    }
    pub fn register(&mut self, analysis: Box<dyn Analysis>) -> Result<(), ConfigurationError> {
        let d = analysis.descriptor();
        if d.name.is_empty()
            || d.version.is_empty()
            || d.phases.is_empty()
            || d.supported_targets.is_empty()
        {
            return Err(ConfigurationError::InvalidDescriptor(d.name));
        }
        if self.registrations.contains_key(&d.name) {
            return Err(ConfigurationError::Duplicate(d.name));
        }
        self.registrations.insert(
            d.name.clone(),
            Registration {
                descriptor: d,
                implementation: Some(analysis),
            },
        );
        Ok(())
    }
    pub fn descriptors(&self) -> impl Iterator<Item = &ExtensionDescriptor> {
        self.registrations.values().map(|r| &r.descriptor)
    }
    pub fn prepare(&self, names: &[String], target: &str) -> Result<Plan, ConfigurationError> {
        let mut requested = BTreeSet::new();
        for name in names {
            if !requested.insert(name.clone()) {
                return Err(ConfigurationError::Duplicate(name.clone()));
            }
            let r = self
                .registrations
                .get(name)
                .ok_or_else(|| ConfigurationError::UnknownAnalysis(name.clone()))?;
            if r.implementation.is_none() {
                return Err(ConfigurationError::Unavailable(name.clone()));
            }
            if !r.descriptor.supported_targets.contains(target) {
                return Err(ConfigurationError::UnsupportedTarget {
                    analysis: name.clone(),
                    target: target.into(),
                });
            }
        }
        for name in &requested {
            for dependency in &self.registrations[name].descriptor.dependencies {
                if !requested.contains(dependency) {
                    return Err(ConfigurationError::MissingDependency {
                        analysis: name.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
        }
        let mut ordered = vec![];
        let mut active = BTreeSet::new();
        let mut done = BTreeSet::new();
        fn visit(
            name: &str,
            r: &Registry,
            active: &mut BTreeSet<String>,
            done: &mut BTreeSet<String>,
            out: &mut Vec<String>,
        ) -> Result<(), ConfigurationError> {
            if done.contains(name) {
                return Ok(());
            }
            if !active.insert(name.into()) {
                return Err(ConfigurationError::DependencyCycle(name.into()));
            }
            for dep in &r.registrations[name].descriptor.dependencies {
                visit(dep, r, active, done, out)?;
            }
            active.remove(name);
            done.insert(name.into());
            out.push(name.into());
            Ok(())
        }
        for name in requested {
            visit(&name, self, &mut active, &mut done, &mut ordered)?
        }
        Ok(Plan {
            requested: ordered,
            target: target.into(),
        })
    }
    pub fn run(
        &self,
        plan: &Plan,
        semantic: &SemanticView<'_>,
        target: Option<&TargetView<'_>>,
    ) -> Vec<AnalysisReport> {
        let mut reports: BTreeMap<String, AnalysisReport> = BTreeMap::new();
        for name in &plan.requested {
            let r = &self.registrations[name];
            let mut report = if r.descriptor.phases.contains(&Phase::Target)
                && target.is_none_or(|t| t.linked_image.is_none() || t.target != plan.target)
            {
                let mut report =
                    AnalysisReport::new(semantic.session, &r.descriptor, Status::Unknown);
                report.diagnostics.push(AnalysisDiagnostic {
                    message: "required final target facts are unavailable".into(),
                    source: None,
                });
                report
            } else {
                let dependencies = r
                    .descriptor
                    .dependencies
                    .iter()
                    .map(|d| (d.as_str(), &reports[d]))
                    .collect();
                r.implementation
                    .as_ref()
                    .expect("prepared implementation")
                    .analyze(AnalysisInput {
                        semantic,
                        target,
                        dependencies,
                    })
            };
            // Observers cannot forge report provenance or reuse another session's facts.
            if report.session != semantic.session
                || report.analysis != r.descriptor.name
                || report.version != r.descriptor.version
                || report.status == Status::NotEnabled
            {
                report = AnalysisReport::new(semantic.session, &r.descriptor, Status::Unknown);
                report.diagnostics.push(AnalysisDiagnostic {
                    message: "analysis returned invalid report identity/status".into(),
                    source: None,
                });
            }
            report.test_only = r.descriptor.test_only;
            reports.insert(name.clone(), report);
        }
        for (name, r) in &self.registrations {
            if !reports.contains_key(name) {
                reports.insert(
                    name.clone(),
                    AnalysisReport::new(semantic.session, &r.descriptor, Status::NotEnabled),
                );
            }
        }
        reports.into_values().collect()
    }
}
/// Strict production acceptance excludes test probes and every non-Verified result.
pub fn strict_gate(reports: &[AnalysisReport]) -> Result<(), Vec<String>> {
    let failures: Vec<_> = reports
        .iter()
        .filter(|r| r.status != Status::NotEnabled && (r.status != Status::Verified || r.test_only))
        .map(|r| r.analysis.clone())
        .collect();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}
/// Owned target facts derived from the actual emitter and final ELF. This is
/// observation, not a stack bound proof or verification of external code.
#[derive(Clone, Debug)]
pub struct TargetSnapshot {
    pub target: String,
    pub frames: Vec<MachineFrame>,
    pub abi_plans: Vec<AbiFact>,
    pub linked_regions: Vec<LinkedRegion>,
    pub linked_image: Option<Vec<u8>>,
    pub platform_contracts: Vec<String>,
    pub required_symbols: Vec<String>,
}
impl TargetSnapshot {
    pub fn from_compilation(
        types: &TypeContext,
        module: &ir::Module,
        emission: &crate::target::rv64::Emission,
        linked_image: Option<&[u8]>,
        platform_contracts: &[String],
    ) -> crate::diagnostic::Result<Self> {
        let mut types = types.clone();
        let mut frames = vec![];
        let mut abi_plans = vec![];
        for fact in &emission.frames {
            let function = module
                .functions
                .iter()
                .find(|f| f.symbol == fact.symbol)
                .ok_or_else(|| {
                    snapshot_error("emission frame has no corresponding MIR function")
                })?;
            abi_plans.push(AbiFact {
                symbol: function.symbol.clone(),
                signature: function.signature.clone(),
                plan: crate::abi::classify(&mut types, &function.signature)?,
            });
            let mut calls = vec![];
            let mut callees = BTreeSet::new();
            let mut asm_clobbers = vec![];
            let mut unresolved = false;
            for block in &function.blocks {
                for instruction in &block.instructions {
                    match &instruction.kind {
                        ir::InstructionKind::Call {
                            target, signature, ..
                        } => {
                            let target = match target {
                                ir::CallTarget::Direct(s) => {
                                    callees.insert(s.clone());
                                    Some(s.clone())
                                }
                                ir::CallTarget::Indirect(_) => {
                                    unresolved = true;
                                    None
                                }
                            };
                            calls.push(MachineCall {
                                target,
                                signature: signature.clone(),
                                abi: crate::abi::classify(&mut types, signature)?,
                                source: instruction.span,
                            });
                        }
                        ir::InstructionKind::Asm {
                            outputs,
                            clobbers,
                            may_block,
                            ..
                        } => {
                            let mut registers: BTreeSet<_> = clobbers.iter().cloned().collect();
                            registers.extend(outputs.iter().map(|o| o.register.clone()));
                            asm_clobbers.push(AsmClobbers {
                                source: instruction.span,
                                registers: registers.into_iter().collect(),
                                may_block: *may_block,
                            });
                        }
                        _ => {}
                    }
                }
                match &block.terminator {
                    ir::Terminator::Panic(_) => {
                        callees.insert("__biomiya_panic".into());
                    }
                    ir::Terminator::SpecialTransfer { handler, .. } => {
                        callees.insert("__biomiya_dispatch".into());
                        callees.insert(handler.clone());
                    }
                    _ => {}
                }
            }
            // Arithmetic checks may add runtime panic calls not represented as
            // a MIR terminator. Decode emitter call lines inside this function.
            let mut inside = false;
            for line in emission.assembly.lines() {
                if line.starts_with(".type ") && line.ends_with("@function") {
                    inside = line.starts_with(&format!(
                        ".type {},",
                        crate::linkage::asm_symbol(&function.symbol)
                    ));
                }
                if inside && let Some(target) = line.trim().strip_prefix("call ") {
                    let alias = target.trim_matches('"');
                    let name = emission
                        .linkage
                        .symbols
                        .iter()
                        .find(|mapping| mapping.alias == alias)
                        .map(|mapping| crate::linkage::encode_name(&mapping.bytes))
                        .unwrap_or_else(|| alias.to_string());
                    callees.insert(name);
                }
            }
            frames.push(MachineFrame {
                symbol: fact.symbol.clone(),
                bytes: fact.frame_bytes,
                local_bytes: fact.local_bytes,
                realignment_slack: fact.realignment_slack,
                transient_bytes: fact.transient_bytes,
                alignment: fact.alignment,
                outgoing_bytes: fact.outgoing_bytes,
                saved_registers: vec!["ra".into(), "s0".into()],
                callees: callees.into_iter().collect(),
                unresolved_indirect_calls: unresolved,
                calls,
                asm_clobbers,
            });
        }
        let linked_regions = linked_image
            .map(elf_regions)
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            target: crate::TARGET.into(),
            frames,
            abi_plans,
            linked_regions,
            linked_image: linked_image.map(<[u8]>::to_vec),
            platform_contracts: platform_contracts.to_vec(),
            required_symbols: emission.required_symbols.clone(),
        })
    }
    pub fn view(&self) -> TargetView<'_> {
        TargetView {
            target: &self.target,
            frames: &self.frames,
            linked_regions: &self.linked_regions,
            linked_image: self.linked_image.as_deref(),
            abi_plans: &self.abi_plans,
            platform_contracts: &self.platform_contracts,
        }
    }
}
fn snapshot_error(message: impl Into<String>) -> crate::diagnostic::Diagnostic {
    crate::diagnostic::Diagnostic::new("I_ELF", Span::default(), message)
}
fn elf_regions(image: &[u8]) -> crate::diagnostic::Result<Vec<LinkedRegion>> {
    // The public snapshot API accepts bytes directly, so it shares final-image
    // ISA validation with the linker before exposing target facts.
    crate::toolchain::validate_elf(image)?;
    fn bytes(image: &[u8], offset: u64, size: u64) -> crate::diagnostic::Result<&[u8]> {
        let end = offset
            .checked_add(size)
            .ok_or_else(|| snapshot_error("ELF range overflows"))?;
        let start =
            usize::try_from(offset).map_err(|_| snapshot_error("ELF offset exceeds host size"))?;
        let end = usize::try_from(end).map_err(|_| snapshot_error("ELF end exceeds host size"))?;
        image
            .get(start..end)
            .ok_or_else(|| snapshot_error("ELF range exceeds file bounds"))
    }
    fn u16_(b: &[u8], o: usize) -> u16 {
        u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
    }
    fn u32_(b: &[u8], o: usize) -> u32 {
        u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
    }
    fn u64_(b: &[u8], o: usize) -> u64 {
        u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
    }
    let h = bytes(image, 0, 64)?;
    if &h[..4] != b"\x7fELF"
        || h[4] != 2
        || h[5] != 1
        || h[6] != 1
        || u16_(h, 16) != 2
        || u32_(h, 48) != 0
        || u16_(h, 18) != 243
        || u16_(h, 52) != 64
    {
        return Err(snapshot_error("expected ELF64 little-endian RISC-V image"));
    }
    let offset = u64_(h, 40);
    let size = u16_(h, 58) as u64;
    let count = u16_(h, 60) as u64;
    let strings = u16_(h, 62) as u64;
    if size != 64 || count == 0 || strings >= count {
        return Err(snapshot_error(
            "ELF section table is missing or unsupported",
        ));
    }
    let table = bytes(
        image,
        offset,
        size.checked_mul(count)
            .ok_or_else(|| snapshot_error("ELF section table size overflows"))?,
    )?;
    let string_header = &table[(strings * 64) as usize..((strings + 1) * 64) as usize];
    if u32_(string_header, 4) != 3 {
        return Err(snapshot_error("ELF section names require a string table"));
    }
    let names = bytes(image, u64_(string_header, 24), u64_(string_header, 32))?;
    let mut regions = vec![];
    for header in table.chunks_exact(64) {
        let flags = u64_(header, 8);
        if flags & 2 == 0 {
            continue;
        }
        let address = u64_(header, 16);
        let length = u64_(header, 32);
        let alignment = u64_(header, 48);
        if alignment > 1 && (!alignment.is_power_of_two() || !address.is_multiple_of(alignment)) {
            return Err(snapshot_error("invalid ELF allocated section alignment"));
        }
        address
            .checked_add(length)
            .ok_or_else(|| snapshot_error("ELF allocated region address overflows"))?;
        if u32_(header, 4) != 8 {
            bytes(image, u64_(header, 24), length)?;
        }
        let name_offset = u32_(header, 0) as usize;
        let rest = names
            .get(name_offset..)
            .ok_or_else(|| snapshot_error("ELF section name exceeds string table"))?;
        let end = rest
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(|| snapshot_error("unterminated ELF section name"))?;
        let name_bytes = rest[..end].to_vec();
        let name = crate::linkage::encode_name(&name_bytes);
        regions.push(LinkedRegion {
            name,
            name_bytes,
            address,
            size: length,
            writable: flags & 1 != 0,
            executable: flags & 4 != 0,
        });
    }
    Ok(regions)
}
