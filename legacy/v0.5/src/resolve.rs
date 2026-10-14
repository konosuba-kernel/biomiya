//! Module/declaration collection and deterministic namespace resolution.
pub use crate::types::DefId;
use crate::{
    diagnostic::{Diagnostic, Result},
    source::Span,
    syntax::*,
};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Namespace {
    Module,
    Type,
    Value,
}
#[derive(Clone, Debug)]
pub struct DeclRecord {
    pub decl: Decl,
    pub module: Vec<String>,
    pub file: usize,
    pub impl_owner: Option<DefId>,
    pub impl_generics: Vec<GenericParam>,
    pub impl_target: Option<Type>,
}
#[derive(Clone, Debug)]
enum Symbol {
    Def(DefId),
    Module(Vec<String>),
    Builtin(Vec<String>),
}
#[derive(Clone, Debug)]
struct Binding {
    symbol: Symbol,
    public: bool,
    imported: bool,
    span: Span,
}
type Key = (Vec<String>, String, Namespace);
#[derive(Clone, Debug)]
pub struct Resolver {
    pub declarations: BTreeMap<DefId, DeclRecord>,
    pub methods: BTreeMap<(DefId, String), DefId>,
    symbols: BTreeMap<Key, Binding>,
    modules: BTreeSet<Vec<String>>,
}
impl Resolver {
    pub fn new(files: &[AstFile]) -> Result<Self> {
        let mut r = Self {
            declarations: BTreeMap::new(),
            methods: BTreeMap::new(),
            symbols: BTreeMap::new(),
            modules: BTreeSet::new(),
        };
        r.modules.insert(vec![]);
        r.add_builtin_registry();
        for file in files {
            if file
                .module
                .first()
                .is_some_and(|x| x == "core" || x == "arch" || x == "root")
            {
                let span = file
                    .declarations
                    .first()
                    .map_or(Span::default(), |d| d.span);
                return Err(Diagnostic::new("E_NAME", span, "reserved system module"));
            }
            for i in 1..=file.module.len() {
                r.modules.insert(file.module[..i].to_vec());
            }
        }
        let modules = r.modules.clone();
        for module in modules {
            if let Some(name) = module.last() {
                let parent = module[..module.len() - 1].to_vec();
                r.insert(
                    parent,
                    name.clone(),
                    Namespace::Module,
                    Binding {
                        symbol: Symbol::Module(module.clone()),
                        public: true,
                        imported: false,
                        span: Span::default(),
                    },
                )?;
            }
        }
        let mut pending = vec![];
        for (file, ast) in files.iter().enumerate() {
            for (index, decl) in ast.declarations.iter().enumerate() {
                pending.push((
                    ast.module.clone(),
                    decl.name.clone(),
                    file,
                    index,
                    decl.clone(),
                ));
            }
        }
        pending.sort_by(|a, b| (&a.0, &a.1, a.2, a.3).cmp(&(&b.0, &b.1, b.2, b.3)));
        for (module, _, file, _, decl) in pending {
            let id = DefId(r.declarations.len() as u32);
            r.declarations.insert(
                id,
                DeclRecord {
                    decl: decl.clone(),
                    module: module.clone(),
                    file,
                    impl_owner: None,
                    impl_generics: vec![],
                    impl_target: None,
                },
            );
            let binding = Binding {
                symbol: Symbol::Def(id),
                public: decl.public,
                imported: false,
                span: decl.span,
            };
            match &decl.kind {
                DeclKind::Alias(_) => {
                    r.insert(module.clone(), decl.name.clone(), Namespace::Type, binding)?
                }
                DeclKind::Struct { .. } => {
                    r.insert(
                        module.clone(),
                        decl.name.clone(),
                        Namespace::Type,
                        binding.clone(),
                    )?;
                    r.insert(module.clone(), decl.name.clone(), Namespace::Value, binding)?;
                }
                DeclKind::Enum { variants, .. } => {
                    r.insert(
                        module.clone(),
                        decl.name.clone(),
                        Namespace::Type,
                        binding.clone(),
                    )?;
                    let mut scope = module.clone();
                    scope.push(decl.name.clone());
                    for variant in variants {
                        r.insert(
                            scope.clone(),
                            variant.name.clone(),
                            Namespace::Value,
                            Binding {
                                span: variant.span,
                                ..binding.clone()
                            },
                        )?;
                    }
                }
                DeclKind::Function(_) | DeclKind::Const { .. } | DeclKind::Static { .. } => {
                    r.insert(module, decl.name.clone(), Namespace::Value, binding)?
                }
                DeclKind::Impl { .. } | DeclKind::Assert(_) => {}
            }
        }
        // Method records retain their implementation context so Self and the
        // implementation's type parameters are available to subsequent checks.
        let impls: Vec<_> = r
            .declarations
            .values()
            .filter(|d| matches!(d.decl.kind, DeclKind::Impl { .. }))
            .cloned()
            .collect();
        for record in impls {
            let DeclKind::Impl {
                generics,
                target,
                methods,
            } = record.decl.kind
            else {
                unreachable!()
            };
            let owner = if let TypeKind::Path(segments) = &target.kind {
                let path = segments.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
                r.lookup(&record.module, &path, Namespace::Type, target.span)?
            } else {
                None
            };
            for decl in methods {
                let id = DefId(r.declarations.len() as u32);
                if let Some(owner) = owner
                    && r.methods.insert((owner, decl.name.clone()), id).is_some()
                {
                    return Err(Diagnostic::new(
                        "E_NAME",
                        decl.span,
                        "duplicate inherent method",
                    ));
                }
                if let Some(owner) = owner {
                    let owner_record = &r.declarations[&owner];
                    let mut scope = owner_record.module.clone();
                    scope.push(owner_record.decl.name.clone());
                    r.insert(
                        scope,
                        decl.name.clone(),
                        Namespace::Value,
                        Binding {
                            symbol: Symbol::Def(id),
                            public: decl.public,
                            imported: false,
                            span: decl.span,
                        },
                    )?;
                }
                r.declarations.insert(
                    id,
                    DeclRecord {
                        decl,
                        module: record.module.clone(),
                        file: record.file,
                        impl_owner: owner,
                        impl_generics: generics.clone(),
                        impl_target: Some(target.clone()),
                    },
                );
            }
        }
        // Module imports may form cycles. Resolve known targets to a fixed point;
        // a cycle of nonexistent import bindings never creates a declaration.
        let mut imports = vec![];
        let mut repeated = BTreeSet::new();
        for ast in files {
            for import in &ast.imports {
                let paths = if import.names.is_empty() {
                    vec![import.path.clone()]
                } else {
                    import
                        .names
                        .iter()
                        .map(|n| {
                            let mut p = import.path.clone();
                            p.push(n.clone());
                            p
                        })
                        .collect()
                };
                for path in paths {
                    if !repeated.insert((ast.module.clone(), path.clone())) {
                        return Err(Diagnostic::new(
                            "E_NAME",
                            import.span,
                            "duplicate import binding",
                        ));
                    }
                    imports.push((ast.module.clone(), path, import.span));
                }
            }
        }
        while !imports.is_empty() {
            let mut deferred = vec![];
            let mut progress = false;
            for (module, path, span) in imports {
                let mut found = vec![];
                for ns in [Namespace::Module, Namespace::Type, Namespace::Value] {
                    if let Some(b) = r.find(&module, &path, ns, span)? {
                        found.push((ns, b));
                    }
                }
                if found.is_empty() {
                    deferred.push((module, path, span));
                    continue;
                }
                let name = path.last().unwrap().clone();
                for (ns, b) in found {
                    r.insert(
                        module.clone(),
                        name.clone(),
                        ns,
                        Binding {
                            symbol: b.symbol,
                            public: false,
                            imported: true,
                            span,
                        },
                    )?;
                }
                progress = true;
            }
            if !progress {
                let (_, path, span) = &deferred[0];
                return Err(Diagnostic::new(
                    "E_NAME",
                    *span,
                    format!("unresolved import {}", path.join("::")),
                ));
            }
            imports = deferred;
        }
        Ok(r)
    }
    fn insert(
        &mut self,
        module: Vec<String>,
        name: String,
        ns: Namespace,
        binding: Binding,
    ) -> Result<()> {
        if ["root", "sizeof", "alignof", "offsetof", "static_assert"].contains(&name.as_str())
            || ns == Namespace::Type
                && [
                    "bool", "Copy", "u8", "u16", "u32", "u64", "usize", "i8", "i16", "i32", "i64",
                    "isize",
                ]
                .contains(&name.as_str())
        {
            return Err(Diagnostic::new(
                "E_NAME",
                binding.span,
                "reserved binding name",
            ));
        }
        if ns != Namespace::Value {
            let other = if ns == Namespace::Type {
                Namespace::Module
            } else {
                Namespace::Type
            };
            if self
                .symbols
                .contains_key(&(module.clone(), name.clone(), other))
            {
                return Err(Diagnostic::new(
                    "E_NAME",
                    binding.span,
                    "module and type names conflict",
                ));
            }
        }
        let key = (module, name, ns);
        if let Some(old) = self.symbols.get(&key) {
            return Err(
                Diagnostic::new("E_NAME", binding.span, "duplicate declaration or import")
                    .note(Some(old.span), "previous binding"),
            );
        }
        self.symbols.insert(key, binding);
        Ok(())
    }
    pub fn module_exists(&self, module: &[String]) -> bool {
        self.modules.contains(module)
    }
    /// Unknown paths return None for contextual variants or predeclared builtins.
    /// Found private declarations always produce E_NAME, even if fallback exists.
    pub fn lookup(
        &self,
        from: &[String],
        path: &[String],
        ns: Namespace,
        span: Span,
    ) -> Result<Option<DefId>> {
        Ok(self
            .find(from, path, ns, span)?
            .and_then(|b| match b.symbol {
                Symbol::Def(id) => Some(id),
                _ => None,
            }))
    }
    pub fn lookup_visible(
        &self,
        from: &[String],
        path: &[String],
        ns: Namespace,
        span: Span,
    ) -> Result<DefId> {
        self.lookup(from, path, ns, span)?.ok_or_else(|| {
            Diagnostic::new("E_NAME", span, format!("unknown name {}", path.join("::")))
        })
    }
    pub fn builtin_path(
        &self,
        from: &[String],
        path: &[String],
        ns: Namespace,
        span: Span,
    ) -> Result<Option<Vec<String>>> {
        Ok(self
            .find(from, path, ns, span)?
            .and_then(|b| match b.symbol {
                Symbol::Builtin(p) => Some(p),
                _ => None,
            }))
    }
    fn check_access(
        &self,
        from: &[String],
        owner: &[String],
        b: &Binding,
        span: Span,
    ) -> Result<()> {
        if from != owner && (!b.public || b.imported) {
            return Err(Diagnostic::new(
                "E_NAME",
                span,
                "private member is not visible in this module",
            ));
        }
        Ok(())
    }
    fn find(
        &self,
        from: &[String],
        path: &[String],
        ns: Namespace,
        span: Span,
    ) -> Result<Option<Binding>> {
        if path.is_empty() {
            return Ok(None);
        }
        if path[0] == "root" {
            return self.walk(from, vec![], &path[1..], ns, span);
        }
        // Current module bindings have priority, including explicit imports.
        if path.len() == 1
            && let Some(b) = self.symbols.get(&(from.to_vec(), path[0].clone(), ns))
        {
            return Ok(Some(b.clone()));
        }
        for initial in [from.to_vec(), vec!["core".into()], vec![]] {
            let exists = if path.len() == 1 {
                self.symbols
                    .contains_key(&(initial.clone(), path[0].clone(), ns))
            } else {
                [Namespace::Module, Namespace::Type].iter().any(|ns| {
                    self.symbols
                        .contains_key(&(initial.clone(), path[0].clone(), *ns))
                })
            };
            if exists {
                return self.walk(from, initial, path, ns, span);
            }
        }
        Ok(None)
    }
    fn walk(
        &self,
        from: &[String],
        mut owner: Vec<String>,
        path: &[String],
        ns: Namespace,
        span: Span,
    ) -> Result<Option<Binding>> {
        if path.is_empty() {
            return Ok(None);
        }
        for name in &path[..path.len() - 1] {
            let module = self
                .symbols
                .get(&(owner.clone(), name.clone(), Namespace::Module));
            let ty = self
                .symbols
                .get(&(owner.clone(), name.clone(), Namespace::Type));
            let Some(b) = module.or(ty) else {
                return Ok(None);
            };
            self.check_access(from, &owner, b, span)?;
            match &b.symbol {
                Symbol::Module(m) => owner = m.clone(),
                Symbol::Def(id) => {
                    let rec = &self.declarations[id];
                    owner = rec.module.clone();
                    owner.push(rec.decl.name.clone());
                }
                Symbol::Builtin(p) => owner = p.clone(),
            }
        }
        let Some(b) = self
            .symbols
            .get(&(owner.clone(), path.last().unwrap().clone(), ns))
        else {
            return Ok(None);
        };
        let accessibility_owner = if b.imported {
            &owner
        } else if let Symbol::Def(id) = b.symbol {
            &self.declarations[&id].module
        } else {
            &owner
        };
        self.check_access(from, accessibility_owner, b, span)?;
        Ok(Some(b.clone()))
    }
    fn add_builtin_registry(&mut self) {
        self.modules.extend([
            vec!["core".into()],
            vec!["arch".into()],
            vec!["arch".into(), "riscv".into()],
        ]);
        let types = [
            "Option",
            "Result",
            "MaybeUninit",
            "UnsafeCell",
            "Atomic",
            "MemoryOrder",
        ];
        let values = [
            "Some",
            "None",
            "Ok",
            "Err",
            "Relaxed",
            "Acquire",
            "Release",
            "AcqRel",
            "SeqCst",
            "replace",
            "swap",
            "split_at_mut",
            "forget",
            "panic",
            "checked_cast",
            "addr",
            "ptr",
            "ptr_add",
            "ptr_add_bytes",
            "addr_of",
            "addr_of_mut",
            "slice_from_raw_parts",
            "slice_from_raw_parts_mut",
            "volatile_load",
            "volatile_store",
            "mmio_load",
            "mmio_store",
        ];
        for (module, names, ns) in [
            (vec!["core".into()], types.as_slice(), Namespace::Type),
            (vec!["core".into()], values.as_slice(), Namespace::Value),
            (
                vec!["arch".into()],
                [
                    "asm",
                    "enter_trap_handler",
                    "enter_irq_handler",
                    "trap_return",
                    "device_read_fence",
                    "device_write_fence",
                ]
                .as_slice(),
                Namespace::Value,
            ),
            (
                vec!["arch".into()],
                ["TrapFrame"].as_slice(),
                Namespace::Type,
            ),
            (
                vec!["arch".into(), "riscv".into()],
                [
                    "cpu_relax",
                    "wfi",
                    "csr_read",
                    "csr_write",
                    "fence_i",
                    "sfence_vma_all",
                ]
                .as_slice(),
                Namespace::Value,
            ),
        ] {
            for name in names {
                let mut path = module.clone();
                path.push((*name).into());
                self.symbols.insert(
                    (module.clone(), (*name).into(), ns),
                    Binding {
                        symbol: Symbol::Builtin(path),
                        public: true,
                        imported: false,
                        span: Span::default(),
                    },
                );
            }
        }
        for (ty, members) in [
            ("Option", &["Some", "None"][..]),
            ("Result", &["Ok", "Err"][..]),
            (
                "MemoryOrder",
                &["Relaxed", "Acquire", "Release", "AcqRel", "SeqCst"][..],
            ),
            (
                "MaybeUninit",
                &["uninit", "write", "as_ptr", "as_mut_ptr", "assume_init"][..],
            ),
            ("UnsafeCell", &["new", "get"][..]),
            (
                "Atomic",
                &[
                    "new",
                    "load",
                    "store",
                    "exchange",
                    "compare_exchange",
                    "fetch_add",
                    "fetch_sub",
                    "fetch_and",
                    "fetch_or",
                ][..],
            ),
        ] {
            let module = vec!["core".into(), ty.into()];
            for member in members {
                let mut path = module.clone();
                path.push((*member).into());
                self.symbols.insert(
                    (module.clone(), (*member).into(), Namespace::Value),
                    Binding {
                        symbol: Symbol::Builtin(path),
                        public: true,
                        imported: false,
                        span: Span::default(),
                    },
                );
            }
        }
    }
}
