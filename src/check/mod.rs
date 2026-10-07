//! Core semantic checks and concrete, typed function instances.
mod builtins;
mod calls;
mod constants;
mod deferred;
mod expressions;
mod inference;
mod names;
mod patterns;
use crate::{
    diagnostic::{Diagnostic, Result},
    hir as h,
    ir::{Data, ScopeId},
    resolve::{DeclRecord, Namespace, Resolver},
    source::Span,
    syntax as ast,
    types::*,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
pub fn check(files: &[ast::AstFile]) -> Result<h::Program> {
    Checker::new(files)?.run()
}
type InstanceKey = (DefId, Vec<TyId>);
type SymbolContract = (Option<FnType>, Option<(TyId, bool)>, bool, Span);
#[derive(Clone, Copy)]
struct CallSite<'a> {
    arguments: &'a [ast::Expr],
    expected: Option<TyId>,
    span: Span,
}
struct RecordSpec {
    id: DefId,
    generics: Vec<ast::GenericParam>,
    variant: Option<usize>,
    fields: Vec<FieldDef>,
    inferred: Vec<Option<TyId>>,
    explicit: bool,
}
struct Checker {
    resolver: Resolver,
    types: TypeContext,
    signatures: BTreeMap<DefId, FnType>,
    functions: Vec<h::Function>,
    instances: BTreeMap<(DefId, Vec<TyId>), String>,
    active: Vec<(DefId, Vec<TyId>)>,
    instance_deps: BTreeMap<InstanceKey, BTreeSet<InstanceKey>>,
    data: Vec<Data>,
    externals: Vec<String>,
    constants: HashMap<DefId, h::Expr>,
    constant_active: BTreeSet<DefId>,
    type_active: BTreeSet<DefId>,
    types_defined: BTreeSet<DefId>,
    signature_active: BTreeSet<DefId>,
    deferred: deferred::State,
    aliases: BTreeSet<DefId>,
}
#[derive(Clone)]
struct Context {
    module: Vec<String>,
    generics: BTreeMap<String, TyId>,
    self_ty: Option<TyId>,
    locals: Vec<h::Local>,
    scopes: Vec<BTreeMap<String, h::LocalId>>,
    scope_ids: Vec<ScopeId>,
    next_scope: u32,
    unsafe_depth: u32,
    result: TyId,
    loops: Vec<(h::LoopId, Option<TyId>)>,
    next_loop: u32,
    defer: u32,
    special: Option<String>,
    in_defer: bool,
    pattern_borrow: Option<bool>,
    place_context: bool,
    readonly_operation: bool,
    template: bool,
    const_context: bool,
    const_env: BTreeMap<h::LocalId, h::Expr>,
    inference_hints: HashMap<Span, TyId>,
}
impl Context {
    fn new(module: Vec<String>, result: TyId) -> Self {
        Self {
            module,
            generics: BTreeMap::new(),
            self_ty: None,
            locals: vec![],
            scopes: vec![BTreeMap::new()],
            scope_ids: vec![ScopeId(0)],
            next_scope: 1,
            unsafe_depth: 0,
            result,
            loops: vec![],
            next_loop: 0,
            defer: 0,
            special: None,
            in_defer: false,
            pattern_borrow: None,
            place_context: false,
            readonly_operation: false,
            template: false,
            const_context: false,
            const_env: BTreeMap::new(),
            inference_hints: HashMap::new(),
        }
    }
    fn local(&self, name: &str) -> Option<h::LocalId> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }
}
fn error(code: &'static str, span: Span, msg: impl Into<String>) -> Diagnostic {
    Diagnostic::new(code, span, msg)
}
impl Checker {
    fn new(files: &[ast::AstFile]) -> Result<Self> {
        Ok(Self {
            resolver: Resolver::new(files)?,
            types: TypeContext::new(),
            signatures: BTreeMap::new(),
            functions: vec![],
            instances: BTreeMap::new(),
            active: vec![],
            instance_deps: BTreeMap::new(),
            data: vec![],
            externals: vec![],
            constants: HashMap::new(),
            constant_active: BTreeSet::new(),
            type_active: BTreeSet::new(),
            types_defined: BTreeSet::new(),
            signature_active: BTreeSet::new(),
            deferred: deferred::State::default(),
            aliases: BTreeSet::new(),
        })
    }
    fn unit(&mut self) -> TyId {
        self.types.intern(TypeKind::Unit)
    }
    fn never(&mut self) -> TyId {
        self.types.intern(TypeKind::Never)
    }
    fn bool(&mut self) -> TyId {
        self.types.intern(TypeKind::Bool)
    }
    fn int(&mut self, i: Integer) -> TyId {
        self.types.intern(TypeKind::Int(i))
    }
    pub(super) fn ensure_signature(&mut self, id: DefId) -> Result<FnType> {
        if let Some(sig) = self.signatures.get(&id) {
            return Ok(sig.clone());
        }
        let r = self.resolver.declarations[&id].clone();
        let ast::DeclKind::Function(f) = &r.decl.kind else {
            return Err(error(
                "E_TYPE",
                r.decl.span,
                "function declaration expected",
            ));
        };
        if !self.signature_active.insert(id) {
            return Err(error(
                "E_TYPE",
                r.decl.span,
                "cyclic function signature dependency",
            ));
        }
        let result = (|| {
            let mut ctx = Context::new(r.module.clone(), self.unit());
            ctx.template = true;
            for (i, g) in r.impl_generics.iter().chain(f.generics.iter()).enumerate() {
                ctx.generics
                    .insert(g.name.clone(), self.types.intern(TypeKind::Param(i as u32)));
            }
            if let Some(t) = &r.impl_target {
                ctx.self_ty = Some(self.ty(t, &mut ctx)?)
            }
            self.signature(f, &mut ctx)
        })();
        self.signature_active.remove(&id);
        let sig = result?;
        self.signatures.insert(id, sig.clone());
        Ok(sig)
    }
    pub(super) fn declaration_type(&mut self, id: DefId) -> Result<TyId> {
        let r = self.resolver.declarations[&id].clone();
        let ty = match r.decl.kind {
            ast::DeclKind::Const { ty, .. } | ast::DeclKind::Static { ty, .. } => ty,
            _ => {
                return Err(error(
                    "E_TYPE",
                    r.decl.span,
                    "value has no declared object type",
                ));
            }
        };
        let mut ctx = Context::new(r.module, self.unit());
        self.ty(&ty, &mut ctx)
    }
    pub(super) fn field_access(
        &self,
        ty: TyId,
        name: &str,
        module: &[String],
        span: Span,
    ) -> Result<()> {
        if let TypeKind::Nominal(id, _) = self.types.kind(ty)
            && let Some(r) = self.resolver.declarations.get(id)
            && r.module != module
            && let ast::DeclKind::Struct { fields, .. } = &r.decl.kind
            && !fields.iter().any(|f| f.name == name && f.public)
        {
            return Err(error("E_NAME", span, "private field"));
        }
        Ok(())
    }
    fn run(mut self) -> Result<h::Program> {
        let records = self.resolver.declarations.clone();
        for (id, r) in &records {
            self.attributes(r)?;
            if let ast::DeclKind::Struct { generics, .. } | ast::DeclKind::Enum { generics, .. } =
                &r.decl.kind
            {
                self.types.define(
                    *id,
                    TypeDef {
                        name: r.decl.name.clone(),
                        params: generics.len() as u32,
                        kind: DefKind::Struct(vec![]),
                        copy: false,
                        resource: false,
                        repr_c: false,
                        packed: false,
                        align: None,
                        span: r.decl.span,
                        layout_dependencies: vec![],
                    },
                );
            }
        }
        for r in records.values() {
            if let ast::DeclKind::Impl {
                generics,
                target,
                methods,
            } = &r.decl.kind
            {
                let ast::TypeKind::Path(p) = &target.kind else {
                    return Err(error(
                        "E_TYPE",
                        target.span,
                        "impl target must be local nominal type",
                    ));
                };
                let path = p.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
                let id =
                    self.resolver
                        .lookup_visible(&r.module, &path, Namespace::Type, target.span)?;
                let owner = &self.resolver.declarations[&id];
                let declared = match &owner.decl.kind {
                    ast::DeclKind::Struct { generics, .. }
                    | ast::DeclKind::Enum { generics, .. }
                        if owner.module == r.module =>
                    {
                        generics
                    }
                    _ => {
                        return Err(error(
                            "E_TYPE",
                            target.span,
                            "impl requires local nominal declaration",
                        ));
                    }
                };
                let args = &p.last().unwrap().arguments;
                if declared.len() != generics.len() || args.len() != generics.len() {
                    return Err(error(
                        "E_TYPE",
                        target.span,
                        "impl parameters must match nominal parameters",
                    ));
                }
                for ((declared, g), arg) in declared.iter().zip(generics).zip(args) {
                    if declared.copy != g.copy
                        || !matches!(&arg.kind,ast::TypeKind::Path(p) if p.len()==1&&p[0].name==g.name&&p[0].arguments.is_empty())
                    {
                        return Err(error(
                            "E_TYPE",
                            target.span,
                            "impl specialization or reordered parameters forbidden",
                        ));
                    }
                }
                for m in methods {
                    if matches!(&m.kind,ast::DeclKind::Function(f) if f.external) {
                        return Err(error("E_TYPE", m.span, "extern methods forbidden"));
                    }
                }
            }
        }
        self.builtin_types();
        for (id, r) in &records {
            if matches!(
                r.decl.kind,
                ast::DeclKind::Struct { .. } | ast::DeclKind::Enum { .. }
            ) {
                self.define_type(*id, r)?;
            }
        }
        for (id, r) in &records {
            if let ast::DeclKind::Function(f) = &r.decl.kind {
                let mut ctx = Context::new(r.module.clone(), self.unit());
                for (i, g) in r.impl_generics.iter().chain(f.generics.iter()).enumerate() {
                    let ty = self.types.intern(TypeKind::Param(i as u32));
                    if ctx.generics.insert(g.name.clone(), ty).is_some() {
                        return Err(error("E_NAME", g.span, "duplicate generic parameter"));
                    }
                }
                if let Some(target) = &r.impl_target {
                    ctx.self_ty = Some(self.ty(target, &mut ctx)?)
                }
                let sig = self.signature(f, &mut ctx)?;
                self.signatures.insert(*id, sig);
            }
        }
        for (id, r) in &records {
            self.public_interface(*id, r)?;
        }
        for (id, r) in &records {
            match &r.decl.kind {
                ast::DeclKind::Function(f)
                    if f.generics.is_empty() && r.impl_generics.is_empty() =>
                {
                    self.instance(*id, vec![])?;
                }
                ast::DeclKind::Const { .. } => {
                    self.constant(*id)?;
                }
                ast::DeclKind::Static { .. } => {
                    self.static_data(*id, r)?;
                }
                ast::DeclKind::Assert(e) => {
                    let mut ctx = Context::new(r.module.clone(), self.unit());
                    ctx.const_context = true;
                    let b = self.bool();
                    let typed = self.expr(e, Some(b), &mut ctx)?;
                    if self.eval(&typed)? != 1 {
                        return Err(error("E_CONST", e.span, "static assertion is false"));
                    }
                }
                _ => {}
            }
        }
        for (id, r) in &records {
            if let ast::DeclKind::Alias(t) = &r.decl.kind {
                let mut c = Context::new(r.module.clone(), self.unit());
                self.ty(t, &mut c)?;
            }
            if let ast::DeclKind::Function(f) = &r.decl.kind
                && (!f.generics.is_empty() || !r.impl_generics.is_empty())
            {
                self.template_names(*id, r, f)?;
                self.template_body(*id, r, f)?;
            }
        }
        self.link_symbols(&records)?;
        for (id, r) in &records {
            if let Some(d) = self.types.definition(*id).cloned() {
                if d.copy && d.resource {
                    return Err(error("E_TYPE", r.decl.span, "resource type cannot be Copy"));
                }
                if d.copy && d.params == 0 {
                    for f in d.fields() {
                        if !self.types.properties(f.ty).copy {
                            return Err(error("E_TYPE", f.span, "Copy type has noncopy field"));
                        }
                    }
                }
            }
        }
        // Every concrete declaration requires finite, target-valid object layout.
        for (id, r) in &records {
            if let ast::DeclKind::Struct { generics, .. } | ast::DeclKind::Enum { generics, .. } =
                &r.decl.kind
                && generics.is_empty()
            {
                let t = self.types.intern(TypeKind::Nominal(*id, vec![]));
                self.prepare_type(t)?;
                crate::layout::LayoutEngine::new(&mut self.types).layout(t)?;
            }
        }
        Ok(h::Program {
            declarations: Some(self.resolver.clone()),
            types: self.types,
            functions: self.functions,
            data: self.data,
            externals: self.externals,
        })
    }
    fn attributes(&mut self, r: &DeclRecord) -> Result<()> {
        let mut seen = BTreeSet::new();
        for a in &r.decl.attributes {
            if !seen.insert(&a.name) {
                return Err(error("E_ATTRIBUTE", a.span, "duplicate attribute"));
            }
            let kind = &r.decl.kind;
            let allowed = match a.name.as_str() {
                "copy" => matches!(
                    kind,
                    ast::DeclKind::Struct { .. } | ast::DeclKind::Enum { .. }
                ),
                "repr" | "packed" => matches!(kind, ast::DeclKind::Struct { .. }),
                "align" => matches!(
                    kind,
                    ast::DeclKind::Struct { .. }
                        | ast::DeclKind::Enum { .. }
                        | ast::DeclKind::Static { value: Some(_), .. }
                ),
                "export" | "section" => matches!(
                    kind,
                    ast::DeclKind::Function(ast::Function { body: Some(_), .. })
                        | ast::DeclKind::Static { value: Some(_), .. }
                ),
                "irq" | "trap" | "trap_entry" => matches!(
                    kind,
                    ast::DeclKind::Function(ast::Function {
                        body: Some(_),
                        external: false,
                        ..
                    })
                ),
                _ => false,
            };
            if !allowed {
                return Err(error(
                    "E_ATTRIBUTE",
                    a.span,
                    "unknown or misplaced attribute",
                ));
            }
            match a.name.as_str() {
                "copy" | "packed" | "irq" | "trap" | "trap_entry" => {
                    if !a.arguments.is_empty() {
                        return Err(error("E_ATTRIBUTE", a.span, "attribute takes no arguments"));
                    }
                }
                "repr" => {
                    if a.arguments.len() != 1
                        || !matches!(&a.arguments[0].kind,ast::ExprKind::Path(p) if p.len()==1&&p[0].name=="C")
                    {
                        return Err(error("E_ATTRIBUTE", a.span, "repr requires C"));
                    }
                }
                "export" | "section" => {
                    if a.arguments.len() != 1
                        || !matches!(&a.arguments[0].kind,ast::ExprKind::String(b) if !b.is_empty()&&!b.contains(&0))
                    {
                        return Err(error(
                            "E_ATTRIBUTE",
                            a.span,
                            "attribute requires nonempty symbol string",
                        ));
                    }
                }
                "align" if a.arguments.len() != 1 => {
                    return Err(error("E_LAYOUT", a.span, "alignment needs one integer"));
                }
                _ => {}
            }
        }
        if seen.contains(&"packed".to_string()) && seen.contains(&"align".to_string()) {
            return Err(error("E_LAYOUT", r.decl.span, "packed and align conflict"));
        }
        if seen
            .iter()
            .filter(|s| ["trap", "irq", "trap_entry"].contains(&s.as_str()))
            .count()
            > 1
        {
            return Err(error(
                "E_ATTRIBUTE",
                r.decl.span,
                "conflicting context attributes",
            ));
        }
        if let ast::DeclKind::Function(f) = &r.decl.kind
            && !f.generics.is_empty()
            && !r.decl.attributes.iter().all(|a| a.name == "section")
        {
            return Err(error(
                "E_ATTRIBUTE",
                r.decl.span,
                "attribute requires nongeneric function",
            ));
        }
        if let ast::DeclKind::Impl { .. } | ast::DeclKind::Assert(_) = r.decl.kind
            && r.decl.public
        {
            return Err(error("E_NAME", r.decl.span, "declaration cannot be public"));
        }
        Ok(())
    }
    fn builtin_types(&mut self) {
        let u = self.int(Integer::Usize);
        let trap = DefId(0xffff_ff01);
        let array = self.types.intern(TypeKind::Array(u, 32));
        let mut fields = vec![FieldDef {
            name: "x".into(),
            ty: array,
            span: Span::default(),
        }];
        for name in ["sepc", "sstatus", "scause", "stval"] {
            fields.push(FieldDef {
                name: name.into(),
                ty: u,
                span: Span::default(),
            })
        }
        self.types.define(
            trap,
            TypeDef {
                name: "arch::TrapFrame".into(),
                params: 0,
                kind: DefKind::Struct(fields),
                copy: false,
                resource: false,
                repr_c: true,
                packed: false,
                align: None,
                span: Span::default(),
                layout_dependencies: vec![],
            },
        );
        let order = DefId(0xffff_ff00);
        self.types.define(
            order,
            TypeDef {
                name: "MemoryOrder".into(),
                params: 0,
                kind: DefKind::Enum(
                    ["Relaxed", "Acquire", "Release", "AcqRel", "SeqCst"]
                        .iter()
                        .map(|n| VariantDef {
                            name: (*n).into(),
                            fields: vec![],
                        })
                        .collect(),
                ),
                copy: true,
                resource: false,
                repr_c: false,
                packed: false,
                align: None,
                span: Span::default(),
                layout_dependencies: vec![],
            },
        );
    }
    fn define_type(&mut self, id: DefId, r: &DeclRecord) -> Result<()> {
        if !self.type_active.insert(id) {
            return Err(error(
                "E_LAYOUT",
                r.decl.span,
                "cyclic type definition dependency",
            ));
        }
        let result = self.define_type_inner(id, r);
        self.type_active.remove(&id);
        if result.is_ok() {
            self.types_defined.insert(id);
        }
        result
    }
    fn ensure_type_definition(&mut self, id: DefId) -> Result<()> {
        if !self.types_defined.contains(&id) && self.resolver.declarations.contains_key(&id) {
            let r = self.resolver.declarations[&id].clone();
            self.define_type(id, &r)?;
        }
        Ok(())
    }
    fn define_type_inner(&mut self, id: DefId, r: &DeclRecord) -> Result<()> {
        let mut ctx = Context::new(r.module.clone(), self.unit());
        let (generics, resource) = match &r.decl.kind {
            ast::DeclKind::Struct {
                generics, resource, ..
            } => (generics, *resource),
            ast::DeclKind::Enum { generics, .. } => (generics, false),
            _ => unreachable!(),
        };
        for (i, g) in generics.iter().enumerate() {
            let p = self.types.intern(TypeKind::Param(i as u32));
            if ctx.generics.insert(g.name.clone(), p).is_some() {
                return Err(error("E_NAME", g.span, "duplicate generic parameter"));
            }
        }
        let field_list =
            |this: &mut Self, fields: &[ast::Field], ctx: &mut Context| -> Result<Vec<FieldDef>> {
                let mut seen = BTreeSet::new();
                let mut v = vec![];
                for f in fields {
                    if !seen.insert(&f.name) {
                        return Err(error("E_NAME", f.span, "duplicate field"));
                    }
                    v.push(FieldDef {
                        name: f.name.clone(),
                        ty: {
                            let t = this.ty(&f.ty, ctx)?;
                            this.object_type(t, f.span)?;
                            t
                        },
                        span: f.span,
                    })
                }
                Ok(v)
            };
        let kind = match &r.decl.kind {
            ast::DeclKind::Struct {
                fields, newtype, ..
            } => {
                let mut f = field_list(self, fields, &mut ctx)?;
                if let Some(t) = newtype {
                    f.push(FieldDef {
                        name: "0".into(),
                        ty: {
                            let value = self.ty(t, &mut ctx)?;
                            self.object_type(value, t.span)?;
                            value
                        },
                        span: t.span,
                    })
                }
                DefKind::Struct(f)
            }
            ast::DeclKind::Enum { variants, .. } => {
                let mut seen = BTreeSet::new();
                let mut v = vec![];
                for variant in variants {
                    if !seen.insert(&variant.name) {
                        return Err(error("E_NAME", variant.span, "duplicate variant"));
                    }
                    let mut fields = field_list(self, &variant.fields, &mut ctx)?;
                    if let Some(t) = &variant.payload {
                        fields.push(FieldDef {
                            name: "0".into(),
                            ty: {
                                let value = self.ty(t, &mut ctx)?;
                                self.object_type(value, t.span)?;
                                value
                            },
                            span: t.span,
                        })
                    }
                    v.push(VariantDef {
                        name: variant.name.clone(),
                        fields,
                    })
                }
                DefKind::Enum(v)
            }
            _ => unreachable!(),
        };
        let attr = |n: &str| r.decl.attributes.iter().find(|a| a.name == n);
        let align = if let Some(a) = attr("align") {
            Some(self.const_ast(&a.arguments[0], &mut ctx)?)
        } else {
            None
        };
        if align.is_some_and(|a| a == 0 || !a.is_power_of_two() || a > 4096) {
            return Err(error("E_LAYOUT", r.decl.span, "unsupported alignment"));
        }
        self.types.define(
            id,
            TypeDef {
                name: r.decl.name.clone(),
                params: generics.len() as u32,
                kind,
                copy: attr("copy").is_some(),
                resource,
                repr_c: attr("repr").is_some(),
                packed: attr("packed").is_some(),
                align,
                span: r.decl.span,
                layout_dependencies: vec![],
            },
        );
        Ok(())
    }
    fn signature(&mut self, f: &ast::Function, ctx: &mut Context) -> Result<FnType> {
        let mut names = BTreeSet::new();
        if f.external && !f.generics.is_empty() {
            return Err(error(
                "E_TYPE",
                f.result.span,
                "C ABI functions cannot be generic",
            ));
        }
        let mut params = vec![];
        for p in &f.parameters {
            if !names.insert(&p.name) {
                return Err(error("E_NAME", p.span, "duplicate parameter"));
            }
            if p.receiver.is_some() && ctx.self_ty.is_none() {
                return Err(error(
                    "E_TYPE",
                    p.span,
                    "receiver outside inherent implementation",
                ));
            }
            let t = self.ty(&p.ty, ctx)?;
            self.object_type(t, p.span)?;
            params.push(t)
        }
        let result = self.ty(&f.result, ctx)?;
        let mut effects = BTreeSet::new();
        for e in &f.effects {
            if e != "may_block" || !effects.insert(e.clone()) {
                return Err(error("E_TYPE", f.result.span, "invalid effects"));
            }
        }
        let sig = FnType {
            params,
            result,
            abi: if f.external { Abi::C } else { Abi::Biomiya },
            unsafe_: f.unsafe_ || f.external,
            effects,
        };
        if sig.abi == Abi::C {
            for p in &sig.params {
                self.prepare_type(*p)?;
                crate::abi::validate_c_type(&mut self.types, *p, false)?
            }
            self.prepare_type(result)?;
            crate::abi::validate_c_type(&mut self.types, result, true)?
        }
        Ok(sig)
    }
    fn symbol(&self, id: DefId, args: &[TyId]) -> String {
        let r = &self.resolver.declarations[&id];
        if let Some(a) = r.decl.attributes.iter().find(|a| a.name == "export")
            && let ast::ExprKind::String(s) = &a.arguments[0].kind
        {
            return crate::linkage::encode_name(s);
        }
        if matches!(&r.decl.kind,ast::DeclKind::Function(f) if f.external) {
            return r.decl.name.clone();
        }
        let mut s = format!("__biomiya_fn_{}", id.0);
        for a in args {
            s.push_str(&format!("_{}", a.0))
        }
        s
    }
    fn instance(&mut self, id: DefId, args: Vec<TyId>) -> Result<String> {
        let key = (id, args.clone());
        if let Some(parent) = self.active.last() {
            self.instance_deps
                .entry(parent.clone())
                .or_default()
                .insert(key.clone());
        }
        self.validate_instance_path(&key, &mut self.active.clone(), &mut BTreeSet::new())?;
        if let Some((_, old)) = self.active.iter().find(|(d, _)| *d == id)
            && old != &args
        {
            return Err(error(
                "E_INSTANTIATION",
                self.resolver.declarations[&id].decl.span,
                "polymorphic recursion",
            ));
        }
        if let Some(s) = self.instances.get(&(id, args.clone())) {
            return Ok(s.clone());
        }
        let r = self.resolver.declarations[&id].clone();
        let ast::DeclKind::Function(f) = r.decl.kind.clone() else {
            return Err(error("E_TYPE", r.decl.span, "not a function"));
        };
        let gs = r
            .impl_generics
            .iter()
            .chain(f.generics.iter())
            .collect::<Vec<_>>();
        if gs.len() != args.len() {
            return Err(error(
                "E_TYPE",
                r.decl.span,
                "generic argument count mismatch",
            ));
        }
        let mut ctx = Context::new(r.module.clone(), self.unit());
        for (g, t) in gs.iter().zip(&args) {
            if g.copy && !self.types.properties(*t).copy {
                return Err(error(
                    "E_INSTANTIATION",
                    g.span,
                    "Copy constraint not satisfied",
                ));
            }
            ctx.generics.insert(g.name.clone(), *t);
        }
        if let Some(t) = &r.impl_target {
            ctx.self_ty = Some(self.ty(t, &mut ctx)?)
        }
        let signature = self.signature(&f, &mut ctx)?;
        for t in &signature.params {
            self.object_type(*t, r.decl.span)?;
            if !self.has_params(*t) {
                self.prepare_type(*t)?;
                crate::layout::LayoutEngine::new(&mut self.types).layout(*t)?;
            }
        }
        if !matches!(self.types.kind(signature.result), TypeKind::Never)
            && !self.has_params(signature.result)
        {
            self.prepare_type(signature.result)?;
            crate::layout::LayoutEngine::new(&mut self.types).layout(signature.result)?;
        }
        ctx.result = signature.result;
        ctx.special = r
            .decl
            .attributes
            .iter()
            .find(|a| ["irq", "trap", "trap_entry"].contains(&a.name.as_str()))
            .map(|a| a.name.clone());
        if let Some(special) = &ctx.special {
            let valid_frame = signature.params.len() == 1
                && matches!(self.types.kind(signature.params[0]),TypeKind::Ref{target,mutable:true} if matches!(self.types.kind(*target),TypeKind::Nominal(DefId(0xffff_ff01),v) if v.is_empty()));
            let valid_result = if special == "trap_entry" {
                matches!(self.types.kind(signature.result), TypeKind::Never)
            } else {
                matches!(self.types.kind(signature.result), TypeKind::Unit)
            };
            if !valid_frame || !valid_result || !gs.is_empty() || signature.abi != Abi::Biomiya {
                return Err(error(
                    "E_CONTEXT",
                    r.decl.span,
                    "invalid special function signature",
                ));
            }
        }
        let symbol = self.symbol(id, &args);
        self.instances.insert((id, args.clone()), symbol.clone());
        if f.body.is_none() {
            self.externals.push(symbol.clone());
            return Ok(symbol);
        }
        self.active.push((id, args.clone()));
        let mut parameters = vec![];
        for (p, t) in f.parameters.iter().zip(&signature.params) {
            parameters.push(self.bind(&p.name, *t, false, p.span, &mut ctx)?)
        }
        let body = self.block(f.body.as_ref().unwrap(), Some(signature.result), &mut ctx)?;
        self.coerce(body.ty, signature.result, body.span)?;
        let section = r
            .decl
            .attributes
            .iter()
            .find(|a| a.name == "section")
            .and_then(|a| match &a.arguments[0].kind {
                ast::ExprKind::String(s) => Some(crate::linkage::encode_name(s)),
                _ => None,
            });
        self.functions.push(h::Function {
            name: {
                let mut p = r.module.clone();
                if let Some(owner) = r.impl_owner {
                    p.push(self.resolver.declarations[&owner].decl.name.clone())
                }
                p.push(r.decl.name.clone());
                p.join("::")
            },
            definition: id,
            symbol: symbol.clone(),
            signature,
            type_arguments: args,
            parameters,
            locals: ctx.locals,
            body,
            span: r.decl.span,
            special: ctx.special,
            exported: r.decl.attributes.iter().any(|a| a.name == "export"),
            section,
        });
        self.active.pop();
        Ok(symbol)
    }
    fn validate_instance_path(
        &self,
        node: &(DefId, Vec<TyId>),
        stack: &mut Vec<(DefId, Vec<TyId>)>,
        seen: &mut BTreeSet<(DefId, Vec<TyId>)>,
    ) -> Result<()> {
        if let Some((_, args)) = stack.iter().find(|(id, _)| *id == node.0) {
            if args != &node.1 {
                return Err(error(
                    "E_INSTANTIATION",
                    self.resolver.declarations[&node.0].decl.span,
                    "polymorphic recursion through cached instance",
                ));
            }
            return Ok(());
        }
        if !seen.insert(node.clone()) {
            return Ok(());
        }
        stack.push(node.clone());
        if let Some(deps) = self.instance_deps.get(node) {
            for dep in deps {
                self.validate_instance_path(dep, stack, seen)?
            }
        }
        stack.pop();
        Ok(())
    }
    fn bind(
        &mut self,
        name: &str,
        ty: TyId,
        mutable: bool,
        span: Span,
        ctx: &mut Context,
    ) -> Result<h::LocalId> {
        if name == "root" || ["sizeof", "alignof", "offsetof", "static_assert"].contains(&name) {
            return Err(error("E_NAME", span, "reserved binding name"));
        }
        if ctx.scopes.last().unwrap().contains_key(name) {
            return Err(error("E_NAME", span, "duplicate local binding"));
        }
        let id = h::LocalId(ctx.locals.len() as u32);
        ctx.locals.push(h::Local {
            name: name.into(),
            ty,
            mutable,
            scope: *ctx.scope_ids.last().unwrap(),
            span,
        });
        ctx.scopes.last_mut().unwrap().insert(name.into(), id);
        Ok(id)
    }
    fn link_symbols(&mut self, records: &BTreeMap<DefId, DeclRecord>) -> Result<()> {
        let mut exports: BTreeMap<String, Span> = BTreeMap::new();
        for r in records.values() {
            if let Some(a) = r.decl.attributes.iter().find(|a| a.name == "export") {
                let ast::ExprKind::String(bytes) = &a.arguments[0].kind else {
                    unreachable!()
                };
                let name = crate::linkage::encode_name(bytes);
                if bytes.starts_with(b"__biomiya_") {
                    return Err(error("E_LINK", a.span, "reserved compiler/runtime symbol"));
                }
                if let Some(old) = exports.insert(name, r.decl.span) {
                    return Err(error("E_LINK", a.span, "duplicate export symbol")
                        .note(Some(old), "previous export"));
                }
            }
        }
        let mut symbols: BTreeMap<String, SymbolContract> = BTreeMap::new();
        for (id, r) in records {
            let exported = r.decl.attributes.iter().any(|a| a.name == "export");
            let entry = match &r.decl.kind {
                ast::DeclKind::Function(f) if f.external || exported => {
                    let symbol = self.symbol(*id, &[]);
                    Some((
                        symbol,
                        Some(self.signatures[id].clone()),
                        None,
                        f.body.is_some(),
                    ))
                }
                ast::DeclKind::Static {
                    ty,
                    mutable,
                    external,
                    ..
                } if *external || exported => {
                    let mut c = Context::new(r.module.clone(), self.unit());
                    let t = self.ty(ty, &mut c)?;
                    Some((
                        self.global_symbol(*id),
                        None,
                        Some((t, *mutable)),
                        !*external,
                    ))
                }
                _ => None,
            };
            if let Some((symbol, function, object, defined)) = entry {
                if let Some((oldf, oldo, oldd, oldspan)) = symbols.get(&symbol) {
                    let compatible = match (oldf, &function, oldo, &object) {
                        (Some(a), Some(b), None, None) => {
                            a.abi == b.abi
                                && a.unsafe_ == b.unsafe_
                                && a.params.len() == b.params.len()
                                && a.params
                                    .iter()
                                    .zip(&b.params)
                                    .all(|(a, b)| self.types.core_compatible(*a, *b))
                                && self.types.core_compatible(a.result, b.result)
                        }
                        (None, None, Some((a, m)), Some((b, n))) => {
                            m == n && self.types.core_compatible(*a, *b)
                        }
                        _ => false,
                    };
                    if !compatible || defined && *oldd {
                        return Err(error(
                            "E_LINK",
                            r.decl.span,
                            "external symbol declaration/definition mismatch",
                        )
                        .note(Some(*oldspan), "previous symbol"));
                    }
                    if defined {
                        symbols.insert(symbol, (function, object, true, r.decl.span));
                    }
                } else {
                    symbols.insert(symbol, (function, object, defined, r.decl.span));
                }
            }
        }
        Ok(())
    }
    fn coerce(&self, a: TyId, b: TyId, span: Span) -> Result<()> {
        if a == b
            || matches!(self.types.kind(a), TypeKind::Never)
            || self.types.core_compatible(a, b)
        {
            return Ok(());
        }
        match (self.types.kind(a), self.types.kind(b)) {
            (
                TypeKind::Ref {
                    target: a,
                    mutable: true,
                },
                TypeKind::Ref { target: b, .. },
            ) if self.types.core_compatible(*a, *b) => Ok(()),
            (
                TypeKind::Slice {
                    element: a,
                    const_: false,
                },
                TypeKind::Slice {
                    element: b,
                    const_: true,
                },
            ) if self.types.core_compatible(*a, *b) => Ok(()),
            (
                TypeKind::Pointer {
                    target: a,
                    space: x,
                    const_: false,
                },
                TypeKind::Pointer {
                    target: b,
                    space: y,
                    const_: true,
                },
            ) if x == y && self.types.core_compatible(*a, *b) => Ok(()),
            (TypeKind::Function(a), TypeKind::Function(b))
                if !a.unsafe_
                    && b.unsafe_
                    && a.abi == b.abi
                    && a.params.len() == b.params.len()
                    && a.params
                        .iter()
                        .zip(&b.params)
                        .all(|(x, y)| self.types.core_compatible(*x, *y))
                    && self.types.core_compatible(a.result, b.result) =>
            {
                Ok(())
            }
            _ => Err(error(
                "E_TYPE",
                span,
                format!(
                    "type mismatch: {} vs {}",
                    self.types.display(a),
                    self.types.display(b)
                ),
            )),
        }
    }
    fn materialize(&mut self, t: TyId, span: Span) -> Result<TyId> {
        if !self.has_params(t) {
            self.prepare_type(t)?;
        }
        let k = match self.types.kind(t).clone() {
            TypeKind::ArrayExpr(elem, len) => {
                let n = crate::layout::LayoutEngine::new(&mut self.types)
                    .evaluate_len(&len)
                    .map_err(|mut d| {
                        d.span = span;
                        d
                    })?;
                let elem = self.materialize(elem, span)?;
                TypeKind::Array(elem, n)
            }
            TypeKind::Array(t, n) => TypeKind::Array(self.materialize(t, span)?, n),
            TypeKind::Tuple(v) => TypeKind::Tuple(
                v.into_iter()
                    .map(|t| self.materialize(t, span))
                    .collect::<Result<_>>()?,
            ),
            TypeKind::Ref { target, mutable } => TypeKind::Ref { target, mutable },
            TypeKind::Pointer {
                target,
                space,
                const_,
            } => TypeKind::Pointer {
                target,
                space,
                const_,
            },
            TypeKind::Slice { element, const_ } => TypeKind::Slice { element, const_ },
            TypeKind::Function(mut f) => {
                f.params = f
                    .params
                    .into_iter()
                    .map(|t| self.materialize(t, span))
                    .collect::<Result<_>>()?;
                f.result = self.materialize(f.result, span)?;
                TypeKind::Function(f)
            }
            k => k,
        };
        Ok(self.types.intern(k))
    }
    fn public_interface(&mut self, id: DefId, r: &DeclRecord) -> Result<()> {
        if !r.decl.public {
            return Ok(());
        }
        if r.impl_owner
            .is_some_and(|id| !self.resolver.declarations[&id].decl.public)
        {
            return Ok(());
        }
        match &r.decl.kind {
            ast::DeclKind::Function(_) => {
                let f = self.signatures[&id].clone();
                for t in f.params.iter().chain(std::iter::once(&f.result)) {
                    self.public_type(*t, r.decl.span)?
                }
            }
            ast::DeclKind::Struct { fields, .. } => {
                let d = self.types.definition(id).unwrap().clone();
                for f in &fields[..] {
                    if f.public {
                        let tf = d.fields().into_iter().find(|x| x.name == f.name).unwrap();
                        self.public_type(tf.ty, f.span)?
                    }
                }
            }
            ast::DeclKind::Enum { .. } => {
                let d = self.types.definition(id).unwrap().clone();
                for f in d.fields() {
                    self.public_type(f.ty, f.span)?
                }
            }
            ast::DeclKind::Alias(t)
            | ast::DeclKind::Const { ty: t, .. }
            | ast::DeclKind::Static { ty: t, .. } => {
                let mut c = Context::new(r.module.clone(), self.unit());
                let t = self.ty(t, &mut c)?;
                self.public_type(t, r.decl.span)?
            }
            _ => {}
        }
        Ok(())
    }
    fn public_type(&self, t: TyId, span: Span) -> Result<()> {
        match self.types.kind(t) {
            TypeKind::Nominal(id, args) => {
                if self
                    .resolver
                    .declarations
                    .get(id)
                    .is_some_and(|r| !r.decl.public)
                {
                    return Err(error(
                        "E_NAME",
                        span,
                        "public interface exposes private type",
                    ));
                }
                for t in args {
                    self.public_type(*t, span)?
                }
            }
            TypeKind::Ref { target: t, .. }
            | TypeKind::Pointer { target: t, .. }
            | TypeKind::Slice { element: t, .. }
            | TypeKind::Array(t, _)
            | TypeKind::ArrayExpr(t, _)
            | TypeKind::Option(t)
            | TypeKind::MaybeUninit(t)
            | TypeKind::UnsafeCell(t)
            | TypeKind::Atomic(t) => self.public_type(*t, span)?,
            TypeKind::Tuple(v) => {
                for t in v {
                    self.public_type(*t, span)?
                }
            }
            TypeKind::Result(a, b) => {
                self.public_type(*a, span)?;
                self.public_type(*b, span)?
            }
            TypeKind::Function(f) => {
                for t in &f.params {
                    self.public_type(*t, span)?
                }
                self.public_type(f.result, span)?
            }
            _ => {}
        }
        Ok(())
    }
    fn object_type(&self, t: TyId, span: Span) -> Result<()> {
        if matches!(self.types.kind(t), TypeKind::Never | TypeKind::Range(_)) {
            Err(error("E_TYPE", span, "type has no object representation"))
        } else {
            Ok(())
        }
    }
    fn ty(&mut self, t: &ast::Type, ctx: &mut Context) -> Result<TyId> {
        let k = match &t.kind {
            ast::TypeKind::Never => TypeKind::Never,
            ast::TypeKind::Tuple(v) => {
                if v.is_empty() {
                    TypeKind::Unit
                } else {
                    TypeKind::Tuple(v.iter().map(|t| self.ty(t, ctx)).collect::<Result<_>>()?)
                }
            }
            ast::TypeKind::Ref { mutable, inner } => TypeKind::Ref {
                target: self.ty(inner, ctx)?,
                mutable: *mutable,
            },
            ast::TypeKind::Pointer {
                space,
                constant,
                inner,
            } => TypeKind::Pointer {
                target: self.ty(inner, ctx)?,
                space: space.as_deref().map_or(AddressSpace::Kernel, |s| match s {
                    "user" => AddressSpace::User,
                    "phys" => AddressSpace::Phys,
                    "mmio" => AddressSpace::Mmio,
                    _ => AddressSpace::Kernel,
                }),
                const_: *constant,
            },
            ast::TypeKind::Slice { constant, inner } => TypeKind::Slice {
                element: self.ty(inner, ctx)?,
                const_: *constant,
            },
            ast::TypeKind::Array { length, inner } => {
                let element = self.ty(inner, ctx)?;
                if ctx.generics.values().any(|t| self.has_params(*t)) {
                    TypeKind::ArrayExpr(element, self.deferred_array_length(length, ctx)?)
                } else {
                    let n = self.const_ast(length, ctx)?;
                    TypeKind::Array(element, n)
                }
            }
            ast::TypeKind::Function {
                parameters,
                result,
                unsafe_,
                external,
                effects,
            } => {
                let params = parameters
                    .iter()
                    .map(|t| self.ty(t, ctx))
                    .collect::<Result<_>>()?;
                let result = self.ty(result, ctx)?;
                TypeKind::Function(FnType {
                    params,
                    result,
                    unsafe_: *unsafe_ || *external,
                    abi: if *external { Abi::C } else { Abi::Biomiya },
                    effects: effects.iter().cloned().collect(),
                })
            }
            ast::TypeKind::Path(p) => {
                let path = p.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
                if p.len() == 1 {
                    if let Some(t) = ctx.generics.get(&p[0].name) {
                        if !p[0].arguments.is_empty() {
                            return Err(error(
                                "E_TYPE",
                                t_span(t, &self.types),
                                "type parameter takes no arguments",
                            ));
                        }
                        return Ok(*t);
                    }
                    if p[0].name == "Self" {
                        return ctx
                            .self_ty
                            .ok_or_else(|| error("E_NAME", t.span, "Self outside implementation"));
                    }
                    if let Some(i) = integer(&p[0].name) {
                        if !p[0].arguments.is_empty() {
                            return Err(error(
                                "E_TYPE",
                                t.span,
                                "primitive takes no type arguments",
                            ));
                        }
                        return Ok(self.int(i));
                    }
                    if p[0].name == "bool" {
                        return Ok(self.bool());
                    }
                }
                let id = self
                    .resolver
                    .lookup(&ctx.module, &path, Namespace::Type, t.span)?;
                let args = p
                    .last()
                    .unwrap()
                    .arguments
                    .iter()
                    .map(|t| self.ty(t, ctx))
                    .collect::<Result<Vec<_>>>()?;
                if let Some(id) = id {
                    let r = self.resolver.declarations[&id].clone();
                    match r.decl.kind {
                        ast::DeclKind::Alias(a) => {
                            if !args.is_empty() {
                                return Err(error("E_TYPE", t.span, "alias takes no arguments"));
                            }
                            if !self.aliases.insert(id) {
                                return Err(error("E_TYPE", t.span, "circular type alias"));
                            }
                            let mut aliasctx = ctx.clone();
                            aliasctx.module = r.module;
                            let ty = self.ty(&a, &mut aliasctx)?;
                            self.aliases.remove(&id);
                            return Ok(ty);
                        }
                        ast::DeclKind::Struct { generics, .. }
                        | ast::DeclKind::Enum { generics, .. } => {
                            if args.len() != generics.len() {
                                return Err(error(
                                    "E_TYPE",
                                    t.span,
                                    "nominal type argument count mismatch",
                                ));
                            }
                            for (g, a) in generics.iter().zip(&args) {
                                if g.copy
                                    && !matches!(self.types.kind(*a), TypeKind::Param(_))
                                    && !self.types.properties(*a).copy
                                {
                                    return Err(error(
                                        "E_INSTANTIATION",
                                        t.span,
                                        "Copy constraint failed",
                                    ));
                                }
                            }
                            TypeKind::Nominal(id, args)
                        }
                        _ => return Err(error("E_NAME", t.span, "not a type")),
                    }
                } else {
                    let builtin = self
                        .resolver
                        .builtin_path(&ctx.module, &path, Namespace::Type, t.span)?
                        .ok_or_else(|| error("E_NAME", t.span, "unknown type"))?;
                    let name = builtin.last().unwrap();
                    match (name.as_str(), args.as_slice()) {
                        ("Option", [a]) => TypeKind::Option(*a),
                        ("Result", [a, b]) => TypeKind::Result(*a, *b),
                        ("MaybeUninit", [a]) => TypeKind::MaybeUninit(*a),
                        ("UnsafeCell", [a]) => TypeKind::UnsafeCell(*a),
                        ("Atomic", [a]) => {
                            if !matches!(
                                self.types.kind(*a),
                                TypeKind::Int(
                                    Integer::U8
                                        | Integer::U16
                                        | Integer::U32
                                        | Integer::U64
                                        | Integer::Usize
                                ) | TypeKind::Pointer {
                                    space: AddressSpace::Kernel,
                                    ..
                                } | TypeKind::Param(_)
                            ) {
                                return Err(error(
                                    "E_TYPE",
                                    t.span,
                                    "Atomic requires scalar integer or bool",
                                ));
                            }
                            TypeKind::Atomic(*a)
                        }
                        ("TrapFrame", []) => TypeKind::Nominal(DefId(0xffff_ff01), vec![]),
                        ("MemoryOrder", []) => TypeKind::Nominal(DefId(0xffff_ff00), vec![]),
                        _ => return Err(error("E_TYPE", t.span, "builtin type argument mismatch")),
                    }
                }
            }
        };
        match &k {
            TypeKind::Ref { target, .. }
            | TypeKind::Pointer { target, .. }
            | TypeKind::Slice {
                element: target, ..
            }
            | TypeKind::Array(target, _)
            | TypeKind::ArrayExpr(target, _) => self.object_type(*target, t.span)?,
            TypeKind::Tuple(v) | TypeKind::Nominal(_, v) => {
                for t in v {
                    self.object_type(*t, Span::default())?
                }
            }
            TypeKind::Function(f) => {
                for t in &f.params {
                    self.object_type(*t, Span::default())?
                }
            }
            _ => {}
        }
        Ok(self.types.intern(k))
    }
}
fn t_span(_: &TyId, _: &TypeContext) -> Span {
    Span::default()
}
fn integer(name: &str) -> Option<Integer> {
    Some(match name {
        "u8" => Integer::U8,
        "u16" => Integer::U16,
        "u32" => Integer::U32,
        "u64" => Integer::U64,
        "usize" => Integer::Usize,
        "i8" => Integer::I8,
        "i16" => Integer::I16,
        "i32" => Integer::I32,
        "i64" => Integer::I64,
        "isize" => Integer::Isize,
        _ => return None,
    })
}
