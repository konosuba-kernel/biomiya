use super::*;
impl Checker {
    pub(super) fn expr(
        &mut self,
        e: &ast::Expr,
        expected: Option<TyId>,
        ctx: &mut Context,
    ) -> Result<h::Expr> {
        if ctx.const_context && ctx.template {
            self.validate_template_const_node(e, ctx)?;
        }
        self.prepare_inference(e, expected, ctx)?;
        let dependent = ctx.template
            && (self.dependent(e, ctx) || expected.is_some_and(|t| self.has_params(t)));
        let mut trial = ctx.clone();
        let result = if dependent {
            self.expr_inner(e, expected, &mut trial)
        } else {
            self.expr_inner(e, expected, ctx)
        };
        match result {
            Err(d)
                if dependent
                    && [
                        "E_TYPE",
                        "E_NAME",
                        "E_LAYOUT",
                        "E_PATTERN",
                        "E_INSTANTIATION",
                    ]
                    .contains(&d.code) =>
            {
                self.check_independent_children(e, ctx)?;
                let ty = if let Some(t) = expected {
                    t
                } else if let ast::ExprKind::Cast { ty, .. } = &e.kind {
                    self.ty(ty, ctx)?
                } else {
                    self.unit()
                };
                Ok(h::Expr {
                    ty,
                    span: e.span,
                    kind: h::ExprKind::Intrinsic {
                        name: "template.deferred".into(),
                        arguments: vec![],
                        type_arguments: vec![],
                    },
                })
            }
            Ok(mut v) => {
                if dependent {
                    *ctx = trial;
                }
                if let Some(target) = expected {
                    self.coerce(v.ty, target, e.span)?;
                    if v.ty != target && !matches!(self.types.kind(v.ty), TypeKind::Never) {
                        v = h::Expr {
                            ty: target,
                            span: e.span,
                            kind: h::ExprKind::Adapt(Box::new(v)),
                        };
                    }
                }
                Ok(v)
            }
            Err(d) => Err(d),
        }
    }
    pub(super) fn expr_inner(
        &mut self,
        e: &ast::Expr,
        expected: Option<TyId>,
        ctx: &mut Context,
    ) -> Result<h::Expr> {
        use ast::ExprKind as A;
        let expected = if expected.is_some() {
            expected
        } else {
            self.hint(e, ctx)?
        };
        let span = e.span;
        let (ty, kind) = match &e.kind {
            A::Integer(s) => {
                let (n, explicit) = literal(s, span)?;
                let ty = explicit
                    .map(|i| self.int(i))
                    .or(expected.filter(|t| matches!(self.types.kind(*t), TypeKind::Int(_))))
                    .unwrap_or_else(|| self.int(Integer::I32));
                self.literal_fit(n, ty, false, span)?;
                (ty, h::ExprKind::Integer(n))
            }
            A::Bool(b) => (self.bool(), h::ExprKind::Integer(*b as u64)),
            A::Null => {
                let t = expected
                    .filter(|t| matches!(self.types.kind(*t), TypeKind::Pointer { .. }))
                    .ok_or_else(|| error("E_TYPE", span, "null requires pointer context"))?;
                (t, h::ExprKind::Integer(0))
            }
            A::String(bytes) => {
                let u = self.int(Integer::U8);
                (
                    self.types.intern(TypeKind::Slice {
                        element: u,
                        const_: true,
                    }),
                    h::ExprKind::String(bytes.clone()),
                )
            }
            A::Path(p) => return self.path_expr(p, expected, span, ctx),
            A::Tuple(v) => {
                let expects = expected.and_then(|t| {
                    if let TypeKind::Tuple(v) = self.types.kind(t) {
                        Some(v.clone())
                    } else {
                        None
                    }
                });
                let mut fields = vec![];
                let mut tys = vec![];
                for (i, x) in v.iter().enumerate() {
                    let x = self.expr(x, expects.as_ref().and_then(|v| v.get(i)).copied(), ctx)?;
                    tys.push(x.ty);
                    fields.push((i, x))
                }
                let ty = if v.is_empty() {
                    self.unit()
                } else {
                    self.types.intern(TypeKind::Tuple(tys))
                };
                (
                    ty,
                    if v.is_empty() {
                        h::ExprKind::Unit
                    } else {
                        h::ExprKind::Aggregate {
                            variant: None,
                            fields,
                        }
                    },
                )
            }
            A::Array(v) => {
                let elem = expected.and_then(|t| match self.types.kind(t) {
                    TypeKind::Array(x, _) => Some(*x),
                    _ => None,
                });
                let mut values = vec![];
                let mut elem = elem;
                for x in v {
                    let x = self.expr(x, elem, ctx)?;
                    elem = Some(x.ty);
                    values.push(x)
                }
                let elem =
                    elem.ok_or_else(|| error("E_TYPE", span, "empty array requires type context"))?;
                (
                    self.types.intern(TypeKind::Array(elem, v.len() as u64)),
                    h::ExprKind::Array(values),
                )
            }
            A::Repeat { value, length } => {
                let n = self.const_ast(length, ctx)?;
                let elem = expected.and_then(|t| match self.types.kind(t) {
                    TypeKind::Array(x, _) => Some(*x),
                    _ => None,
                });
                let value = self.expr(value, elem, ctx)?;
                if !self.types.properties(value.ty).copy {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "array repetition requires Copy even at zero length",
                    ));
                }
                (
                    self.types.intern(TypeKind::Array(value.ty, n)),
                    h::ExprKind::Repeat {
                        value: Box::new(value),
                        length: n,
                    },
                )
            }
            A::Record { path, fields } => return self.record(path, fields, expected, span, ctx),
            A::Unary {
                op,
                operand,
                adjacent,
            } => {
                if op == "-"
                    && *adjacent
                    && let A::Integer(s) = &operand.kind
                {
                    let (n, explicit) = literal(s, span)?;
                    let ty = explicit
                        .map(|i| self.int(i))
                        .or(expected.filter(|t| matches!(self.types.kind(*t), TypeKind::Int(_))))
                        .unwrap_or_else(|| self.int(Integer::I32));
                    self.literal_fit(n, ty, true, span)?;
                    let bits = match self.types.kind(ty) {
                        TypeKind::Int(i) => i.bits(),
                        _ => unreachable!(),
                    };
                    let n = 0u64.wrapping_sub(n)
                        & if bits == 64 {
                            u64::MAX
                        } else {
                            (1 << bits) - 1
                        };
                    return Ok(h::Expr {
                        ty,
                        kind: h::ExprKind::Integer(n),
                        span,
                    });
                }
                if op == "&" || op == "&mut" {
                    let mutable = op == "&mut";
                    let old = ctx.place_context;
                    ctx.place_context = true;
                    let value = self.expr(operand, None, ctx)?;
                    ctx.place_context = old;
                    self.place(&value, mutable, ctx)?;

                    let ty = self.types.intern(TypeKind::Ref {
                        target: value.ty,
                        mutable,
                    });
                    (
                        ty,
                        h::ExprKind::Borrow {
                            value: Box::new(value),
                            mutable,
                            reborrow: false,
                        },
                    )
                } else {
                    let value = self.expr(operand, if op == "*" { None } else { expected }, ctx)?;
                    match op.as_str() {
                        "*" => {
                            let target = match self.types.kind(value.ty) {
                                TypeKind::Ref { target, .. } => *target,
                                TypeKind::Pointer { target, space, .. } => {
                                    if ctx.unsafe_depth == 0 {
                                        return Err(error(
                                            "E_UNSAFE",
                                            span,
                                            "raw dereference requires unsafe block",
                                        ));
                                    }
                                    if *space != AddressSpace::Kernel {
                                        return Err(error(
                                            "E_TYPE",
                                            span,
                                            "this address space cannot be ordinarily dereferenced",
                                        ));
                                    }
                                    *target
                                }
                                _ => {
                                    return Err(error(
                                        "E_TYPE",
                                        span,
                                        "dereference requires pointer or reference",
                                    ));
                                }
                            };
                            if (matches!(
                                self.types.kind(value.ty),
                                TypeKind::Ref { mutable: false, .. }
                                    | TypeKind::Pointer { const_: true, .. }
                            ) || self.readonly_chain(&value))
                                && !self.types.properties(target).copy
                                && !ctx.place_context
                                && !ctx.readonly_operation
                            {
                                return Err(error(
                                    "E_TYPE",
                                    span,
                                    "cannot move noncopy value through shared reference",
                                ));
                            }
                            (target, h::ExprKind::Deref(Box::new(value)))
                        }
                        "!" => {
                            let bool_ty = self.bool();
                            self.coerce(value.ty, bool_ty, span)?;
                            (
                                value.ty,
                                h::ExprKind::Unary {
                                    op: op.clone(),
                                    value: Box::new(value),
                                },
                            )
                        }
                        "~" | "-" => {
                            if !matches!(self.types.kind(value.ty), TypeKind::Int(_))
                                || op == "-"
                                    && !matches!(self.types.kind(value.ty),TypeKind::Int(i) if i.signed())
                            {
                                return Err(error(
                                    "E_TYPE",
                                    span,
                                    "integer unary operand required",
                                ));
                            }
                            (
                                value.ty,
                                h::ExprKind::Unary {
                                    op: op.clone(),
                                    value: Box::new(value),
                                },
                            )
                        }
                        _ => return Err(error("E_TYPE", span, "unknown unary operator")),
                    }
                }
            }
            A::Binary { op, left, right } => {
                return self.binary(op, left, right, expected, span, ctx);
            }
            A::Cast { value, ty } => {
                let target = self.ty(ty, ctx)?;
                let value = self.expr(value, None, ctx)?;
                match (self.types.kind(value.ty), self.types.kind(target)) {
                    (TypeKind::Int(_), TypeKind::Int(_)) => {}
                    _ => return Err(error("E_TYPE", span, "unsupported cast")),
                }
                (target, h::ExprKind::Cast(Box::new(value)))
            }
            A::Call { callee, arguments } => {
                return self.call(callee, arguments, expected, span, ctx);
            }
            A::Field {
                base,
                name,
                arguments,
                ..
            } => {
                if !arguments.is_empty() {
                    return Err(error("E_TYPE", span, "method value cannot be captured"));
                }
                let base = self.expr(base, None, ctx)?;
                let readonly = matches!(
                    self.types.kind(base.ty),
                    TypeKind::Ref { mutable: false, .. }
                );
                let field = self.field(base, name, span, ctx)?;
                if readonly
                    && !self.types.properties(field.ty).copy
                    && !ctx.place_context
                    && !ctx.readonly_operation
                {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "cannot move field through shared reference",
                    ));
                }
                return Ok(field);
            }
            A::Index { base, index } => {
                let mut base = self.expr(base, None, ctx)?;
                let shared = matches!(
                    self.types.kind(base.ty),
                    TypeKind::Ref { mutable: false, .. }
                );
                base = self.autoderef(base);
                let k = self.types.kind(base.ty).clone();
                let elem = match k {
                    TypeKind::Array(e, _) => e,
                    TypeKind::Slice { element, .. } => element,
                    _ => return Err(error("E_TYPE", span, "index requires array or slice")),
                };
                if let A::Binary { op, left, right } = &index.kind
                    && (op == ".." || op == "..=")
                {
                    let hint = self.hint(right, ctx)?;
                    let start = self.expr(left, hint, ctx)?;
                    let end = self.expr(right, Some(start.ty), ctx)?;
                    if !matches!(self.types.kind(start.ty), TypeKind::Int(_)) {
                        return Err(error(
                            "E_TYPE",
                            span,
                            "slice range requires integer endpoints",
                        ));
                    }
                    if op == "..=" {
                        return Err(error("E_TYPE", span, "closed ranges cannot slice"));
                    }
                    let constant = match k {
                        TypeKind::Slice { const_, .. } => const_,
                        _ => self.place(&base, true, ctx).is_err(),
                    };
                    let ty = self.types.intern(TypeKind::Slice {
                        element: elem,
                        const_: constant,
                    });
                    return Ok(h::Expr {
                        ty,
                        kind: h::ExprKind::Slice {
                            base: Box::new(base),
                            start: Box::new(start),
                            end: Box::new(end),
                        },
                        span,
                    });
                }
                if (shared || matches!(k, TypeKind::Slice { const_: true, .. }))
                    && !self.types.properties(elem).copy
                    && !ctx.place_context
                    && !ctx.readonly_operation
                {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "cannot move element from readonly view",
                    ));
                }
                let index = self.expr(index, None, ctx)?;
                if !matches!(self.types.kind(index.ty), TypeKind::Int(_)) {
                    return Err(error("E_TYPE", span, "index must be integer"));
                }
                (
                    elem,
                    h::ExprKind::Index {
                        base: Box::new(base),
                        index: Box::new(index),
                    },
                )
            }
            A::Try(value) => {
                let v = self.expr(value, None, ctx)?;
                let TypeKind::Result(ok, err) = self.types.kind(v.ty).clone() else {
                    return Err(error("E_TYPE", span, "? requires Result"));
                };
                let TypeKind::Result(_, outererr) = self.types.kind(ctx.result) else {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "? requires Result-returning function",
                    ));
                };
                self.coerce(err, *outererr, span)?;
                if ctx.in_defer {
                    return Err(error("E_TYPE", span, "? forbidden within defer"));
                }
                (ok, h::ExprKind::Try(Box::new(v)))
            }
            A::Block(b) => return self.block(b, expected, ctx),
            A::Unsafe(b) => {
                ctx.unsafe_depth += 1;
                let body = self.block(b, expected, ctx)?;
                ctx.unsafe_depth -= 1;
                let scope = match body.kind {
                    h::ExprKind::Block { scope, .. } => scope,
                    _ => unreachable!(),
                };
                (
                    body.ty,
                    h::ExprKind::Unsafe {
                        scope,
                        body: Box::new(body),
                    },
                )
            }
            A::If {
                condition,
                then,
                else_,
            } => {
                let b = self.bool();
                let condition = self.expr(condition, Some(b), ctx)?;
                let then = self.block(then, expected, ctx)?;
                let else_ = if let Some(e) = else_ {
                    Some(Box::new(self.expr(e, expected.or(Some(then.ty)), ctx)?))
                } else {
                    None
                };
                let ty = if let Some(e) = &else_ {
                    if matches!(self.types.kind(then.ty), TypeKind::Never) {
                        e.ty
                    } else {
                        self.coerce(e.ty, then.ty, span)?;
                        then.ty
                    }
                } else {
                    let u = self.unit();
                    self.coerce(then.ty, u, span)?;
                    u
                };
                (
                    ty,
                    h::ExprKind::If {
                        condition: Box::new(condition),
                        then: Box::new(then),
                        else_,
                    },
                )
            }
            A::Return(value) => {
                if ctx.in_defer {
                    return Err(error("E_TYPE", span, "return within defer"));
                }
                let value = value
                    .as_ref()
                    .map(|v| self.expr(v, Some(ctx.result), ctx).map(Box::new))
                    .transpose()?;
                if value.is_none() {
                    let unit = self.unit();
                    self.coerce(unit, ctx.result, span)?
                }
                (self.never(), h::ExprKind::Return(value))
            }
            A::Break(value) => {
                let (id, expect) = ctx
                    .loops
                    .last()
                    .copied()
                    .ok_or_else(|| error("E_TYPE", span, "break outside loop"))?;
                let value = value
                    .as_ref()
                    .map(|v| self.expr(v, expect, ctx).map(Box::new))
                    .transpose()?;
                let ty = value.as_ref().map_or_else(|| self.unit(), |v| v.ty);
                if let Some(t) = expect {
                    self.coerce(ty, t, span)?
                } else {
                    ctx.loops.last_mut().unwrap().1 = Some(ty)
                }
                (self.never(), h::ExprKind::Break { id, value })
            }
            A::Continue => {
                let id = ctx
                    .loops
                    .last()
                    .ok_or_else(|| error("E_TYPE", span, "continue outside loop"))?
                    .0;
                (self.never(), h::ExprKind::Continue(id))
            }
            A::Loop(b) => {
                let id = h::LoopId(ctx.next_loop);
                ctx.next_loop += 1;
                ctx.loops.push((id, expected));
                let u = self.unit();
                let body = self.block(b, Some(u), ctx)?;
                let ty = ctx.loops.pop().unwrap().1.unwrap_or_else(|| self.never());
                (
                    ty,
                    h::ExprKind::Loop {
                        id,
                        body: Box::new(body),
                    },
                )
            }
            A::While { condition, body } => {
                let b = self.bool();
                let condition = self.expr(condition, Some(b), ctx)?;
                let id = h::LoopId(ctx.next_loop);
                ctx.next_loop += 1;
                let u = self.unit();
                ctx.loops.push((id, Some(u)));
                let body = self.block(body, Some(u), ctx)?;
                ctx.loops.pop();
                (
                    u,
                    h::ExprKind::While {
                        id,
                        condition: Box::new(condition),
                        body: Box::new(body),
                    },
                )
            }
            A::For {
                move_,
                binding,
                input,
                body,
            } => {
                let input = self.expr(input, None, ctx)?;
                let elem = match self.types.kind(input.ty).clone() {
                    TypeKind::Range(t) if !*move_ => t,
                    TypeKind::Array(t, _) if *move_ => t,
                    TypeKind::Slice { element, const_ } if !*move_ => {
                        self.types.intern(TypeKind::Ref {
                            target: element,
                            mutable: !const_,
                        })
                    }
                    _ => return Err(error("E_TYPE", span, "for requires range,array or slice")),
                };
                let id = h::LoopId(ctx.next_loop);
                ctx.next_loop += 1;
                let u = self.unit();
                ctx.loops.push((id, Some(u)));
                ctx.scopes.push(BTreeMap::new());
                let bind = if binding == "_" {
                    None
                } else {
                    Some(self.bind(binding, elem, false, span, ctx)?)
                };
                let body = self.block(body, Some(u), ctx)?;
                ctx.scopes.pop();
                ctx.loops.pop();
                (
                    u,
                    h::ExprKind::For {
                        id,
                        binding: bind,
                        input: Box::new(input),
                        body: Box::new(body),
                        move_: *move_,
                    },
                )
            }
            A::Match { value, arms } => return self.match_expr(value, arms, expected, span, ctx),
            A::Layout {
                operation,
                ty,
                field,
            } => {
                let t = self.ty(ty, ctx)?;
                if operation == "offsetof" {
                    self.field_access(t, field.as_deref().unwrap(), &ctx.module, span)?;
                }
                if ctx.template && self.has_params(t) {
                    let ty = self.int(Integer::Usize);
                    return Ok(h::Expr {
                        ty,
                        span,
                        kind: h::ExprKind::Intrinsic {
                            name: field.as_ref().map_or_else(
                                || format!("__const.{operation}"),
                                |f| format!("__const.{operation}.{f}"),
                            ),
                            arguments: vec![],
                            type_arguments: vec![t],
                        },
                    });
                }
                self.prepare_type(t)?;
                let l = crate::layout::LayoutEngine::new(&mut self.types).layout(t)?;
                let n = match operation.as_str() {
                    "sizeof" => l.size,
                    "alignof" => l.align,
                    "offsetof" => {
                        let fields = self.fields(t, None, span)?;
                        let i = fields
                            .iter()
                            .position(|f| Some(&f.name) == field.as_ref())
                            .ok_or_else(|| error("E_NAME", span, "unknown layout field"))?;
                        l.fields[i]
                    }
                    _ => unreachable!(),
                };
                (self.int(Integer::Usize), h::ExprKind::Integer(n))
            }
            A::Asm {
                template,
                operands,
                effects,
            } => {
                if ctx.unsafe_depth == 0 {
                    return Err(error("E_UNSAFE", span, "asm requires unsafe block"));
                }
                let mut inputs = vec![];
                let mut outputs = vec![];
                let mut clobbers = vec![];
                let mut registers: BTreeMap<String, u8> = BTreeMap::new();
                for o in operands {
                    let r = String::from_utf8(o.register.clone())
                        .map_err(|_| error("E_TARGET", o.span, "invalid register"))?;
                    let flag = match o.kind.as_str() {
                        "in" => 1,
                        "out" => 2,
                        _ => 4,
                    };
                    let prior = registers.get(&r).copied().unwrap_or(0);
                    if !valid_register(&r) || prior & flag != 0 || (prior | flag) & 6 == 6 {
                        return Err(error(
                            "E_TARGET",
                            o.span,
                            "invalid, reserved or duplicate asm register",
                        ));
                    }
                    registers.insert(r.clone(), prior | flag);
                    match o.kind.as_str() {
                        "in" => {
                            let v = self.expr(o.value.as_ref().unwrap(), None, ctx)?;
                            if !matches!(
                                self.types.kind(v.ty),
                                TypeKind::Int(_) | TypeKind::Bool | TypeKind::Pointer { .. }
                            ) {
                                return Err(error("E_TARGET", o.span, "asm input must be scalar"));
                            }
                            inputs.push((r, v))
                        }
                        "out" => {
                            let v = self.expr(o.value.as_ref().unwrap(), None, ctx)?;
                            self.place(&v, true, ctx)?;
                            if !matches!(
                                self.types.kind(v.ty),
                                TypeKind::Int(_) | TypeKind::Bool | TypeKind::Pointer { .. }
                            ) {
                                return Err(error("E_TARGET", o.span, "asm output must be scalar"));
                            }
                            outputs.push((r, v))
                        }
                        _ => clobbers.push(r),
                    }
                }
                if template.contains(&0) {
                    return Err(error("E_TARGET", span, "asm template contains NUL"));
                }
                if effects.iter().any(|e| e != "may_block") || effects.len() > 1 {
                    return Err(error("E_TARGET", span, "invalid asm effects"));
                }
                (
                    self.unit(),
                    h::ExprKind::Asm {
                        template: String::from_utf8(template.clone())
                            .map_err(|_| error("E_TARGET", span, "asm template must be UTF8"))?,
                        inputs,
                        outputs,
                        clobbers,
                        may_block: !effects.is_empty(),
                    },
                )
            }
        };
        let result = h::Expr { ty, kind, span };
        if let Some(t) = expected {
            self.coerce(ty, t, span)?;
            if ty != t && !matches!(self.types.kind(ty), TypeKind::Never) {
                return Ok(h::Expr {
                    ty: t,
                    kind: h::ExprKind::Adapt(Box::new(result)),
                    span,
                });
            }
        }
        Ok(result)
    }
    pub(super) fn literal_fit(&self, n: u64, ty: TyId, negative: bool, span: Span) -> Result<()> {
        let TypeKind::Int(i) = self.types.kind(ty) else {
            return Err(error(
                "E_TYPE",
                span,
                "integer literal requires integer type",
            ));
        };
        if negative && !i.signed() {
            return Err(error("E_TYPE", span, "unsigned integer cannot be negated"));
        }
        let max = if i.signed() {
            (1u64 << (i.bits() - 1)) - (!negative as u64)
        } else if i.bits() == 64 {
            u64::MAX
        } else {
            (1 << i.bits()) - 1
        };
        if n > max {
            return Err(error("E_TYPE", span, "integer literal out of range"));
        }
        Ok(())
    }
    pub(super) fn block(
        &mut self,
        b: &ast::Block,
        expected: Option<TyId>,
        ctx: &mut Context,
    ) -> Result<h::Expr> {
        let scope = ScopeId(ctx.next_scope);
        ctx.next_scope += 1;
        ctx.scopes.push(BTreeMap::new());
        ctx.scope_ids.push(scope);
        let mut statements = vec![];
        let mut diverges = false;
        for s in &b.statements {
            match s {
                ast::Statement::Empty(_) => {}
                ast::Statement::Binding {
                    mutable,
                    pattern,
                    ty,
                    value,
                    ..
                } => {
                    if *mutable && ctx.const_context {
                        return Err(error(
                            "E_CONST",
                            pattern.span,
                            "var is forbidden in constants",
                        ));
                    }
                    let expect = ty.as_ref().map(|t| self.ty(t, ctx)).transpose()?;
                    let value = self.expr(value, expect, ctx)?;
                    if matches!(
                        self.types.kind(value.ty),
                        TypeKind::Range(_) | TypeKind::Never
                    ) {
                        return Err(error("E_TYPE", value.span, "binding needs an object type"));
                    }
                    let pattern = self.pattern(pattern, value.ty, *mutable, false, ctx)?;
                    if ctx.const_context {
                        self.bind_constant(&pattern, &value, &mut ctx.const_env)?;
                    }
                    statements.push(h::Statement::Bind { pattern, value })
                }
                ast::Statement::Expr(e) => {
                    let e = self.expr(e, None, ctx)?;
                    diverges |= matches!(self.types.kind(e.ty), TypeKind::Never);
                    if matches!(e.kind, h::ExprKind::SpecialTransfer { .. }) {
                        return Err(error(
                            "E_CONTEXT",
                            e.span,
                            "special transfer must be function tail",
                        ));
                    }
                    statements.push(h::Statement::Expr(e))
                }
                ast::Statement::Assert(e) => {
                    let bt = self.bool();
                    let mut assertion_context = ctx.clone();
                    assertion_context.const_context = true;
                    let e = self.expr(e, Some(bt), &mut assertion_context)?;
                    if ctx.const_context {
                        statements.push(h::Statement::Expr(h::Expr {
                            ty: self.unit(),
                            span: e.span,
                            kind: h::ExprKind::Intrinsic {
                                name: "__const.assert".into(),
                                arguments: vec![e],
                                type_arguments: vec![],
                            },
                        }));
                    } else {
                        if self.eval(&e)? != 1 {
                            return Err(error("E_CONST", e.span, "static assertion false"));
                        }
                    }
                }
                ast::Statement::Defer(e) => {
                    let old = ctx.in_defer;
                    ctx.in_defer = true;
                    let outer_loops = std::mem::take(&mut ctx.loops);
                    let expression = if matches!(e.kind, ast::ExprKind::Block(_)) {
                        let u = self.unit();
                        self.expr(e, Some(u), ctx)?
                    } else if matches!(e.kind, ast::ExprKind::Call { .. }) {
                        self.expr(e, None, ctx)?
                    } else {
                        return Err(error("E_TYPE", e.span, "defer requires call or block"));
                    };
                    ctx.in_defer = old;
                    ctx.loops = outer_loops;
                    let id = ctx.defer;
                    ctx.defer += 1;
                    statements.push(h::Statement::Defer {
                        id,
                        span: e.span,
                        expression,
                    })
                }
            }
        }
        let tail = b
            .tail
            .as_ref()
            .map(|e| self.expr(e, expected, ctx).map(Box::new))
            .transpose()?;
        let ty = tail.as_ref().map_or_else(
            || if diverges { self.never() } else { self.unit() },
            |e| e.ty,
        );
        if let Some(t) = expected {
            self.coerce(ty, t, b.span)?
        }
        ctx.scopes.pop();
        ctx.scope_ids.pop();
        Ok(h::Expr {
            ty,
            kind: h::ExprKind::Block {
                scope,
                statements,
                tail,
            },
            span: b.span,
        })
    }
    pub(super) fn equality_type(&mut self, t: TyId) -> bool {
        match self.types.kind(t).clone() {
            TypeKind::Unit
            | TypeKind::Bool
            | TypeKind::Int(_)
            | TypeKind::Pointer { .. }
            | TypeKind::Function(_)
            | TypeKind::Ref { .. } => true,
            TypeKind::Array(t, _) => self.equality_type(t),
            TypeKind::Tuple(v) => v.iter().all(|t| self.equality_type(*t)),
            TypeKind::Option(t) => self.equality_type(t),
            TypeKind::Result(a, b) => self.equality_type(a) && self.equality_type(b),
            TypeKind::Nominal(id, args) => {
                let d = self.types.definition(id).unwrap().clone();
                d.copy
                    && d.fields().iter().all(|f| {
                        let t = self.types.substitute(f.ty, &args);
                        self.equality_type(t)
                    })
            }
            _ => false,
        }
    }
    pub(super) fn autoderef(&self, mut e: h::Expr) -> h::Expr {
        while let TypeKind::Ref { target, .. } = self.types.kind(e.ty) {
            let span = e.span;
            let ty = *target;
            e = h::Expr {
                ty,
                kind: h::ExprKind::Deref(Box::new(e)),
                span,
            }
        }
        e
    }
    pub(super) fn readonly_chain(&self, e: &h::Expr) -> bool {
        match &e.kind {
            h::ExprKind::Deref(v) => {
                matches!(self.types.kind(v.ty), TypeKind::Ref { mutable: false, .. })
                    || self.readonly_chain(v)
            }
            h::ExprKind::Field { base, .. } | h::ExprKind::Index { base, .. } => {
                self.readonly_chain(base)
            }
            h::ExprKind::Adapt(v) => self.readonly_chain(v),
            _ => false,
        }
    }
    pub(super) fn place(&self, e: &h::Expr, mutable: bool, ctx: &Context) -> Result<()> {
        let writable = match &e.kind {
            h::ExprKind::Local(id) => ctx.locals[id.0 as usize].mutable,
            h::ExprKind::Global {
                mutable: mut_,
                external,
                ..
            } => {
                if *external && ctx.unsafe_depth == 0 {
                    return Err(error(
                        "E_UNSAFE",
                        e.span,
                        "external static access requires unsafe",
                    ));
                }
                *mut_
            }
            h::ExprKind::Deref(v) => match self.types.kind(v.ty) {
                TypeKind::Ref { mutable, .. } => *mutable && !self.readonly_chain(v),
                TypeKind::Pointer { const_, .. } => !*const_,
                _ => false,
            },
            h::ExprKind::Field { base, .. } => {
                self.place(base, mutable, ctx)?;
                return Ok(());
            }
            h::ExprKind::Index { base, .. } => match self.types.kind(base.ty) {
                TypeKind::Slice { const_, .. } => !*const_ && !self.readonly_chain(base),
                _ => {
                    self.place(base, mutable, ctx)?;
                    return Ok(());
                }
            },
            _ => return Err(error("E_TYPE", e.span, "expression is not a place")),
        };
        if mutable && !writable {
            return Err(error("E_TYPE", e.span, "place is not writable"));
        }
        Ok(())
    }
    pub(super) fn fields(
        &mut self,
        t: TyId,
        variant: Option<usize>,
        span: Span,
    ) -> Result<Vec<FieldDef>> {
        if let TypeKind::Nominal(id, _) = self.types.kind(t) {
            self.ensure_type_definition(*id)?;
            self.prepare_type(t)?;
        }
        match self.types.kind(t).clone() {
            TypeKind::Tuple(v) => Ok(v
                .iter()
                .enumerate()
                .map(|(i, t)| FieldDef {
                    name: i.to_string(),
                    ty: *t,
                    span,
                })
                .collect()),
            TypeKind::Nominal(id, args) => {
                let d = self
                    .types
                    .definition(id)
                    .ok_or_else(|| error("E_TYPE", span, "undefined nominal type"))?
                    .clone();
                let fs = match d.kind {
                    DefKind::Struct(f) => f,
                    DefKind::Enum(v) => v
                        .get(variant.ok_or_else(|| error("E_TYPE", span, "variant required"))?)
                        .ok_or_else(|| error("E_TYPE", span, "invalid variant"))?
                        .fields
                        .clone(),
                };
                let mut fields = vec![];
                for mut f in fs {
                    f.ty = self.types.substitute(f.ty, &args);
                    if !args.iter().any(|t| self.has_params(*t)) {
                        f.ty = self.materialize(f.ty, span)?;
                    }
                    fields.push(f);
                }
                Ok(fields)
            }
            TypeKind::Option(t) => Ok(if variant == Some(0) {
                vec![FieldDef {
                    name: "0".into(),
                    ty: t,
                    span,
                }]
            } else {
                vec![]
            }),
            TypeKind::Result(ok, err) => Ok(vec![FieldDef {
                name: "0".into(),
                ty: if variant == Some(0) { ok } else { err },
                span,
            }]),
            _ => Err(error("E_TYPE", span, "type has no fields")),
        }
    }
    fn field(&mut self, base: h::Expr, name: &str, span: Span, ctx: &Context) -> Result<h::Expr> {
        let base = self.autoderef(base);
        if name == "len"
            && matches!(
                self.types.kind(base.ty),
                TypeKind::Array(..) | TypeKind::ArrayExpr(..) | TypeKind::Slice { .. }
            )
        {
            return Ok(h::Expr {
                ty: self.int(Integer::Usize),
                kind: h::ExprKind::Len(Box::new(base)),
                span,
            });
        }
        let fs = self.fields(base.ty, None, span)?;
        let i = fs
            .iter()
            .position(|f| f.name == name)
            .ok_or_else(|| error("E_NAME", span, "unknown field"))?;
        if let TypeKind::Nominal(id, _) = self.types.kind(base.ty)
            && let Some(r) = self.resolver.declarations.get(id)
            && ctx.module != r.module
        {
            let public = match &r.decl.kind {
                ast::DeclKind::Struct { fields, .. } => fields.get(i).is_some_and(|f| f.public),
                _ => false,
            };
            if !public {
                return Err(error("E_NAME", span, "private field"));
            }
        }
        Ok(h::Expr {
            ty: fs[i].ty,
            kind: h::ExprKind::Field {
                base: Box::new(base),
                field: i,
            },
            span,
        })
    }
    fn binary(
        &mut self,
        op: &str,
        left: &ast::Expr,
        right: &ast::Expr,
        expected: Option<TyId>,
        span: Span,
        ctx: &mut Context,
    ) -> Result<h::Expr> {
        let assignment = op.ends_with('=') && !matches!(op, "==" | "!=" | "<=" | ">=" | "..=");
        if assignment {
            let old = ctx.place_context;
            ctx.place_context = true;
            let place = self.expr(left, None, ctx)?;
            ctx.place_context = old;
            self.place(&place, true, ctx)?;
            let value = self.expr(right, Some(place.ty), ctx)?;
            if op != "=" && !matches!(self.types.kind(place.ty), TypeKind::Int(_)) {
                return Err(error(
                    "E_TYPE",
                    span,
                    "compound assignment requires integer",
                ));
            }
            return Ok(h::Expr {
                ty: self.unit(),
                kind: h::ExprKind::Assign {
                    op: op.into(),
                    place: Box::new(place),
                    value: Box::new(value),
                },
                span,
            });
        }
        let boolop = op == "&&" || op == "||";
        let compare = ["==", "!=", "<", "<=", ">", ">="].contains(&op);
        let expect = if boolop {
            Some(self.bool())
        } else if compare {
            None
        } else {
            expected.filter(|t| matches!(self.types.kind(*t), TypeKind::Int(_)))
        };
        let reverse = matches!(left.kind, ast::ExprKind::Integer(_))
            && !matches!(right.kind, ast::ExprKind::Integer(_));
        let old_readonly = ctx.readonly_operation;
        if op == "==" || op == "!=" {
            ctx.readonly_operation = true;
        }
        let (a, b) = if reverse {
            let b = self.expr(right, expect, ctx)?;
            let a = self.expr(left, Some(b.ty), ctx)?;
            (a, b)
        } else {
            let a = self.expr(left, expect, ctx)?;
            let b = self.expr(right, Some(a.ty), ctx)?;
            (a, b)
        };
        ctx.readonly_operation = old_readonly;
        self.coerce(b.ty, a.ty, span)?;
        let ty = if op == ".." || op == "..=" {
            if !matches!(self.types.kind(a.ty), TypeKind::Int(_)) {
                return Err(error("E_TYPE", span, "range requires integer endpoints"));
            }
            self.types.intern(TypeKind::Range(a.ty))
        } else if compare {
            if matches!(op, "==" | "!=") && !self.equality_type(a.ty) {
                return Err(error("E_TYPE", span, "type does not support equality"));
            }
            if !matches!(op, "==" | "!=") && !matches!(self.types.kind(a.ty), TypeKind::Int(_)) {
                return Err(error("E_TYPE", span, "comparison requires integer"));
            }
            self.bool()
        } else if boolop {
            self.bool()
        } else {
            if !matches!(self.types.kind(a.ty), TypeKind::Int(_)) {
                return Err(error("E_TYPE", span, "integer operator requires integer"));
            }
            if op.ends_with('?') {
                self.types.intern(TypeKind::Option(a.ty))
            } else {
                a.ty
            }
        };
        Ok(h::Expr {
            ty,
            kind: if op == ".." || op == "..=" {
                h::ExprKind::Range {
                    start: Box::new(a),
                    end: Box::new(b),
                    inclusive: op == "..=",
                }
            } else {
                h::ExprKind::Binary {
                    op: op.into(),
                    left: Box::new(a),
                    right: Box::new(b),
                }
            },
            span,
        })
    }
}
pub(super) fn literal(s: &str, span: Span) -> Result<(u64, Option<Integer>)> {
    let mut digits = s;
    let mut suffix = None;
    for n in [
        "u8", "u16", "u32", "u64", "usize", "i8", "i16", "i32", "i64", "isize",
    ] {
        if let Some(d) = s.strip_suffix(&format!("_{n}")) {
            digits = d;
            suffix = integer(n);
            break;
        }
    }
    let digits = digits.replace('_', "");
    let (radix, s) = if let Some(s) = digits.strip_prefix("0x") {
        (16, s)
    } else if let Some(s) = digits.strip_prefix("0b") {
        (2, s)
    } else {
        (10, digits.as_str())
    };
    let n = u64::from_str_radix(s, radix)
        .map_err(|_| error("E_TYPE", span, "integer literal exceeds target width"))?;
    Ok((n, suffix))
}
fn valid_register(r: &str) -> bool {
    [
        "a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7", "t0", "t1", "t2", "t3", "t4", "t5", "t6",
    ]
    .contains(&r)
}
