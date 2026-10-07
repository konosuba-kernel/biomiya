//! Local structural constraints. Literal defaulting happens after peers and
//! generic signatures have contributed their constraints. Completed local binding
//! initializers default independently; their types become hard constraints for
//! later expressions. Deferred field projections settle before defaulting.
//! Contextual adaptations are confined to explicitly expected types: inferred
//! peers unify their original qualifiers and preserve function effect metadata.
use super::*;
#[derive(Clone, Debug)]
enum Term {
    Var(usize),
    Known(TyId),
    Form(Head, Vec<Term>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Head {
    Tuple,
    Array(u64),
    Nominal(DefId),
    Ref(bool),
    Pointer(AddressSpace, bool),
    Slice(bool),
    Option,
    Result,
    MaybeUninit,
    UnsafeCell,
    Atomic,
    Range,
    Function(Abi, bool, BTreeSet<String>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Any,
    Integer,
    Pointer,
}
struct Variable {
    class: Class,
    bound: Option<Term>,
    literal: bool,
}
struct Projection {
    base: Term,
    result: Term,
    name: String,
    span: Span,
}
struct Constraints<'a> {
    checker: &'a mut Checker,
    vars: Vec<Variable>,
    nodes: Vec<(Span, Term)>,
    projections: Vec<Projection>,
    breaks: Vec<Term>,
}
impl Checker {
    pub(super) fn hint(&mut self, e: &ast::Expr, ctx: &mut Context) -> Result<Option<TyId>> {
        if ctx.const_context && matches!(e.kind, ast::ExprKind::Block(_) | ast::ExprKind::Unsafe(_))
        {
            return Ok(None);
        }
        if let Some(t) = ctx.inference_hints.get(&e.span) {
            return Ok(Some(*t));
        }
        if ctx.template {
            return self.template_hint(e, ctx);
        }
        self.prepare_inference(e, None, ctx)?;
        Ok(ctx.inference_hints.get(&e.span).copied())
    }
    pub(super) fn prepare_inference(
        &mut self,
        e: &ast::Expr,
        expected: Option<TyId>,
        ctx: &mut Context,
    ) -> Result<()> {
        if ctx.template
            || ctx.inference_hints.contains_key(&e.span)
            || ctx.const_context
                && matches!(e.kind, ast::ExprKind::Block(_) | ast::ExprKind::Unsafe(_))
        {
            return Ok(());
        }
        let mut solver = Constraints {
            checker: self,
            vars: vec![],
            nodes: vec![],
            projections: vec![],
            breaks: vec![],
        };
        let mut locals = BTreeMap::new();
        for scope in &ctx.scopes {
            for (name, id) in scope {
                locals.insert(name.clone(), Term::Known(ctx.locals[id.0 as usize].ty));
            }
        }
        solver.collect(e, expected.map(Term::Known), ctx, &mut locals)?;
        solver.finish()?;
        for (span, term) in solver.nodes.clone() {
            if let Some(ty) = solver.materialize(&term) {
                ctx.inference_hints.insert(span, ty);
            }
        }
        Ok(())
    }
    fn template_hint(&mut self, e: &ast::Expr, ctx: &mut Context) -> Result<Option<TyId>> {
        Ok(match &e.kind {
            ast::ExprKind::Integer(s) => super::expressions::literal(s, e.span)?
                .1
                .map(|i| self.int(i)),
            ast::ExprKind::Bool(_) => Some(self.bool()),
            ast::ExprKind::Path(p) if p.len() == 1 => {
                ctx.local(&p[0].name).map(|id| ctx.locals[id.0 as usize].ty)
            }
            ast::ExprKind::Cast { ty, .. } => Some(self.ty(ty, ctx)?),
            _ => None,
        })
    }
}
impl Constraints<'_> {
    fn fresh(&mut self, class: Class) -> Term {
        let id = self.vars.len();
        self.vars.push(Variable {
            class,
            bound: None,
            literal: class == Class::Integer,
        });
        Term::Var(id)
    }
    fn prune(&self, t: &Term) -> Term {
        match t {
            Term::Var(id) => self.vars[*id]
                .bound
                .as_ref()
                .map(|t| self.prune(t))
                .unwrap_or_else(|| t.clone()),
            _ => t.clone(),
        }
    }
    fn occurs(&self, id: usize, t: &Term) -> bool {
        match self.prune(t) {
            Term::Var(v) => id == v,
            Term::Form(_, v) => v.iter().any(|t| self.occurs(id, t)),
            _ => false,
        }
    }
    fn known(&mut self, k: TypeKind) -> Term {
        Term::Known(self.checker.types.intern(k))
    }
    fn form(&self, t: &Term) -> Term {
        match self.prune(t) {
            Term::Known(id) => {
                use TypeKind::*;
                match self.checker.types.kind(id).clone() {
                    Tuple(v) => Term::Form(Head::Tuple, v.into_iter().map(Term::Known).collect()),
                    Array(t, n) => Term::Form(Head::Array(n), vec![Term::Known(t)]),
                    Nominal(id, v) => {
                        Term::Form(Head::Nominal(id), v.into_iter().map(Term::Known).collect())
                    }
                    Ref { target, mutable } => {
                        Term::Form(Head::Ref(mutable), vec![Term::Known(target)])
                    }
                    Pointer {
                        target,
                        space,
                        const_,
                    } => Term::Form(Head::Pointer(space, const_), vec![Term::Known(target)]),
                    Slice { element, const_ } => {
                        Term::Form(Head::Slice(const_), vec![Term::Known(element)])
                    }
                    Option(t) => Term::Form(Head::Option, vec![Term::Known(t)]),
                    Result(t, e) => Term::Form(Head::Result, vec![Term::Known(t), Term::Known(e)]),
                    MaybeUninit(t) => Term::Form(Head::MaybeUninit, vec![Term::Known(t)]),
                    UnsafeCell(t) => Term::Form(Head::UnsafeCell, vec![Term::Known(t)]),
                    Atomic(t) => Term::Form(Head::Atomic, vec![Term::Known(t)]),
                    Range(t) => Term::Form(Head::Range, vec![Term::Known(t)]),
                    Function(f) => {
                        let mut v = f.params.into_iter().map(Term::Known).collect::<Vec<_>>();
                        v.push(Term::Known(f.result));
                        Term::Form(Head::Function(f.abi, f.unsafe_, f.effects), v)
                    }
                    _ => Term::Known(id),
                }
            }
            t => t,
        }
    }
    fn substitute(&mut self, t: TyId, params: &[Term]) -> Term {
        if let Some(args) = params
            .iter()
            .map(|p| self.materialize(p))
            .collect::<std::option::Option<Vec<_>>>()
            && !args.iter().any(|t| self.checker.has_params(*t))
        {
            return Term::Known(self.checker.types.substitute(t, &args));
        }
        use TypeKind::*;
        match self.checker.types.kind(t).clone() {
            Param(n) => params.get(n as usize).cloned().unwrap_or(Term::Known(t)),
            _ => match self.form(&Term::Known(t)) {
                Term::Form(h, v) => Term::Form(
                    h,
                    v.into_iter()
                        .map(|t| match t {
                            Term::Known(t) => self.substitute(t, params),
                            t => t,
                        })
                        .collect(),
                ),
                t => t,
            },
        }
    }
    fn unify(&mut self, a: &Term, b: &Term, span: Span) -> Result<()> {
        let a = self.prune(a);
        let b = self.prune(b);
        if self.never(&a) || self.never(&b) {
            return Ok(());
        }
        match (a, b) {
            (Term::Var(a), Term::Var(b)) if a == b => Ok(()),
            (Term::Var(id), t) | (t, Term::Var(id)) => {
                if self.occurs(id, &t) {
                    return Err(error("E_TYPE", span, "recursive inferred type"));
                }
                let class = self.vars[id].class;
                if let Term::Var(other) = &t {
                    let combined = match (class, self.vars[*other].class) {
                        (Class::Any, c) | (c, Class::Any) => c,
                        (a, b) if a == b => a,
                        _ => {
                            return Err(error(
                                "E_TYPE",
                                span,
                                "incompatible inference constraints",
                            ));
                        }
                    };
                    self.vars[*other].class = combined;
                } else if class != Class::Any {
                    let valid = match (class, self.form(&t)) {
                        (Class::Integer, Term::Known(t)) => {
                            matches!(self.checker.types.kind(t), TypeKind::Int(_))
                        }
                        (Class::Pointer, Term::Form(Head::Pointer(..), _)) => true,
                        _ => false,
                    };
                    if !valid {
                        return Err(error(
                            "E_TYPE",
                            span,
                            "literal type is incompatible with context",
                        ));
                    }
                }
                self.vars[id].bound = Some(t);
                Ok(())
            }
            (Term::Known(a), Term::Known(b)) if self.checker.types.core_compatible(a, b) => Ok(()),
            (a, b) => match (self.form(&a), self.form(&b)) {
                (Term::Form(a, x), Term::Form(b, y))
                    if Self::same_head(&a, &b) && x.len() == y.len() =>
                {
                    for (a, b) in x.iter().zip(&y) {
                        self.unify(a, b, span)?
                    }
                    Ok(())
                }
                _ => Err(error(
                    "E_TYPE",
                    span,
                    "incompatible structural type constraints",
                )),
            },
        }
    }
    fn same_head(a: &Head, b: &Head) -> bool {
        match (a, b) {
            (Head::Function(a, x, _), Head::Function(b, y, _)) => a == b && x == y,
            _ => a == b,
        }
    }
    fn never(&self, t: &Term) -> bool {
        matches!(self.prune(t),Term::Known(t) if matches!(self.checker.types.kind(t),TypeKind::Never))
    }
    fn contextual(&mut self, actual: &Term, wanted: &Term, span: Span) -> Result<Term> {
        let a = self.form(actual);
        let b = self.form(wanted);
        let adaptation = matches!(
            (&a, &b),
            (
                Term::Form(Head::Ref(true), _),
                Term::Form(Head::Ref(false), _)
            ) | (
                Term::Form(Head::Slice(false), _),
                Term::Form(Head::Slice(true), _)
            )
        ) || matches!((&a,&b),(Term::Form(Head::Pointer(x,false),_),Term::Form(Head::Pointer(y,true),_))if x==y)
            || matches!((&a,&b),(Term::Form(Head::Function(x,false,_),_),Term::Form(Head::Function(y,true,_),_))if x==y);
        if adaptation {
            if let (Term::Form(_, x), Term::Form(_, y)) = (a, b) {
                for (a, b) in x.iter().zip(&y) {
                    self.unify(a, b, span)?
                }
            }
        } else {
            self.unify(actual, wanted, span)?
        }
        Ok(wanted.clone())
    }
    fn materialize(&mut self, t: &Term) -> Option<TyId> {
        match self.prune(t) {
            Term::Known(t) => Some(t),
            Term::Var(_) => None,
            Term::Form(h, v) => {
                let v = v
                    .iter()
                    .map(|t| self.materialize(t))
                    .collect::<Option<Vec<_>>>()?;
                let k = match h {
                    Head::Tuple => {
                        if v.is_empty() {
                            TypeKind::Unit
                        } else {
                            TypeKind::Tuple(v)
                        }
                    }
                    Head::Array(n) => TypeKind::Array(v[0], n),
                    Head::Nominal(d) => TypeKind::Nominal(d, v),
                    Head::Ref(mutable) => TypeKind::Ref {
                        target: v[0],
                        mutable,
                    },
                    Head::Pointer(space, const_) => TypeKind::Pointer {
                        target: v[0],
                        space,
                        const_,
                    },
                    Head::Slice(const_) => TypeKind::Slice {
                        element: v[0],
                        const_,
                    },
                    Head::Option => TypeKind::Option(v[0]),
                    Head::Result => TypeKind::Result(v[0], v[1]),
                    Head::MaybeUninit => TypeKind::MaybeUninit(v[0]),
                    Head::UnsafeCell => TypeKind::UnsafeCell(v[0]),
                    Head::Atomic => TypeKind::Atomic(v[0]),
                    Head::Range => TypeKind::Range(v[0]),
                    Head::Function(abi, unsafe_, effects) => TypeKind::Function(FnType {
                        params: v[..v.len() - 1].to_vec(),
                        result: *v.last()?,
                        abi,
                        unsafe_,
                        effects,
                    }),
                };
                Some(self.checker.types.intern(k))
            }
        }
    }
    fn finish(&mut self) -> Result<()> {
        self.finish_from(0)
    }
    fn finish_from(&mut self, start: usize) -> Result<()> {
        self.solve_projections()?;
        for id in start..self.vars.len() {
            if self.vars[id].literal
                && self.vars[id].class == Class::Integer
                && matches!(self.prune(&Term::Var(id)), Term::Var(_))
            {
                let t = self.checker.int(Integer::I32);
                self.unify(&Term::Var(id), &Term::Known(t), Span::default())?;
            }
        }
        self.solve_projections()
    }
    fn solve_projections(&mut self) -> Result<()> {
        loop {
            let old = self.projections.len();
            let requests = std::mem::take(&mut self.projections);
            for p in requests {
                if let Some(t) = self.project(&p.base, &p.name, p.span)? {
                    self.unify(&p.result, &t, p.span)?
                } else {
                    self.projections.push(p)
                }
            }
            if self.projections.len() == old {
                return Ok(());
            }
        }
    }
    fn project(&mut self, base: &Term, name: &str, span: Span) -> Result<Option<Term>> {
        let mut t = self.form(base);
        while let Term::Form(Head::Ref(_), v) = t {
            t = self.form(&v[0]);
        }
        Ok(match t {
            Term::Var(_) => None,
            Term::Form(Head::Array(_) | Head::Slice(_), v) => {
                if name == "len" {
                    Some(Term::Known(self.checker.int(crate::types::Integer::Usize)))
                } else if name == "[]" {
                    Some(v[0].clone())
                } else {
                    None
                }
            }
            Term::Form(Head::Tuple, v) => {
                name.parse::<usize>().ok().and_then(|n| v.get(n).cloned())
            }
            Term::Form(Head::Nominal(d), args) => {
                let def = self
                    .checker
                    .types
                    .definition(d)
                    .cloned()
                    .ok_or_else(|| error("E_NAME", span, "undefined nominal type"))?;
                match def.kind {
                    DefKind::Struct(fs) => fs
                        .into_iter()
                        .find(|f| f.name == name)
                        .map(|f| self.substitute(f.ty, &args)),
                    _ => None,
                }
            }
            _ => None,
        })
    }
    fn collect(
        &mut self,
        e: &ast::Expr,
        expected: Option<Term>,
        ctx: &mut Context,
        locals: &mut BTreeMap<String, Term>,
    ) -> Result<Term> {
        use ast::ExprKind::*;
        let span = e.span;
        let result = match &e.kind {
            Integer(s) => {
                if let Some(i) = super::expressions::literal(s, span)?.1 {
                    Term::Known(self.checker.int(i))
                } else {
                    self.fresh(Class::Integer)
                }
            }
            Bool(_) => Term::Known(self.checker.bool()),
            String(_) => {
                let u = self.checker.int(crate::types::Integer::U8);
                self.known(TypeKind::Slice {
                    element: u,
                    const_: true,
                })
            }
            Null => self.fresh(Class::Pointer),
            Path(p) => self.path(p, span, ctx, locals)?,
            Tuple(values) => {
                if values.is_empty() {
                    Term::Known(self.checker.unit())
                } else {
                    let targets = expected.as_ref().map(|t| self.form(t));
                    let mut terms = vec![];
                    for (n, value) in values.iter().enumerate() {
                        let want = match &targets {
                            Some(Term::Form(Head::Tuple, v)) => v.get(n).cloned(),
                            _ => None,
                        };
                        terms.push(self.collect(value, want, ctx, locals)?)
                    }
                    Term::Form(Head::Tuple, terms)
                }
            }
            Array(values) => {
                let element = self.fresh(Class::Any);
                if let Some(Term::Form(Head::Array(n), v)) = expected.as_ref().map(|t| self.form(t))
                {
                    if n != values.len() as u64 {
                        return Err(error("E_TYPE", span, "array length mismatch"));
                    }
                    self.unify(&element, &v[0], span)?;
                }
                for value in values {
                    let term = self.collect(value, Some(element.clone()), ctx, locals)?;
                    self.unify(&element, &term, value.span)?
                }
                Term::Form(Head::Array(values.len() as u64), vec![element])
            }
            Repeat { value, length } => {
                let n = self.checker.const_ast(length, ctx)?;
                let target = match expected.as_ref().map(|t| self.form(t)) {
                    Some(Term::Form(Head::Array(_), v)) => Some(v[0].clone()),
                    _ => None,
                };
                let value = self.collect(value, target, ctx, locals)?;
                Term::Form(Head::Array(n), vec![value])
            }
            Record { path, fields } => self.construct(
                path,
                fields.iter().map(|(n, e)| (Some(n.as_str()), e)).collect(),
                expected.clone(),
                span,
                ctx,
                locals,
            )?,
            Cast { value, ty } => {
                self.collect(value, None, ctx, locals)?;
                Term::Known(self.checker.ty(ty, ctx)?)
            }
            Layout { .. } => Term::Known(self.checker.int(crate::types::Integer::Usize)),
            Unary { op, operand, .. } => {
                let target = match expected.as_ref().map(|t| self.form(t)) {
                    Some(Term::Form(Head::Ref(_), v)) if op == "&" || op == "&mut" => {
                        Some(v[0].clone())
                    }
                    _ if ["-", "~"].contains(&op.as_str()) => expected.clone(),
                    _ => None,
                };
                let value = self.collect(operand, target, ctx, locals)?;
                match op.as_str() {
                    "&" | "&mut" => Term::Form(Head::Ref(op == "&mut"), vec![value]),
                    "*" => match self.form(&value) {
                        Term::Form(Head::Ref(_) | Head::Pointer(..), v) => v[0].clone(),
                        _ => self.fresh(Class::Any),
                    },
                    "!" => {
                        let b = Term::Known(self.checker.bool());
                        self.unify(&value, &b, span)?;
                        b
                    }
                    _ => value,
                }
            }
            Binary { op, left, right } => {
                let compare = matches!(op.as_str(), "==" | "!=" | "<" | "<=" | ">" | ">=");
                let logical = matches!(op.as_str(), "&&" | "||");
                let assignment = op.ends_with('=') && !compare && op != "..=";
                if assignment {
                    let a = self.collect(left, None, ctx, locals)?;
                    self.collect(right, Some(a), ctx, locals)?;
                    Term::Known(self.checker.unit())
                } else {
                    let target = if compare {
                        None
                    } else if logical {
                        Some(Term::Known(self.checker.bool()))
                    } else if op.ends_with('?') {
                        match expected.as_ref().map(|t| self.form(t)) {
                            Some(Term::Form(Head::Option, v)) => Some(v[0].clone()),
                            _ => None,
                        }
                    } else if op == ".." || op == "..=" {
                        None
                    } else {
                        expected.clone()
                    };
                    let a = self.collect(left, target.clone(), ctx, locals)?;
                    let b = self.collect(right, target, ctx, locals)?;
                    self.unify(&a, &b, span)?;
                    if compare || logical {
                        Term::Known(self.checker.bool())
                    } else if op.ends_with('?') {
                        Term::Form(Head::Option, vec![a])
                    } else if op == ".." || op == "..=" {
                        Term::Form(Head::Range, vec![a])
                    } else {
                        a
                    }
                }
            }
            If {
                condition,
                then,
                else_,
            } => {
                let b = Term::Known(self.checker.bool());
                self.collect(condition, Some(b), ctx, locals)?;
                let result = expected.clone().unwrap_or_else(|| self.fresh(Class::Any));
                let mut branch = locals.clone();
                let a = self.block(then, expected.clone(), ctx, &mut branch)?;
                if let Some(other) = else_ {
                    let mut branch = locals.clone();
                    let b = self.collect(other, expected.clone(), ctx, &mut branch)?;
                    self.unify(&result, &a, span)?;
                    self.unify(&result, &b, span)?;
                    result
                } else {
                    Term::Known(self.checker.unit())
                }
            }
            Match { value, arms } => {
                let subject = self.collect(value, None, ctx, locals)?;
                let (mut subject, borrow) = match self.form(&subject) {
                    Term::Form(Head::Ref(m), v) => (v[0].clone(), Some(m)),
                    t => (t, None),
                };
                let result = expected.clone().unwrap_or_else(|| self.fresh(Class::Any));
                for (pattern, arm) in arms {
                    let mut branch = locals.clone();
                    self.pattern(pattern, &mut subject, borrow, ctx, &mut branch)?;
                    let value = self.collect(arm, expected.clone(), ctx, &mut branch)?;
                    self.unify(&result, &value, arm.span)?;
                }
                result
            }
            Block(b) | Unsafe(b) => {
                let mut nested = locals.clone();
                self.block(b, expected.clone(), ctx, &mut nested)?
            }
            Loop(b) => {
                let result = expected.clone().unwrap_or_else(|| self.fresh(Class::Any));
                self.breaks.push(result.clone());
                let mut nested = locals.clone();
                self.block(b, None, ctx, &mut nested)?;
                self.breaks.pop();
                result
            }
            Break(value) => {
                if let Some(result) = self.breaks.last().cloned() {
                    let value = if let Some(value) = value {
                        self.collect(value, None, ctx, locals)?
                    } else {
                        Term::Known(self.checker.unit())
                    };
                    self.unify(&result, &value, span)?;
                }
                Term::Known(self.checker.types.intern(TypeKind::Never))
            }
            Return(value) => {
                if let Some(value) = value {
                    self.collect(value, Some(Term::Known(ctx.result)), ctx, locals)?;
                }
                Term::Known(self.checker.types.intern(TypeKind::Never))
            }
            Continue => Term::Known(self.checker.types.intern(TypeKind::Never)),
            While { condition, body } => {
                let b = Term::Known(self.checker.bool());
                self.collect(condition, Some(b), ctx, locals)?;
                let mut nested = locals.clone();
                let unit = Term::Known(self.checker.unit());
                self.breaks.push(unit.clone());
                self.block(body, None, ctx, &mut nested)?;
                self.breaks.pop();
                unit
            }
            For {
                binding,
                input,
                body,
                ..
            } => {
                let value = self.collect(input, None, ctx, locals)?;
                let element = match self.form(&value) {
                    Term::Form(Head::Range | Head::Array(_), v) => v[0].clone(),
                    Term::Form(Head::Slice(c), v) => Term::Form(Head::Ref(!c), v),
                    _ => self.fresh(Class::Any),
                };
                let mut nested = locals.clone();
                nested.insert(binding.clone(), element);
                let unit = Term::Known(self.checker.unit());
                self.breaks.push(unit.clone());
                self.block(body, None, ctx, &mut nested)?;
                self.breaks.pop();
                unit
            }
            Field { base, name, .. } => {
                let base = self.collect(base, None, ctx, locals)?;
                let result = self.fresh(Class::Any);
                self.projections.push(Projection {
                    base,
                    result: result.clone(),
                    name: name.clone(),
                    span,
                });
                result
            }
            Index { base, index } => {
                let base = self.collect(base, None, ctx, locals)?;
                self.collect(index, None, ctx, locals)?;
                if matches!(&index.kind,Binary{op,..}if op==".."||op=="..=") {
                    let element = self.fresh(Class::Any);
                    self.projections.push(Projection {
                        base,
                        result: element.clone(),
                        name: "[]".into(),
                        span,
                    });
                    Term::Form(Head::Slice(true), vec![element])
                } else {
                    let result = self.fresh(Class::Any);
                    self.projections.push(Projection {
                        base,
                        result: result.clone(),
                        name: "[]".into(),
                        span,
                    });
                    result
                }
            }
            Call { callee, arguments } => {
                self.call(callee, arguments, expected.clone(), span, ctx, locals)?
            }
            Try(value) => {
                let value = self.collect(value, None, ctx, locals)?;
                match self.form(&value) {
                    Term::Form(Head::Result, v) => v[0].clone(),
                    _ => self.fresh(Class::Any),
                }
            }
            Asm { .. } => Term::Known(self.checker.unit()),
        };
        self.nodes.push((span, result.clone()));
        if let Some(want) = expected {
            self.contextual(&result, &want, span)
        } else {
            Ok(result)
        }
    }
    fn block(
        &mut self,
        b: &ast::Block,
        expected: Option<Term>,
        ctx: &mut Context,
        locals: &mut BTreeMap<String, Term>,
    ) -> Result<Term> {
        if ctx.const_context {
            return self.const_prefix_block(b, expected, ctx, locals);
        }
        let mut last = None;
        for statement in &b.statements {
            match statement {
                ast::Statement::Binding {
                    pattern, ty, value, ..
                } => {
                    let boundary = self.vars.len();
                    let want = ty
                        .as_ref()
                        .map(|t| self.checker.ty(t, ctx).map(Term::Known))
                        .transpose()?;
                    let value = self.collect(value, want, ctx, locals)?;
                    self.finish_from(boundary)?;
                    let mut value = if let Some(t) = self.materialize(&value) {
                        Term::Known(t)
                    } else {
                        value
                    };
                    self.pattern(pattern, &mut value, None, ctx, locals)?;
                }
                ast::Statement::Expr(e) => {
                    last = Some(self.collect(e, None, ctx, locals)?);
                }
                ast::Statement::Defer(e) => {
                    self.collect(e, None, ctx, locals)?;
                }
                _ => {}
            }
        }
        let result = if let Some(tail) = &b.tail {
            self.collect(tail, expected, ctx, locals)?
        } else if last.as_ref().is_some_and(|t| self.never(t)) {
            self.known(TypeKind::Never)
        } else {
            Term::Known(self.checker.unit())
        };
        self.nodes.push((b.span, result.clone()));
        Ok(result)
    }
    /// Binding initializers are independent inference boundaries. Check each
    /// prefix once in a scoped constant context, then leave the tail's literal
    /// variables available for structural constraints from other branches.
    fn const_prefix_block(
        &mut self,
        b: &ast::Block,
        expected: Option<Term>,
        ctx: &mut Context,
        locals: &mut BTreeMap<String, Term>,
    ) -> Result<Term> {
        let mut prefix = ctx.clone();
        let scope = ScopeId(prefix.next_scope);
        prefix.next_scope += 1;
        prefix.scopes.push(BTreeMap::new());
        prefix.scope_ids.push(scope);
        let mut last = None;
        for statement in &b.statements {
            match statement {
                ast::Statement::Binding {
                    mutable,
                    pattern,
                    ty,
                    value,
                    ..
                } => {
                    let expected = ty
                        .as_ref()
                        .map(|t| self.checker.ty(t, &mut prefix))
                        .transpose()?;
                    let typed = self.checker.expr(value, expected, &mut prefix)?;
                    let bound =
                        self.checker
                            .pattern(pattern, typed.ty, *mutable, false, &mut prefix)?;
                    self.checker
                        .bind_constant(&bound, &typed, &mut prefix.const_env)?;
                    let mut t = Term::Known(typed.ty);
                    self.pattern(pattern, &mut t, None, &mut prefix, locals)?;
                    self.nodes.push((value.span, t));
                }
                ast::Statement::Expr(value) => {
                    last = Some(self.collect(value, None, &mut prefix, locals)?);
                }
                ast::Statement::Assert(_) | ast::Statement::Empty(_) => {}
                ast::Statement::Defer(value) => {
                    self.collect(value, None, &mut prefix, locals)?;
                }
            }
        }
        let result = if let Some(tail) = &b.tail {
            self.collect(tail, expected, &mut prefix, locals)?
        } else if last.as_ref().is_some_and(|t| self.never(t)) {
            self.known(TypeKind::Never)
        } else {
            Term::Known(self.checker.unit())
        };
        self.nodes.push((b.span, result.clone()));
        Ok(result)
    }
    fn path(
        &mut self,
        p: &[ast::PathSegment],
        span: Span,
        ctx: &mut Context,
        locals: &BTreeMap<String, Term>,
    ) -> Result<Term> {
        if p.len() == 1
            && let Some(t) = locals.get(&p[0].name)
        {
            return Ok(t.clone());
        }
        let path = p.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
        if let Some(id) =
            self.checker
                .resolver
                .lookup(&ctx.module, &path, Namespace::Value, span)?
        {
            let decl = self.checker.resolver.declarations[&id].clone();
            return match decl.decl.kind {
                ast::DeclKind::Const { .. } | ast::DeclKind::Static { .. } => {
                    Ok(Term::Known(self.checker.declaration_type(id)?))
                }
                ast::DeclKind::Function(f) => {
                    let n = f.generics.len() + decl.impl_generics.len();
                    let params = self.parameters(p, n, ctx)?;
                    let signature = self.checker.ensure_signature(id)?;
                    let t = self.checker.types.intern(TypeKind::Function(signature));
                    Ok(self.substitute(t, &params))
                }
                ast::DeclKind::Enum { .. } => {
                    self.construct(p, vec![], None, span, ctx, &mut locals.clone())
                }
                _ => Ok(self.fresh(Class::Any)),
            };
        }
        let name = path.last().map(String::as_str).unwrap_or("");
        if matches!(
            name,
            "None" | "Relaxed" | "Acquire" | "Release" | "AcqRel" | "SeqCst"
        ) {
            return self.construct(p, vec![], None, span, ctx, &mut locals.clone());
        }
        Ok(self.fresh(Class::Any))
    }
    fn parameters(
        &mut self,
        path: &[ast::PathSegment],
        count: usize,
        ctx: &mut Context,
    ) -> Result<Vec<Term>> {
        let explicit = path.iter().flat_map(|p| &p.arguments).collect::<Vec<_>>();
        if explicit.is_empty() {
            return Ok((0..count).map(|_| self.fresh(Class::Any)).collect());
        }
        if explicit.len() != count {
            return Err(error(
                "E_TYPE",
                path.last().map(|p| p.span).unwrap_or_default(),
                "type argument count mismatch",
            ));
        }
        explicit
            .into_iter()
            .map(|t| self.checker.ty(t, ctx).map(Term::Known))
            .collect()
    }
    fn construct(
        &mut self,
        path: &[ast::PathSegment],
        values: Vec<(Option<&str>, &ast::Expr)>,
        expected: Option<Term>,
        span: Span,
        ctx: &mut Context,
        locals: &mut BTreeMap<String, Term>,
    ) -> Result<Term> {
        let names = path.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
        let name = names.last().map(String::as_str).unwrap_or("");
        let owner = self
            .checker
            .resolver
            .lookup(&ctx.module, &names, Namespace::Value, span)?
            .or(self
                .checker
                .resolver
                .lookup(&ctx.module, &names, Namespace::Type, span)?);
        let result = if let Some(id) = owner {
            if let Some(def) = self.checker.types.definition(id).cloned() {
                let args = self.parameters(path, def.params as usize, ctx)?;
                Term::Form(Head::Nominal(id), args)
            } else {
                return Ok(self.fresh(Class::Any));
            }
        } else {
            match name {
                "Some" | "None" => Term::Form(Head::Option, vec![self.fresh(Class::Any)]),
                "Ok" | "Err" => Term::Form(
                    Head::Result,
                    vec![self.fresh(Class::Any), self.fresh(Class::Any)],
                ),
                "Relaxed" | "Acquire" | "Release" | "AcqRel" | "SeqCst" => {
                    return Ok(self.known(TypeKind::Nominal(DefId(0xffff_ff00), vec![])));
                }
                _ => return Ok(self.fresh(Class::Any)),
            }
        };
        if let Some(want) = expected {
            self.unify(&result, &want, span)?;
        }
        let fields = self.payload(&result, name, span)?.unwrap_or_default();
        if fields.len() != values.len() {
            return Err(error("E_TYPE", span, "constructor field count mismatch"));
        }
        for (i, (field, value)) in values.into_iter().enumerate() {
            let want = if let Some(name) = field {
                fields
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, t)| t.clone())
                    .ok_or_else(|| error("E_TYPE", value.span, "unknown constructor field"))?
            } else {
                fields[i].1.clone()
            };
            self.collect(value, Some(want), ctx, locals)?;
        }
        Ok(result)
    }
    fn payload(
        &mut self,
        subject: &Term,
        name: &str,
        span: Span,
    ) -> Result<Option<Vec<(String, Term)>>> {
        if let Some(concrete) = self.materialize(subject)
            && !self.checker.has_params(concrete)
        {
            self.checker.prepare_type(concrete)?;
        }
        Ok(match self.form(subject) {
            Term::Form(Head::Option, v) => match name {
                "Some" => Some(vec![("0".into(), v[0].clone())]),
                "None" => Some(vec![]),
                _ => None,
            },
            Term::Form(Head::Result, v) => match name {
                "Ok" => Some(vec![("0".into(), v[0].clone())]),
                "Err" => Some(vec![("0".into(), v[1].clone())]),
                _ => None,
            },
            Term::Form(Head::Nominal(d), args) => {
                let def = self
                    .checker
                    .types
                    .definition(d)
                    .cloned()
                    .ok_or_else(|| error("E_NAME", span, "undefined nominal type"))?;
                let fields = match def.kind {
                    DefKind::Struct(fs) if def.name.rsplit("::").next() == Some(name) => Some(fs),
                    DefKind::Struct(_) => None,
                    DefKind::Enum(v) => v.into_iter().find(|v| v.name == name).map(|v| v.fields),
                };
                fields.map(|fs| {
                    fs.into_iter()
                        .map(|f| (f.name, self.substitute(f.ty, &args)))
                        .collect()
                })
            }
            _ => None,
        })
    }
    fn pattern(
        &mut self,
        p: &ast::Pattern,
        subject: &mut Term,
        borrow: Option<bool>,
        _ctx: &mut Context,
        locals: &mut BTreeMap<String, Term>,
    ) -> Result<()> {
        use ast::PatternKind::*;
        match &p.kind {
            Wildcard => {}
            Bool(_) => {
                let b = Term::Known(self.checker.bool());
                self.unify(subject, &b, p.span)?
            }
            Integer { value, .. } => {
                let t = if let Some(i) = super::expressions::literal(value, p.span)?.1 {
                    Term::Known(self.checker.int(i))
                } else {
                    self.fresh(Class::Integer)
                };
                self.unify(subject, &t, p.span)?
            }
            Path(path) => {
                let name = path.last().unwrap();
                if self.payload(subject, name, p.span)?.is_none() {
                    let bound = if let Some(m) = borrow {
                        Term::Form(Head::Ref(m), vec![subject.clone()])
                    } else {
                        subject.clone()
                    };
                    locals.insert(name.clone(), bound);
                }
            }
            Tuple(patterns) => {
                let fields = match self.form(subject) {
                    Term::Form(Head::Tuple, v) => v,
                    Term::Known(t) if matches!(self.checker.types.kind(t), TypeKind::Unit) => {
                        vec![]
                    }
                    Term::Var(_) => {
                        let v = (0..patterns.len())
                            .map(|_| self.fresh(Class::Any))
                            .collect::<Vec<_>>();
                        self.unify(subject, &Term::Form(Head::Tuple, v.clone()), p.span)?;
                        v
                    }
                    _ => return Err(error("E_PATTERN", p.span, "tuple pattern on non-tuple")),
                };
                if fields.len() != patterns.len() {
                    return Err(error("E_PATTERN", p.span, "tuple pattern arity mismatch"));
                }
                for (p, mut t) in patterns.iter().zip(fields) {
                    self.pattern(p, &mut t, borrow, _ctx, locals)?
                }
            }
            Payload { path, value } => {
                let name = path.last().unwrap();
                let fields = self
                    .payload(subject, name, p.span)?
                    .ok_or_else(|| error("E_PATTERN", p.span, "unknown payload pattern"))?;
                if fields.len() != 1 {
                    return Err(error(
                        "E_PATTERN",
                        p.span,
                        "single-payload pattern required",
                    ));
                }
                let mut t = fields[0].1.clone();
                self.pattern(value, &mut t, borrow, _ctx, locals)?
            }
            Record { path, fields } => {
                let name = path.last().unwrap();
                let members = self
                    .payload(subject, name, p.span)?
                    .ok_or_else(|| error("E_PATTERN", p.span, "record pattern on non-record"))?;
                for (name, p) in fields {
                    let mut t = members
                        .iter()
                        .find(|(n, _)| n == name)
                        .map(|(_, t)| t.clone())
                        .ok_or_else(|| {
                            error("E_PATTERN", p.span, "unknown record-pattern field")
                        })?;
                    self.pattern(p, &mut t, borrow, _ctx, locals)?
                }
            }
        }
        Ok(())
    }
    fn call(
        &mut self,
        callee: &ast::Expr,
        args: &[ast::Expr],
        expected: Option<Term>,
        span: Span,
        ctx: &mut Context,
        locals: &mut BTreeMap<String, Term>,
    ) -> Result<Term> {
        if let ast::ExprKind::Path(path) = &callee.kind {
            let names = path.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
            let owner = if path.len() == 1 && locals.contains_key(&path[0].name) {
                None
            } else {
                self.checker
                    .resolver
                    .lookup(&ctx.module, &names, Namespace::Value, span)?
            };
            if let Some(id) = owner {
                let record = self.checker.resolver.declarations[&id].clone();
                if let ast::DeclKind::Function(f) = &record.decl.kind {
                    let params =
                        self.parameters(path, f.generics.len() + record.impl_generics.len(), ctx)?;
                    let signature = self.checker.ensure_signature(id)?;
                    if args.len() != signature.params.len() {
                        return Err(error("E_TYPE", span, "argument count mismatch"));
                    }
                    let result = self.substitute(signature.result, &params);
                    if let Some(want) = expected {
                        self.contextual(&result, &want, span)?;
                    }
                    let explicit = path.iter().any(|p| !p.arguments.is_empty());
                    for (arg, formal) in args.iter().zip(signature.params) {
                        let depends = self.checker.has_params(formal);
                        let formal = self.substitute(formal, &params);
                        if explicit || !depends {
                            self.collect(arg, Some(formal), ctx, locals)?;
                        } else {
                            let actual = self.collect(arg, None, ctx, locals)?;
                            self.unify(&actual, &formal, arg.span)?;
                        }
                    }
                    return Ok(result);
                }
                return self.construct(
                    path,
                    args.iter().map(|e| (None, e)).collect(),
                    expected,
                    span,
                    ctx,
                    locals,
                );
            }
            let builtin = if path.len() == 1 && locals.contains_key(&path[0].name) {
                None
            } else {
                self.checker
                    .resolver
                    .builtin_path(&ctx.module, &names, Namespace::Value, span)?
            };
            let name = builtin
                .as_ref()
                .and_then(|p| p.last())
                .map(String::as_str)
                .unwrap_or("");
            if matches!(
                name,
                "volatile_load"
                    | "mmio_load"
                    | "volatile_store"
                    | "mmio_store"
                    | "slice_from_raw_parts"
                    | "slice_from_raw_parts_mut"
            ) {
                let inner = self.parameters(path, 1, ctx)?[0].clone();
                let store = name.ends_with("store");
                let space = if name.starts_with("mmio") {
                    AddressSpace::Mmio
                } else {
                    AddressSpace::Kernel
                };
                let constant = !store && !name.ends_with("_mut");
                if let Some(arg) = args.first() {
                    self.collect(
                        arg,
                        Some(Term::Form(
                            Head::Pointer(space, constant),
                            vec![inner.clone()],
                        )),
                        ctx,
                        locals,
                    )?;
                }
                if let Some(arg) = args.get(1) {
                    let want = if store {
                        inner.clone()
                    } else {
                        self.known(TypeKind::Int(Integer::Usize))
                    };
                    self.collect(arg, Some(want), ctx, locals)?;
                }
                return Ok(if store {
                    Term::Known(self.checker.unit())
                } else if name.starts_with("slice") {
                    Term::Form(Head::Slice(constant), vec![inner])
                } else {
                    inner
                });
            }
            if name == "checked_cast" {
                for arg in args {
                    self.collect(arg, None, ctx, locals)?;
                }
                let payload = self.parameters(path, 1, ctx)?[0].clone();
                return Ok(Term::Form(Head::Option, vec![payload]));
            }
            if matches!(name, "addr_of" | "addr_of_mut")
                && let Some(arg) = args.first()
            {
                let inner = self.collect(arg, None, ctx, locals)?;
                return Ok(Term::Form(
                    Head::Pointer(AddressSpace::Kernel, name == "addr_of"),
                    vec![inner],
                ));
            }
            if matches!(name, "ptr_add" | "ptr_add_bytes")
                && let Some(arg) = args.first()
            {
                let pointer = self.collect(arg, expected.clone(), ctx, locals)?;
                for arg in args.iter().skip(1) {
                    let usize_ = self.known(TypeKind::Int(Integer::Usize));
                    self.collect(arg, Some(usize_), ctx, locals)?;
                }
                return Ok(pointer);
            }
            if name == "addr" {
                return Ok(self.known(TypeKind::Int(Integer::Usize)));
            }
            if matches!(name, "Some" | "None" | "Ok" | "Err") {
                return self.construct(
                    path,
                    args.iter().map(|e| (None, e)).collect(),
                    expected,
                    span,
                    ctx,
                    locals,
                );
            }
            if name == "new" && names.len() >= 2 {
                let h = match names[names.len() - 2].as_str() {
                    "Atomic" => Some(Head::Atomic),
                    "UnsafeCell" => Some(Head::UnsafeCell),
                    "MaybeUninit" => Some(Head::MaybeUninit),
                    _ => None,
                };
                if let Some(h) = h {
                    let inner = self.parameters(path, 1, ctx)?[0].clone();
                    let result = Term::Form(h, vec![inner.clone()]);
                    if let Some(want) = expected {
                        self.unify(&result, &want, span)?
                    }
                    if let Some(arg) = args.first() {
                        self.collect(arg, Some(inner), ctx, locals)?;
                    }
                    return Ok(result);
                }
            }
            if name == "uninit" {
                let inner = self.parameters(path, 1, ctx)?[0].clone();
                return Ok(Term::Form(Head::MaybeUninit, vec![inner]));
            }
            if name == "panic" {
                return Ok(self.known(TypeKind::Never));
            }
        }
        if let ast::ExprKind::Field {
            base,
            name,
            arguments: typeargs,
            grouped: false,
        } = &callee.kind
        {
            let receiver = self.collect(base, None, ctx, locals)?;
            // A receiver may itself be a field whose type is an outstanding
            // projection; settle it before selecting the method signature.
            self.solve_projections()?;
            let mut t = self.form(&receiver);
            while let Term::Form(Head::Ref(_), v) = t {
                t = self.form(&v[0]);
            }
            if let Term::Form(Head::Nominal(id), nominalargs) = t.clone()
                && let Some(method) = self
                    .checker
                    .resolver
                    .methods
                    .get(&(id, name.clone()))
                    .copied()
            {
                let record = self.checker.resolver.declarations[&method].clone();
                if let ast::DeclKind::Function(function) = record.decl.kind {
                    let mut params = nominalargs;
                    if typeargs.is_empty() {
                        params.extend((0..function.generics.len()).map(|_| self.fresh(Class::Any)));
                    } else {
                        for arg in typeargs {
                            params.push(Term::Known(self.checker.ty(arg, ctx)?));
                        }
                    }
                    let signature = self.checker.ensure_signature(method)?;
                    let result = self.substitute(signature.result, &params);
                    if let Some(want) = expected.clone() {
                        self.contextual(&result, &want, span)?;
                    }
                    for (arg, formal) in args.iter().zip(signature.params.iter().skip(1)) {
                        let contextual = !typeargs.is_empty() || !self.checker.has_params(*formal);
                        let formal = self.substitute(*formal, &params);
                        if contextual {
                            self.collect(arg, Some(formal), ctx, locals)?;
                        } else {
                            let actual = self.collect(arg, None, ctx, locals)?;
                            self.unify(&actual, &formal, arg.span)?;
                        }
                    }
                    return Ok(result);
                }
            }
            if let Term::Form(head, v) = t {
                match head {
                    Head::Atomic => {
                        let inner = v[0].clone();
                        let count = if name == "compare_exchange" {
                            2
                        } else if name == "load" {
                            0
                        } else {
                            1
                        };
                        for arg in args.iter().take(count) {
                            self.collect(arg, Some(inner.clone()), ctx, locals)?;
                        }
                        return Ok(if name == "store" {
                            Term::Known(self.checker.unit())
                        } else if name == "compare_exchange" {
                            Term::Form(Head::Result, vec![inner.clone(), inner])
                        } else {
                            inner
                        });
                    }
                    Head::MaybeUninit | Head::UnsafeCell => {
                        if name == "write" {
                            if let Some(arg) = args.first() {
                                self.collect(arg, Some(v[0].clone()), ctx, locals)?;
                            }
                            return Ok(Term::Known(self.checker.unit()));
                        }
                        if name == "assume_init" {
                            return Ok(v[0].clone());
                        }
                        if matches!(name.as_str(), "get" | "as_ptr" | "as_mut_ptr") {
                            return Ok(Term::Form(
                                Head::Pointer(AddressSpace::Kernel, name == "as_ptr"),
                                vec![v[0].clone()],
                            ));
                        }
                    }
                    Head::Slice(c) if name == "as_ptr" || name == "as_mut_ptr" => {
                        return Ok(Term::Form(
                            Head::Pointer(AddressSpace::Kernel, name == "as_ptr" || c),
                            vec![v[0].clone()],
                        ));
                    }
                    _ => {}
                }
            }
        }
        let function = self.collect(callee, None, ctx, locals)?;
        if let Term::Form(Head::Function(..), v) = self.form(&function) {
            if v.len() != args.len() + 1 {
                return Err(error("E_TYPE", span, "function argument count mismatch"));
            }
            for (arg, formal) in args.iter().zip(&v) {
                self.collect(arg, Some(formal.clone()), ctx, locals)?;
            }
            return Ok(v.last().unwrap().clone());
        }
        Ok(self.fresh(Class::Any))
    }
}
