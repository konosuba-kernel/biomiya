//! Replay original dependent length expressions in their concrete definition context.
use super::*;
use std::collections::HashSet;

type Registration = (Span, Vec<String>, Vec<(String, TyId)>, Option<TyId>);
#[derive(Clone)]
struct Retained {
    expression: ast::Expr,
    context: Context,
    bindings: Vec<(String, TyId)>,
    queries: Vec<TyId>,
}
#[derive(Default)]
pub(super) struct State {
    registrations: HashMap<Registration, ConstExprId>,
    expressions: HashMap<ConstExprId, Retained>,
    active_queries: HashSet<(ConstExprId, Vec<TyId>)>,
    active_types: HashSet<TyId>,
    prepared_types: HashSet<TyId>,
}
impl Checker {
    pub(super) fn deferred_array_length(
        &mut self,
        e: &ast::Expr,
        ctx: &mut Context,
    ) -> Result<ConstLen> {
        let bindings = ctx
            .generics
            .iter()
            .map(|(n, t)| (n.clone(), *t))
            .collect::<Vec<_>>();
        let key = (e.span, ctx.module.clone(), bindings.clone(), ctx.self_ty);
        if let Some(id) = self.deferred.registrations.get(&key).copied() {
            let old = &self.deferred.expressions[&id];
            return Ok(ConstLen {
                integer: Integer::Usize,
                kind: ConstLenKind::Deferred {
                    id,
                    args: bindings.iter().map(|(_, t)| *t).collect(),
                    queries: old.queries.clone(),
                },
            });
        }
        let names = ctx
            .scopes
            .iter()
            .flat_map(|s| s.keys().cloned())
            .collect::<BTreeSet<_>>();
        let generic_names = ctx.generics.keys().cloned().collect::<BTreeSet<_>>();
        self.name_expr(e, &ctx.module, &names, &generic_names)?;
        let id =
            ConstExprId(u32::try_from(self.deferred.expressions.len()).map_err(|_| {
                error("I_LIMIT", e.span, "too many dependent constant expressions")
            })?);
        self.deferred.registrations.insert(key, id);
        self.deferred.expressions.insert(
            id,
            Retained {
                expression: e.clone(),
                context: ctx.clone(),
                bindings: bindings.clone(),
                queries: vec![],
            },
        );
        let typed = self.check_const_template(e, ctx)?;
        let mut queries = vec![];
        self.collect_typed_constructors(&typed, &mut queries);
        self.collect_deferred_queries(e, ctx, &mut queries)?;
        queries.sort_unstable();
        queries.dedup();
        self.deferred.expressions.get_mut(&id).unwrap().queries = queries.clone();
        Ok(ConstLen {
            integer: Integer::Usize,
            kind: ConstLenKind::Deferred {
                id,
                args: bindings.into_iter().map(|(_, t)| t).collect(),
                queries,
            },
        })
    }
    /// This prepares representation requirements; references, pointers and function
    /// values have fixed representations and stop layout dependency traversal.
    pub(super) fn prepare_type(&mut self, t: TyId) -> Result<()> {
        if self.has_params(t) || self.deferred.prepared_types.contains(&t) {
            return Ok(());
        }
        if let TypeKind::Nominal(id, _) = self.types.kind(t) {
            self.ensure_type_definition(*id)?;
        }
        crate::layout::LayoutEngine::new(&mut self.types).validate_dependencies(t)?;
        if !self.deferred.active_types.insert(t) {
            return Ok(());
        }
        let result = self.prepare_type_inner(t).map_err(|d| {
            if let TypeKind::Nominal(id, args) = self.types.kind(t) {
                let name = self
                    .types
                    .definition(*id)
                    .map(|d| d.name.as_str())
                    .unwrap_or("<unknown>");
                let args = args
                    .iter()
                    .map(|t| self.types.display(*t))
                    .collect::<Vec<_>>()
                    .join(", ");
                d.note(None, format!("while preparing {name}<{args}>"))
            } else {
                d
            }
        });
        self.deferred.active_types.remove(&t);
        if result.is_ok() {
            self.deferred.prepared_types.insert(t);
        }
        result
    }
    fn prepare_type_inner(&mut self, t: TyId) -> Result<()> {
        match self.types.kind(t).clone() {
            TypeKind::ArrayExpr(element, len) => {
                self.prepare_length(&len)?;
                self.prepare_type(element)?;
            }
            TypeKind::Array(element, _)
            | TypeKind::Option(element)
            | TypeKind::MaybeUninit(element)
            | TypeKind::UnsafeCell(element)
            | TypeKind::Atomic(element) => self.prepare_type(element)?,
            TypeKind::Result(a, b) => {
                self.prepare_type(a)?;
                self.prepare_type(b)?;
            }
            TypeKind::Tuple(fields) => {
                for field in fields {
                    self.prepare_type(field)?;
                }
            }
            TypeKind::Nominal(id, args) => {
                self.ensure_type_definition(id)?;
                let def =
                    self.types.definition(id).cloned().ok_or_else(|| {
                        error("E_LAYOUT", Span::default(), "undefined nominal type")
                    })?;
                for field in def.fields() {
                    let ty = self.types.substitute(field.ty, &args);
                    self.prepare_type(ty)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn prepare_length(&mut self, len: &ConstLen) -> Result<()> {
        use ConstLenKind::*;
        match &len.kind {
            Deferred { id, args, .. } => {
                if args.iter().any(|t| self.has_params(*t)) {
                    return Ok(());
                }
                if self.types.cached_const(*id, args).is_some() {
                    return Ok(());
                }
                let old = self.deferred.expressions.get(id).cloned().ok_or_else(|| {
                    error(
                        "I_BUG",
                        Span::default(),
                        "missing original dependent constant expression",
                    )
                })?;
                if old.bindings.len() != args.len() {
                    return Err(error(
                        "I_BUG",
                        old.expression.span,
                        "dependent constant binding count mismatch",
                    ));
                }
                let key = (*id, args.clone());
                if !self.deferred.active_queries.insert(key.clone()) {
                    return Err(error(
                        "E_LAYOUT",
                        old.expression.span,
                        "cyclic dependent constant evaluation",
                    ));
                }
                let result = (|| {
                    let mut substitutions = vec![];
                    for ((_, template), actual) in old.bindings.iter().zip(args) {
                        if let TypeKind::Param(index) = self.types.kind(*template) {
                            let index = *index as usize;
                            while substitutions.len() <= index {
                                let p = self
                                    .types
                                    .intern(TypeKind::Param(substitutions.len() as u32));
                                substitutions.push(p);
                            }
                            substitutions[index] = *actual;
                        }
                    }
                    let mut context = old.context.clone();
                    for (name, template) in &old.bindings {
                        let concrete = self.types.substitute(*template, &substitutions);
                        context.generics.insert(name.clone(), concrete);
                    }
                    context.self_ty = context
                        .self_ty
                        .map(|t| self.types.substitute(t, &substitutions));
                    context.result = self.types.substitute(context.result, &substitutions);
                    for local in &mut context.locals {
                        local.ty = self.types.substitute(local.ty, &substitutions);
                    }
                    context.template = false;
                    context.inference_hints.clear();
                    let value = self.const_ast(&old.expression, &mut context).map_err(|d| {
                        d.note(
                            Some(old.expression.span),
                            format!(
                                "while evaluating dependent query {} with type arguments [{}]",
                                id.0,
                                args.iter()
                                    .map(|t| self.types.display(*t))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        )
                    })?;
                    self.types.cache_const(*id, args, value);
                    Ok(())
                })();
                self.deferred.active_queries.remove(&key);
                result?;
            }
            SizeOf(t) | AlignOf(t) | OffsetOf(t, _) => self.prepare_type(*t)?,
            Unary(_, x) | Cast(x) | CastFits { value: x, .. } => self.prepare_length(x)?,
            Binary(_, a, b)
            | Assert(a, b)
            | CheckedFits {
                left: a, right: b, ..
            } => {
                self.prepare_length(a)?;
                self.prepare_length(b)?;
            }
            If(c, a, b) => {
                self.prepare_length(c)?;
                self.prepare_length(a)?;
                self.prepare_length(b)?;
            }
            Literal(_) | Panic => {}
        }
        Ok(())
    }
    fn collect_typed_constructors(&self, e: &h::Expr, out: &mut Vec<TyId>) {
        use h::ExprKind::*;
        match &e.kind {
            Aggregate { fields, .. } => {
                out.push(e.ty);
                for (_, field) in fields {
                    self.collect_typed_constructors(field, out);
                }
            }
            Unary { value, .. }
            | Cast(value)
            | Adapt(value)
            | Deref(value)
            | RawAddress { value, .. }
            | Borrow { value, .. } => self.collect_typed_constructors(value, out),
            Binary { left, right, .. }
            | Assign {
                place: left,
                value: right,
                ..
            } => {
                self.collect_typed_constructors(left, out);
                self.collect_typed_constructors(right, out);
            }
            Array(fields) => {
                for field in fields {
                    self.collect_typed_constructors(field, out);
                }
            }
            Repeat { value, .. } | Field { base: value, .. } => {
                self.collect_typed_constructors(value, out)
            }
            Index { base, index } => {
                self.collect_typed_constructors(base, out);
                self.collect_typed_constructors(index, out);
            }
            Slice { base, start, end } => {
                self.collect_typed_constructors(base, out);
                self.collect_typed_constructors(start, out);
                self.collect_typed_constructors(end, out);
            }
            If {
                condition,
                then,
                else_,
            } => {
                self.collect_typed_constructors(condition, out);
                self.collect_typed_constructors(then, out);
                if let Some(other) = else_ {
                    self.collect_typed_constructors(other, out);
                }
            }
            Intrinsic { arguments, .. } => {
                if matches!(
                    self.types.kind(e.ty),
                    TypeKind::MaybeUninit(_) | TypeKind::UnsafeCell(_) | TypeKind::Atomic(_)
                ) {
                    out.push(e.ty);
                }
                for argument in arguments {
                    self.collect_typed_constructors(argument, out);
                }
            }
            Call {
                callee, arguments, ..
            } => {
                self.collect_typed_constructors(callee, out);
                for argument in arguments {
                    self.collect_typed_constructors(argument, out);
                }
            }
            Block {
                statements, tail, ..
            } => {
                for statement in statements {
                    match statement {
                        h::Statement::Expr(value)
                        | h::Statement::Bind { value, .. }
                        | h::Statement::Defer {
                            expression: value, ..
                        } => self.collect_typed_constructors(value, out),
                    }
                }
                if let Some(tail) = tail {
                    self.collect_typed_constructors(tail, out);
                }
            }
            Match { value, arms } => {
                self.collect_typed_constructors(value, out);
                for (_, arm) in arms {
                    self.collect_typed_constructors(arm, out);
                }
            }
            Unsafe { body, .. } | Try(body) | Loop { body, .. } => {
                self.collect_typed_constructors(body, out)
            }
            _ => {}
        }
    }
    fn collect_deferred_queries(
        &mut self,
        e: &ast::Expr,
        ctx: &mut Context,
        out: &mut Vec<TyId>,
    ) -> Result<()> {
        self.collect_query_expr(e, ctx, out, true)
    }
    fn collect_query_expr(
        &mut self,
        e: &ast::Expr,
        ctx: &mut Context,
        out: &mut Vec<TyId>,
        layout: bool,
    ) -> Result<()> {
        use ast::ExprKind::*;
        match &e.kind {
            Layout { ty, .. } => {
                let t = self.ty(ty, ctx)?;
                if layout {
                    out.push(t);
                }
            }
            Path(path) => self.collect_query_path(path, ctx, out, layout)?,
            Tuple(values) | Array(values) => {
                for value in values {
                    self.collect_query_expr(value, ctx, out, layout)?;
                }
            }
            Repeat { value, length } => {
                self.collect_query_expr(value, ctx, out, layout)?;
                self.collect_query_expr(length, ctx, out, layout)?;
            }
            Record { path, fields } => {
                self.collect_constructor_layout(path, e.span, ctx, out, layout)?;
                self.collect_query_path(path, ctx, out, layout)?;
                for (_, value) in fields {
                    self.collect_query_expr(value, ctx, out, layout)?;
                }
            }
            Cast { value, ty } => {
                self.collect_query_expr(value, ctx, out, layout)?;
                self.collect_query_type(ty, ctx, out, layout)?;
            }
            Field {
                base, arguments, ..
            } => {
                self.collect_query_expr(base, ctx, out, layout)?;
                for ty in arguments {
                    self.collect_query_type(ty, ctx, out, layout)?;
                }
            }
            Unary { operand, .. } | Try(operand) => {
                self.collect_query_expr(operand, ctx, out, layout)?
            }
            Binary { left, right, .. }
            | Index {
                base: left,
                index: right,
            } => {
                self.collect_query_expr(left, ctx, out, layout)?;
                self.collect_query_expr(right, ctx, out, layout)?;
            }
            Call { callee, arguments } => {
                self.collect_query_expr(callee, ctx, out, layout)?;
                for argument in arguments {
                    self.collect_query_expr(argument, ctx, out, layout)?;
                }
            }
            Block(block) | Unsafe(block) | Loop(block) => {
                self.collect_query_block(block, ctx, out, layout)?
            }
            If {
                condition,
                then,
                else_,
            } => {
                self.collect_query_expr(condition, ctx, out, layout)?;
                self.collect_query_block(then, ctx, out, layout)?;
                if let Some(other) = else_ {
                    self.collect_query_expr(other, ctx, out, layout)?;
                }
            }
            Match { value, arms } => {
                self.collect_query_expr(value, ctx, out, layout)?;
                for (_, arm) in arms {
                    self.collect_query_expr(arm, ctx, out, layout)?;
                }
            }
            While { condition, body } => {
                self.collect_query_expr(condition, ctx, out, layout)?;
                self.collect_query_block(body, ctx, out, layout)?;
            }
            For { input, body, .. } => {
                self.collect_query_expr(input, ctx, out, layout)?;
                self.collect_query_block(body, ctx, out, layout)?;
            }
            Return(value) | Break(value) => {
                if let Some(value) = value {
                    self.collect_query_expr(value, ctx, out, layout)?;
                }
            }
            Asm { operands, .. } => {
                for operand in operands {
                    if let Some(value) = &operand.value {
                        self.collect_query_expr(value, ctx, out, layout)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
    /// Constructors consume the aggregate representation, unlike a function
    /// address. Their types therefore enter the same declaration dependency graph
    /// as explicit layout queries, including constructors in unselected branches.
    fn collect_constructor_layout(
        &mut self,
        path: &[ast::PathSegment],
        span: Span,
        ctx: &mut Context,
        out: &mut Vec<TyId>,
        layout: bool,
    ) -> Result<()> {
        if !layout || path.len() == 1 && ctx.local(&path[0].name).is_some() {
            return Ok(());
        }
        let names = path.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
        let owner = self
            .resolver
            .lookup(&ctx.module, &names, Namespace::Value, span)?
            .or(self
                .resolver
                .lookup(&ctx.module, &names, Namespace::Type, span)?);
        let Some(id) = owner else {
            return Ok(());
        };
        let count = match &self.resolver.declarations[&id].decl.kind {
            ast::DeclKind::Struct { generics, .. } | ast::DeclKind::Enum { generics, .. } => {
                generics.len()
            }
            _ => return Ok(()),
        };
        let written = path.iter().flat_map(|p| &p.arguments).collect::<Vec<_>>();
        if written.len() != count {
            return Ok(());
        }
        let args = written
            .into_iter()
            .map(|t| self.ty(t, ctx))
            .collect::<Result<Vec<_>>>()?;
        out.push(self.types.intern(TypeKind::Nominal(id, args)));
        Ok(())
    }
    fn collect_query_path(
        &mut self,
        path: &[ast::PathSegment],
        ctx: &mut Context,
        out: &mut Vec<TyId>,
        layout: bool,
    ) -> Result<()> {
        for segment in path {
            for ty in &segment.arguments {
                self.collect_query_type(ty, ctx, out, layout)?;
            }
        }
        Ok(())
    }
    fn collect_query_type(
        &mut self,
        ty: &ast::Type,
        ctx: &mut Context,
        out: &mut Vec<TyId>,
        layout: bool,
    ) -> Result<()> {
        use ast::TypeKind::*;
        match &ty.kind {
            Path(path) => self.collect_query_path(path, ctx, out, layout)?,
            Array { length, inner } => {
                self.collect_query_expr(length, ctx, out, layout)?;
                self.collect_query_type(inner, ctx, out, layout)?;
            }
            Tuple(fields) => {
                for field in fields {
                    self.collect_query_type(field, ctx, out, layout)?;
                }
            }
            Ref { inner, .. } | Pointer { inner, .. } | Slice { inner, .. } => {
                self.collect_query_type(inner, ctx, out, false)?
            }
            Function {
                parameters, result, ..
            } => {
                for parameter in parameters {
                    self.collect_query_type(parameter, ctx, out, false)?;
                }
                self.collect_query_type(result, ctx, out, false)?;
            }
            Never => {}
        }
        Ok(())
    }
    fn collect_query_block(
        &mut self,
        b: &ast::Block,
        ctx: &mut Context,
        out: &mut Vec<TyId>,
        layout: bool,
    ) -> Result<()> {
        for statement in &b.statements {
            match statement {
                ast::Statement::Binding { ty, value, .. } => {
                    if let Some(ty) = ty {
                        self.collect_query_type(ty, ctx, out, layout)?;
                    }
                    self.collect_query_expr(value, ctx, out, layout)?;
                }
                ast::Statement::Expr(value)
                | ast::Statement::Assert(value)
                | ast::Statement::Defer(value) => {
                    self.collect_query_expr(value, ctx, out, layout)?
                }
                _ => {}
            }
        }
        if let Some(tail) = &b.tail {
            self.collect_query_expr(tail, ctx, out, layout)?;
        }
        Ok(())
    }
}
