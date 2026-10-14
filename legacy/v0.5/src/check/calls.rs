use super::*;
impl Checker {
    pub(super) fn path_expr(
        &mut self,
        p: &[ast::PathSegment],
        expected: Option<TyId>,
        span: Span,
        ctx: &mut Context,
    ) -> Result<h::Expr> {
        let path = p.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
        if p.len() == 1
            && let Some(id) = ctx.local(&p[0].name)
        {
            if !p[0].arguments.is_empty() {
                return Err(error(
                    "E_TYPE",
                    span,
                    "local binding takes no type arguments",
                ));
            }
            let t = ctx.locals[id.0 as usize].ty;
            if let Some(e) = expected {
                self.coerce(t, e, span)?
            }
            return Ok(h::Expr {
                ty: t,
                kind: h::ExprKind::Local(id),
                span,
            });
        }
        if path.len() == 2
            && let Some(i) = integer(&path[0])
            && (path[1] == "MIN" || path[1] == "MAX")
        {
            let n = if i.signed() {
                if path[1] == "MIN" {
                    1u64 << (i.bits() - 1)
                } else {
                    (1u64 << (i.bits() - 1)) - 1
                }
            } else if path[1] == "MIN" {
                0
            } else if i.bits() == 64 {
                u64::MAX
            } else {
                (1 << i.bits()) - 1
            };
            return Ok(h::Expr {
                ty: self.int(i),
                kind: h::ExprKind::Integer(n),
                span,
            });
        }
        if let Some(id) = self
            .resolver
            .lookup(&ctx.module, &path, Namespace::Value, span)?
        {
            let r = self.resolver.declarations[&id].clone();
            let e = match r.decl.kind {
                ast::DeclKind::Const { .. } => self.constant(id)?,
                ast::DeclKind::Static {
                    mutable,
                    external,
                    ty,
                    ..
                } => {
                    if (mutable || external) && ctx.unsafe_depth == 0 {
                        return Err(error(
                            "E_UNSAFE",
                            span,
                            "mutable or external static access requires unsafe block",
                        ));
                    }
                    let _ = ty;
                    let ty = self.declaration_type(id)?;
                    h::Expr {
                        ty,
                        kind: h::ExprKind::Global {
                            symbol: self.global_symbol(id),
                            mutable,
                            external,
                        },
                        span,
                    }
                }
                ast::DeclKind::Function(f) => {
                    if r.decl
                        .attributes
                        .iter()
                        .any(|a| ["irq", "trap", "trap_entry"].contains(&a.name.as_str()))
                    {
                        return Err(error(
                            "E_CONTEXT",
                            span,
                            "special function cannot become ordinary function pointer",
                        ));
                    }
                    let mut args = p
                        .iter()
                        .flat_map(|p| p.arguments.iter())
                        .map(|t| self.ty(t, ctx))
                        .collect::<Result<Vec<_>>>()?;
                    let count = f.generics.len() + r.impl_generics.len();
                    if args.is_empty()
                        && count != 0
                        && let Some(target) = expected
                        && matches!(self.types.kind(target), TypeKind::Function(_))
                    {
                        let signature = self.ensure_signature(id)?;
                        let template = self.types.intern(TypeKind::Function(signature));
                        let mut inferred = vec![None; count];
                        self.infer(template, target, &mut inferred, span)?;
                        args = inferred
                            .into_iter()
                            .collect::<Option<Vec<_>>>()
                            .ok_or_else(|| {
                                error("E_TYPE", span, "cannot infer generic function value")
                            })?;
                    }
                    if args.len() != f.generics.len() + r.impl_generics.len() {
                        return Err(error(
                            "E_TYPE",
                            span,
                            "function type arguments must be complete",
                        ));
                    }
                    let symbol = self.instance(id, args.clone())?;
                    let sig = self.ensure_signature(id)?;
                    let temp = self.types.intern(TypeKind::Function(sig));
                    let ty = self.types.substitute(temp, &args);
                    h::Expr {
                        ty,
                        kind: h::ExprKind::Function { symbol },
                        span,
                    }
                }
                ast::DeclKind::Enum { .. } => return self.constructor(p, &[], expected, span, ctx),
                _ => {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "constructor requires call or record fields",
                    ));
                }
            };
            if let Some(t) = expected {
                self.coerce(e.ty, t, span)?
            }
            return Ok(e);
        }
        if let Some(b) = self
            .resolver
            .builtin_path(&ctx.module, &path, Namespace::Value, span)?
        {
            let name = b.last().unwrap().as_str();
            if ["None", "Relaxed", "Acquire", "Release", "AcqRel", "SeqCst"].contains(&name) {
                return self.constructor(p, &[], expected, span, ctx);
            }
            return Err(error(
                "E_TYPE",
                span,
                "intrinsic cannot be taken as function value",
            ));
        }
        // An unqualified variant can be selected by the expected enum.
        if path.len() == 1
            && let Some(t) = expected
            && self.variant(t, path.last().unwrap()).is_some()
        {
            return self.constructor(p, &[], expected, span, ctx);
        }
        Err(error(
            "E_NAME",
            span,
            format!("unknown value {}", path.join("::")),
        ))
    }
    fn method_exists(&mut self, base: &ast::Expr, name: &str, ctx: &mut Context) -> Result<bool> {
        let Some(mut ty) = self.hint(base, ctx)? else {
            return Ok(false);
        };
        while let TypeKind::Ref { target, .. } = self.types.kind(ty) {
            ty = *target;
        }
        Ok(match self.types.kind(ty) {
            TypeKind::Nominal(def, _) => {
                self.resolver.methods.contains_key(&(*def, name.to_owned()))
            }
            _ => false,
        })
    }
    pub(super) fn call(
        &mut self,
        callee: &ast::Expr,
        args: &[ast::Expr],
        expected: Option<TyId>,
        span: Span,
        ctx: &mut Context,
    ) -> Result<h::Expr> {
        if let ast::ExprKind::Field {
            base,
            name,
            arguments,
            grouped,
        } = &callee.kind
        {
            if *grouped {
                return self.indirect(callee, args, span, ctx);
            }
            if arguments.is_empty()
                && !self.method_exists(base, name, ctx)?
                && self
                    .hint(callee, ctx)?
                    .is_some_and(|t| matches!(self.types.kind(t), TypeKind::Function(_)))
            {
                return self.indirect(callee, args, span, ctx);
            }
            return self.method(
                base,
                name,
                arguments,
                CallSite {
                    arguments: args,
                    expected,
                    span,
                },
                ctx,
            );
        }
        if let ast::ExprKind::Path(p) = &callee.kind {
            let path = p.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
            if p.len() == 1 && ctx.local(&p[0].name).is_some() {
                return self.indirect(callee, args, span, ctx);
            }
            if let Some(id) = self
                .resolver
                .lookup(&ctx.module, &path, Namespace::Value, span)?
            {
                let r = self.resolver.declarations[&id].clone();
                if let ast::DeclKind::Function(f) = r.decl.kind.clone() {
                    let explicit = p
                        .iter()
                        .flat_map(|p| p.arguments.iter())
                        .map(|t| self.ty(t, ctx))
                        .collect::<Result<Vec<_>>>()?;
                    return self.function_call(
                        id,
                        &f,
                        explicit,
                        None,
                        CallSite {
                            arguments: args,
                            expected,
                            span,
                        },
                        ctx,
                    );
                }
                return self.constructor(p, args, expected, span, ctx);
            }
            if let Some(builtin) =
                self.resolver
                    .builtin_path(&ctx.module, &path, Namespace::Value, span)?
            {
                let name = builtin.join("::");
                if ["Some", "None", "Ok", "Err"].contains(&builtin.last().unwrap().as_str()) {
                    return self.constructor(p, args, expected, span, ctx);
                }
                return self.intrinsic(&name, p, args, expected, span, ctx);
            }
            if path.len() == 1
                && let Some(expected) = expected
                && self.variant(expected, path.last().unwrap()).is_some()
            {
                return self.constructor(p, args, Some(expected), span, ctx);
            }
            return Err(error("E_NAME", span, "unknown called function"));
        }
        self.indirect(callee, args, span, ctx)
    }
    fn indirect(
        &mut self,
        callee: &ast::Expr,
        args: &[ast::Expr],
        span: Span,
        ctx: &mut Context,
    ) -> Result<h::Expr> {
        let callee = self.expr(callee, None, ctx)?;
        let TypeKind::Function(sig) = self.types.kind(callee.ty).clone() else {
            return Err(error("E_TYPE", span, "callee is not a function"));
        };
        self.check_call_unsafe(&sig, span, ctx)?;
        if sig.params.len() != args.len() {
            return Err(error("E_TYPE", span, "argument count mismatch"));
        }
        let arguments = args
            .iter()
            .zip(&sig.params)
            .map(|(a, t)| self.expr(a, Some(*t), ctx))
            .collect::<Result<_>>()?;
        Ok(h::Expr {
            ty: sig.result,
            kind: h::ExprKind::Call {
                callee: Box::new(callee),
                signature: sig,
                arguments,
            },
            span,
        })
    }
    fn check_call_unsafe(&self, sig: &FnType, span: Span, ctx: &Context) -> Result<()> {
        if (sig.unsafe_ || sig.abi == Abi::C) && ctx.unsafe_depth == 0 {
            return Err(error(
                "E_UNSAFE",
                span,
                "unsafe or C ABI call requires unsafe block",
            ));
        }
        Ok(())
    }
    pub(super) fn function_call(
        &mut self,
        id: DefId,
        f: &ast::Function,
        explicit: Vec<TyId>,
        receiver: Option<h::Expr>,
        site: CallSite<'_>,
        ctx: &mut Context,
    ) -> Result<h::Expr> {
        let CallSite {
            arguments: args,
            expected,
            span,
        } = site;
        let r = self.resolver.declarations[&id].clone();
        if r.decl
            .attributes
            .iter()
            .any(|a| ["trap", "trap_entry", "irq"].contains(&a.name.as_str()))
        {
            return Err(error(
                "E_CONTEXT",
                span,
                "special-context function cannot be called normally",
            ));
        }
        let template = self.ensure_signature(id)?;
        if template.params.len() != args.len() + receiver.is_some() as usize {
            return Err(error("E_TYPE", span, "argument count mismatch"));
        }
        let n = f.generics.len() + r.impl_generics.len();
        let mut inferred = vec![None; n];
        if !explicit.is_empty() {
            if explicit.len() != n {
                return Err(error(
                    "E_TYPE",
                    span,
                    "explicit type arguments must be complete",
                ));
            }
            for (i, t) in explicit.iter().enumerate() {
                inferred[i] = Some(*t)
            }
        }
        if explicit.is_empty()
            && let Some(t) = expected
        {
            self.infer(template.result, t, &mut inferred, span)?
        }
        let start = receiver.is_some() as usize;
        for (a, t) in args.iter().zip(&template.params[start..]) {
            if explicit.is_empty()
                && self.has_params(*t)
                && let Some(actual) = self.hint(a, ctx)?
            {
                self.infer(*t, actual, &mut inferred, span)?
            }
        }
        let mut values = vec![];
        let mut offset = 0;
        if let Some(receiver) = receiver {
            self.infer(template.params[0], receiver.ty, &mut inferred, span)?;
            values.push(receiver);
            offset = 1
        }
        for (a, t) in args.iter().zip(&template.params[offset..]) {
            let want = self.partial_type(*t, &inferred);
            let v = self.expr(a, want, ctx)?;
            self.infer(*t, v.ty, &mut inferred, span)?;
            values.push(v)
        }
        let typeargs = inferred
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| error("E_TYPE", span, "cannot infer generic arguments"))?;
        let ft = self.types.intern(TypeKind::Function(template));
        let t = self.types.substitute(ft, &typeargs);
        let t = if !typeargs.iter().any(|t| self.has_params(*t)) {
            self.materialize(t, span)?
        } else {
            t
        };
        let TypeKind::Function(sig) = self.types.kind(t).clone() else {
            unreachable!()
        };
        self.check_call_unsafe(&sig, span, ctx)?;
        for (v, t) in values.iter_mut().zip(&sig.params) {
            self.coerce(v.ty, *t, span)?;
            if v.ty != *t {
                let old = v.clone();
                *v = h::Expr {
                    ty: *t,
                    kind: h::ExprKind::Adapt(Box::new(old)),
                    span: v.span,
                }
            }
        }
        if ctx.template && typeargs.iter().any(|t| self.has_params(*t)) {
            return Ok(h::Expr {
                ty: sig.result,
                span,
                kind: h::ExprKind::Intrinsic {
                    name: "template.deferred_call".into(),
                    arguments: values,
                    type_arguments: typeargs,
                },
            });
        }
        let symbol = self.instance(id, typeargs)?;
        let callee = h::Expr {
            ty: t,
            kind: h::ExprKind::Function { symbol },
            span,
        };
        Ok(h::Expr {
            ty: sig.result,
            kind: h::ExprKind::Call {
                callee: Box::new(callee),
                signature: sig,
                arguments: values,
            },
            span,
        })
    }
    pub(super) fn partial_type(&mut self, t: TyId, inferred: &[Option<TyId>]) -> Option<TyId> {
        if self.has_params(t) {
            let args = inferred.iter().copied().collect::<Option<Vec<_>>>()?;
            let t = self.types.substitute(t, &args);
            if args.iter().any(|t| self.has_params(*t)) {
                Some(t)
            } else {
                self.materialize(t, Span::default()).ok()
            }
        } else {
            Some(t)
        }
    }
    pub(super) fn has_params(&self, t: TyId) -> bool {
        match self.types.kind(t) {
            TypeKind::Param(_) => true,
            TypeKind::ArrayExpr(t, len) => self.has_params(*t) || self.length_has_params(len),
            TypeKind::Function(f) => {
                f.params.iter().any(|t| self.has_params(*t)) || self.has_params(f.result)
            }
            TypeKind::Ref { target, .. }
            | TypeKind::Pointer { target, .. }
            | TypeKind::Array(target, _)
            | TypeKind::Option(target)
            | TypeKind::MaybeUninit(target)
            | TypeKind::UnsafeCell(target)
            | TypeKind::Atomic(target)
            | TypeKind::Range(target) => self.has_params(*target),
            TypeKind::Slice { element, .. } => self.has_params(*element),
            TypeKind::Result(a, b) => self.has_params(*a) || self.has_params(*b),
            TypeKind::Tuple(v) | TypeKind::Nominal(_, v) => v.iter().any(|t| self.has_params(*t)),
            _ => false,
        }
    }
    fn length_has_params(&self, len: &ConstLen) -> bool {
        match &len.kind {
            ConstLenKind::Deferred { args, queries, .. } => {
                args.iter().chain(queries).any(|t| self.has_params(*t))
            }
            ConstLenKind::SizeOf(t) | ConstLenKind::AlignOf(t) | ConstLenKind::OffsetOf(t, _) => {
                self.has_params(*t)
            }
            ConstLenKind::Unary(_, x)
            | ConstLenKind::Cast(x)
            | ConstLenKind::CastFits { value: x, .. } => self.length_has_params(x),
            ConstLenKind::Binary(_, a, b)
            | ConstLenKind::Assert(a, b)
            | ConstLenKind::CheckedFits {
                left: a, right: b, ..
            } => self.length_has_params(a) || self.length_has_params(b),
            ConstLenKind::If(c, a, b) => {
                self.length_has_params(c) || self.length_has_params(a) || self.length_has_params(b)
            }
            _ => false,
        }
    }
    pub(super) fn infer(
        &self,
        template: TyId,
        actual: TyId,
        args: &mut [Option<TyId>],
        span: Span,
    ) -> Result<()> {
        match (self.types.kind(template), self.types.kind(actual)) {
            (TypeKind::Param(i), _) => {
                let p = &mut args[*i as usize];
                if let Some(t) = p {
                    if !self.types.core_compatible(actual, *t)
                        && !matches!(self.types.kind(actual), TypeKind::Never)
                    {
                        return Err(error(
                            "E_TYPE",
                            span,
                            "generic argument equality cannot use implicit weakening",
                        ));
                    }
                } else {
                    *p = Some(actual)
                }
            }
            (TypeKind::Ref { target: a, .. }, TypeKind::Ref { target: b, .. })
            | (TypeKind::Pointer { target: a, .. }, TypeKind::Pointer { target: b, .. })
            | (TypeKind::Slice { element: a, .. }, TypeKind::Slice { element: b, .. })
            | (TypeKind::Array(a, _), TypeKind::Array(b, _))
            | (TypeKind::ArrayExpr(a, _), TypeKind::Array(b, _))
            | (TypeKind::ArrayExpr(a, _), TypeKind::ArrayExpr(b, _))
            | (TypeKind::Option(a), TypeKind::Option(b))
            | (TypeKind::MaybeUninit(a), TypeKind::MaybeUninit(b))
            | (TypeKind::UnsafeCell(a), TypeKind::UnsafeCell(b))
            | (TypeKind::Atomic(a), TypeKind::Atomic(b)) => self.infer(*a, *b, args, span)?,
            (TypeKind::Result(a, b), TypeKind::Result(c, d)) => {
                self.infer(*a, *c, args, span)?;
                self.infer(*b, *d, args, span)?
            }
            (TypeKind::Function(a), TypeKind::Function(b)) => {
                if a.abi != b.abi || a.params.len() != b.params.len() || a.unsafe_ && !b.unsafe_ {
                    return Err(error("E_TYPE", span, "generic function signature mismatch"));
                }
                for (a, b) in a.params.iter().zip(&b.params) {
                    self.infer(*a, *b, args, span)?
                }
                self.infer(a.result, b.result, args, span)?
            }
            (TypeKind::Tuple(a), TypeKind::Tuple(b))
            | (TypeKind::Nominal(_, a), TypeKind::Nominal(_, b)) => {
                for (a, b) in a.iter().zip(b) {
                    self.infer(*a, *b, args, span)?
                }
            }
            _ => self.coerce(actual, template, span)?,
        }
        Ok(())
    }
    pub(super) fn variant(&self, t: TyId, name: &str) -> Option<usize> {
        match self.types.kind(t) {
            TypeKind::Option(_) => ["Some", "None"].iter().position(|n| *n == name),
            TypeKind::Result(..) => ["Ok", "Err"].iter().position(|n| *n == name),
            TypeKind::Nominal(id, _) => match &self.types.definition(*id)?.kind {
                DefKind::Enum(v) => v.iter().position(|v| v.name == name),
                _ => None,
            },
            _ => None,
        }
    }
    fn constructor(
        &mut self,
        p: &[ast::PathSegment],
        args: &[ast::Expr],
        expected: Option<TyId>,
        span: Span,
        ctx: &mut Context,
    ) -> Result<h::Expr> {
        let name = &p.last().unwrap().name;
        let path = p.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
        let owner = self
            .resolver
            .lookup(&ctx.module, &path, Namespace::Value, span)?;
        if let Some(id) = owner {
            self.ensure_type_definition(id)?;
            let r = self.resolver.declarations[&id].clone();
            let (generics, variant) = match &r.decl.kind {
                ast::DeclKind::Struct {
                    generics,
                    newtype: Some(_),
                    ..
                } => (generics.clone(), None),
                ast::DeclKind::Enum { generics, variants } => {
                    let v = variants
                        .iter()
                        .position(|v| &v.name == name)
                        .ok_or_else(|| error("E_NAME", span, "variant not found"))?;
                    if !variants[v].fields.is_empty() {
                        return Err(error(
                            "E_TYPE",
                            span,
                            "named variant requires record fields",
                        ));
                    }
                    (generics.clone(), Some(v))
                }
                _ => {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "record struct requires named construction",
                    ));
                }
            };
            if r.module != ctx.module && variant.is_none() {
                return Err(error("E_NAME", span, "newtype payload is private"));
            }
            let mut inferred = vec![None; generics.len()];
            if let Some(t) = expected {
                let TypeKind::Nominal(found, a) = self.types.kind(t) else {
                    return Err(error("E_TYPE", span, "constructor type mismatch"));
                };
                if *found != id {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "constructor nominal identity mismatch",
                    ));
                }
                for (i, t) in a.iter().enumerate() {
                    inferred[i] = Some(*t)
                }
            }
            let explicit = p
                .iter()
                .flat_map(|s| &s.arguments)
                .map(|t| self.ty(t, ctx))
                .collect::<Result<Vec<_>>>()?;
            if !explicit.is_empty() {
                if explicit.len() != inferred.len() {
                    return Err(error("E_TYPE", span, "constructor generic count mismatch"));
                }
                for (i, t) in explicit.iter().enumerate() {
                    if inferred[i].is_some_and(|a| a != *t) {
                        return Err(error(
                            "E_TYPE",
                            span,
                            "explicit constructor context mismatch",
                        ));
                    }
                    inferred[i] = Some(*t)
                }
            }
            let params = (0..generics.len())
                .map(|i| self.types.intern(TypeKind::Param(i as u32)))
                .collect();
            let template = self.types.intern(TypeKind::Nominal(id, params));
            let fields = self.fields(template, variant, span)?;
            if fields.len() != args.len() {
                return Err(error("E_TYPE", span, "constructor payload count mismatch"));
            }
            for (a, f) in args.iter().zip(&fields) {
                if let Some(t) = self.hint(a, ctx)? {
                    self.infer(f.ty, t, &mut inferred, span)?
                }
            }
            let mut values = vec![];
            for (i, (a, f)) in args.iter().zip(&fields).enumerate() {
                let hint = self.partial_type(f.ty, &inferred);
                let v = self.expr(a, hint, ctx)?;
                self.infer(f.ty, v.ty, &mut inferred, span)?;
                values.push((i, v))
            }
            let a = inferred
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| error("E_TYPE", span, "constructor type cannot be inferred"))?;
            for (g, t) in generics.iter().zip(&a) {
                if g.copy && !self.types.properties(*t).copy {
                    return Err(error(
                        "E_INSTANTIATION",
                        span,
                        "constructor Copy constraint failed",
                    ));
                }
            }
            let ty = self.types.intern(TypeKind::Nominal(id, a));
            return Ok(h::Expr {
                ty,
                span,
                kind: h::ExprKind::Aggregate {
                    variant,
                    fields: values,
                },
            });
        }
        let mut ty = expected;
        if ty.is_none() {
            match name.as_str() {
                "Some" if args.len() == 1 => {
                    let v = self.expr(&args[0], None, ctx)?;
                    let ty = self.types.intern(TypeKind::Option(v.ty));
                    return Ok(h::Expr {
                        ty,
                        kind: h::ExprKind::Aggregate {
                            variant: Some(0),
                            fields: vec![(0, v)],
                        },
                        span,
                    });
                }
                "Relaxed" | "Acquire" | "Release" | "AcqRel" | "SeqCst" => {
                    ty = Some(
                        self.types
                            .intern(TypeKind::Nominal(DefId(0xffff_ff00), vec![])),
                    )
                }
                _ => return Err(error("E_TYPE", span, "constructor type cannot be inferred")),
            }
        }
        let ty = ty.unwrap();
        let variant = self
            .variant(ty, name)
            .ok_or_else(|| error("E_NAME", span, "unknown variant"))?;
        let fields = self.fields(ty, Some(variant), span)?;
        if fields.len() != args.len() {
            return Err(error("E_TYPE", span, "constructor argument count mismatch"));
        }
        let mut values = vec![];
        for (i, (a, f)) in args.iter().zip(&fields).enumerate() {
            values.push((i, self.expr(a, Some(f.ty), ctx)?))
        }
        Ok(h::Expr {
            ty,
            kind: h::ExprKind::Aggregate {
                variant: Some(variant),
                fields: values,
            },
            span,
        })
    }
    pub(super) fn record_spec(
        &mut self,
        p: &[ast::PathSegment],
        names: &[String],
        expected: Option<TyId>,
        span: Span,
        ctx: &mut Context,
    ) -> Result<RecordSpec> {
        let path = p.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
        let id = self
            .resolver
            .lookup(&ctx.module, &path, Namespace::Value, span)?
            .or(self
                .resolver
                .lookup(&ctx.module, &path, Namespace::Type, span)?)
            .ok_or_else(|| error("E_NAME", span, "unknown record constructor"))?;
        let record = self.resolver.declarations[&id].clone();
        let (generics, variant) = match &record.decl.kind {
            ast::DeclKind::Struct {
                generics,
                fields,
                newtype,
                ..
            } => {
                if newtype.is_some() {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "newtype requires payload construction",
                    ));
                }
                if record.module != ctx.module && fields.iter().any(|f| !f.public) {
                    return Err(error(
                        "E_NAME",
                        span,
                        "record constructor has private fields",
                    ));
                }
                (generics.clone(), None)
            }
            ast::DeclKind::Enum { generics, variants } => {
                let v = variants
                    .iter()
                    .position(|v| v.name == p.last().unwrap().name)
                    .ok_or_else(|| error("E_NAME", span, "unknown record variant"))?;
                if variants[v].payload.is_some() {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "payload variant requires call construction",
                    ));
                }
                (generics.clone(), Some(v))
            }
            _ => return Err(error("E_TYPE", span, "not a record type")),
        };
        self.ensure_type_definition(id)?;
        let mut inferred = vec![None; generics.len()];
        if let Some(t) = expected {
            let TypeKind::Nominal(owner, args) = self.types.kind(t) else {
                return Err(error("E_TYPE", span, "record expected type mismatch"));
            };
            if *owner != id || args.len() != inferred.len() {
                return Err(error(
                    "E_TYPE",
                    span,
                    "constructor nominal context mismatch",
                ));
            }
            for (i, t) in args.iter().enumerate() {
                inferred[i] = Some(*t)
            }
        }
        let explicit = p
            .iter()
            .flat_map(|s| &s.arguments)
            .map(|t| self.ty(t, ctx))
            .collect::<Result<Vec<_>>>()?;
        if !explicit.is_empty() {
            if explicit.len() != inferred.len() {
                return Err(error("E_TYPE", span, "constructor type argument count"));
            }
            for (i, t) in explicit.iter().enumerate() {
                if inferred[i].is_some_and(|old| !self.types.core_compatible(old, *t)) {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "constructor generic context mismatch",
                    ));
                }
                inferred[i] = Some(*t)
            }
        }
        let params = (0..generics.len())
            .map(|i| self.types.intern(TypeKind::Param(i as u32)))
            .collect();
        let template = self.types.intern(TypeKind::Nominal(id, params));
        let fields = self.fields(template, variant, span)?;
        let mut seen = BTreeSet::new();
        for n in names {
            if !fields.iter().any(|f| &f.name == n) {
                return Err(error("E_TYPE", span, "extra constructor field"));
            }
            if !seen.insert(n) {
                return Err(error("E_TYPE", span, "duplicate constructor field"));
            }
        }
        if seen.len() != fields.len() {
            return Err(error("E_TYPE", span, "missing constructor field"));
        }
        Ok(RecordSpec {
            id,
            generics,
            variant,
            fields,
            inferred,
            explicit: !explicit.is_empty(),
        })
    }
    pub(super) fn record(
        &mut self,
        p: &[ast::PathSegment],
        init: &[(String, ast::Expr)],
        expected: Option<TyId>,
        span: Span,
        ctx: &mut Context,
    ) -> Result<h::Expr> {
        let spec = self.record_spec(
            p,
            &init.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
            expected,
            span,
            ctx,
        )?;
        let RecordSpec {
            id,
            generics,
            variant,
            fields,
            mut inferred,
            explicit,
        } = spec;
        if let Some(arguments) = inferred.iter().copied().collect::<Option<Vec<_>>>() {
            let nominal = self.types.intern(TypeKind::Nominal(id, arguments));
            self.prepare_type(nominal)?;
        }
        for (name, value) in init {
            if !explicit
                && let Some(f) = fields.iter().find(|f| &f.name == name)
                && self.has_params(f.ty)
                && let Some(actual) = self.hint(value, ctx)?
            {
                self.infer(f.ty, actual, &mut inferred, span)?
            }
        }
        let mut used = BTreeSet::new();
        let mut values = vec![];
        for (name, value) in init {
            let i = fields
                .iter()
                .position(|f| &f.name == name)
                .ok_or_else(|| error("E_TYPE", value.span, "extra constructor field"))?;
            if !used.insert(i) {
                return Err(error("E_TYPE", value.span, "duplicate constructor field"));
            }
            let expect = self.partial_type(fields[i].ty, &inferred);
            let v = self.expr(value, expect, ctx)?;
            self.infer(fields[i].ty, v.ty, &mut inferred, span)?;
            values.push((i, v))
        }
        if used.len() != fields.len() {
            return Err(error("E_TYPE", span, "missing constructor field"));
        }
        let args = inferred
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| error("E_TYPE", span, "constructor type arguments not inferred"))?;
        for (g, t) in generics.iter().zip(&args) {
            if g.copy && !self.types.properties(*t).copy {
                return Err(error(
                    "E_INSTANTIATION",
                    span,
                    "constructor Copy constraint failed",
                ));
            }
        }
        let ty = self.types.intern(TypeKind::Nominal(id, args));
        let final_fields = self.fields(ty, variant, span)?;
        for (i, v) in &mut values {
            self.coerce(v.ty, final_fields[*i].ty, span)?;
        }
        Ok(h::Expr {
            ty,
            span,
            kind: h::ExprKind::Aggregate {
                variant,
                fields: values,
            },
        })
    }
}
