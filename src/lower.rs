//! Typed HIR to sequential MIR. Evaluation snapshots and lexical cleanup are
//! explicit; optional analyses observe the same events used for code generation.
use crate::{
    diagnostic::{Diagnostic, Result},
    hir::{self, Expr, ExprKind as E, Pattern, PatternKind as P, Statement},
    ir::{self, *},
    layout::{Layout, LayoutEngine},
    source::Span,
    types::*,
};
use std::collections::BTreeMap;
#[derive(Clone)]
struct ScopeState {
    id: ScopeId,
    defers: Vec<(u32, Expr)>,
    live: Vec<SlotId>,
}
#[derive(Clone)]
struct LoopState {
    exit: BlockId,
    continue_: BlockId,
    depth: usize,
    result: Option<SlotId>,
}
pub fn lower(program: &hir::Program) -> Result<Compilation> {
    let mut types = program.types.clone();
    let mut module = Module {
        functions: vec![],
        data: program.data.clone(),
        externals: program.externals.clone(),
    };
    for function in &program.functions {
        let mut builder = Builder::new(&mut types, &mut module.data, function);
        builder.finish(&function.body)?;
        module.functions.push(builder.function);
    }
    Ok(Compilation { types, module })
}
struct Builder<'a> {
    types: &'a mut TypeContext,
    data: &'a mut Vec<Data>,
    function: ir::Function,
    current: Option<BlockId>,
    scopes: Vec<ScopeState>,
    loops: BTreeMap<u32, LoopState>,
    expression: u32,
    unit: TyId,
    usize_: TyId,
    bool_: TyId,
}
impl<'a> Builder<'a> {
    fn new(types: &'a mut TypeContext, data: &'a mut Vec<Data>, f: &hir::Function) -> Self {
        let unit = types.intern(TypeKind::Unit);
        let usize_ = types.intern(TypeKind::Int(Integer::Usize));
        let bool_ = types.intern(TypeKind::Bool);
        let function = ir::Function {
            definition: f.definition,
            symbol: f.symbol.clone(),
            signature: f.signature.clone(),
            parameters: f.parameters.iter().map(|id| SlotId(id.0)).collect(),
            slots: f
                .locals
                .iter()
                .map(|l| Slot {
                    ty: l.ty,
                    name: Some(l.name.clone()),
                    span: l.span,
                    scope: l.scope,
                })
                .collect(),
            blocks: vec![Block {
                instructions: vec![],
                terminator: Terminator::Unreachable,
                span: f.span,
            }],
            entry: BlockId(0),
            span: f.span,
            special: f.special.clone(),
            exported: f.exported,
            section: f.section.clone(),
        };
        Self {
            types,
            data,
            function,
            current: Some(BlockId(0)),
            scopes: vec![ScopeState {
                id: ScopeId(u32::MAX),
                defers: vec![],
                live: vec![],
            }],
            loops: BTreeMap::new(),
            expression: 0,
            unit,
            usize_,
            bool_,
        }
    }
    fn finish(&mut self, body: &Expr) -> Result<()> {
        for parameter in self.function.parameters.clone() {
            self.scopes[0].live.push(parameter);
            self.event(SemanticEvent::StorageLive(parameter), body.span)
        }
        let value = self.value(body)?;
        if self.current.is_some() {
            let result = self.snapshot(value, self.function.signature.result, body.span);
            if let Operand::Slot(id) = result {
                self.protect(id)
            }
            self.cleanup(0, body.span)?;
            self.terminate(Terminator::Return(Some(result)));
        }
        Ok(())
    }
    fn error(&self, span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new("I_UNSUPPORTED", span, message)
    }
    fn dummy(&self, ty: TyId) -> Operand {
        Operand::Integer { bits: 0, ty }
    }
    fn emit(&mut self, kind: InstructionKind, span: Span) {
        if let Some(b) = self.current {
            self.function.blocks[b.0 as usize]
                .instructions
                .push(Instruction { kind, span });
        }
    }
    fn event(&mut self, event: SemanticEvent, span: Span) {
        self.emit(InstructionKind::Event(event), span)
    }
    fn terminate(&mut self, t: Terminator) {
        if let Some(b) = self.current.take() {
            self.function.blocks[b.0 as usize].terminator = t;
        }
    }
    fn block(&mut self, span: Span) -> BlockId {
        let id = BlockId(self.function.blocks.len() as u32);
        self.function.blocks.push(Block {
            instructions: vec![],
            terminator: Terminator::Unreachable,
            span,
        });
        id
    }
    fn enter(&mut self, b: BlockId) {
        self.current = Some(b)
    }
    fn layout(&mut self, ty: TyId) -> Result<Layout> {
        LayoutEngine::new(self.types).layout(ty)
    }
    fn temporary(&mut self, ty: TyId, span: Span) -> SlotId {
        let id = SlotId(self.function.slots.len() as u32);
        let scope = self.scopes.last().unwrap().id;
        self.function.slots.push(Slot {
            ty,
            name: None,
            span,
            scope,
        });
        self.scopes.last_mut().unwrap().live.push(id);
        self.event(SemanticEvent::StorageLive(id), span);
        id
    }
    fn set(&mut self, dst: SlotId, value: Rvalue, span: Span) {
        self.emit(InstructionKind::Set { dst, value }, span)
    }
    fn compute(&mut self, ty: TyId, value: Rvalue, span: Span) -> Operand {
        let dst = self.temporary(ty, span);
        self.set(dst, value, span);
        Operand::Slot(dst)
    }
    fn snapshot(&mut self, value: Operand, ty: TyId, span: Span) -> Operand {
        if self.current.is_none() {
            return self.dummy(ty);
        }
        self.compute(ty, Rvalue::Use(value), span)
    }
    fn slot_place(&mut self, id: SlotId) -> Result<Place> {
        let ty = self.function.slots[id.0 as usize].ty;
        let align = self.layout(ty)?.align;
        Ok(Place {
            base: PlaceBase::Slot(id),
            offset: 0,
            ty,
            align,
        })
    }
    fn operand_place(&mut self, value: Operand) -> Result<Place> {
        let ty = value.ty(&self.function);
        let align = self.layout(ty)?.align;
        match value {
            Operand::Slot(id) => Ok(Place {
                base: PlaceBase::Slot(id),
                offset: 0,
                ty,
                align,
            }),
            Operand::Symbol { name, addend, .. } if addend >= 0 => Ok(Place {
                base: PlaceBase::Global(name),
                offset: addend as u64,
                ty,
                align,
            }),
            _ => Err(self.error(
                self.function.span,
                "aggregate requires materialized storage",
            )),
        }
    }
    fn read(&mut self, place: Place, span: Span) -> Result<Operand> {
        let ty = place.ty;
        let props = self.types.try_properties(ty)?;
        self.event(
            if props.copy {
                SemanticEvent::Copy(place.clone())
            } else {
                SemanticEvent::Move(place.clone())
            },
            span,
        );
        Ok(self.compute(ty, Rvalue::Load(place), span))
    }
    fn inspect_read(&mut self, place: Place, span: Span) -> Operand {
        let ty = place.ty;
        self.event(SemanticEvent::Inspect(place.clone()), span);
        self.compute(ty, Rvalue::Load(place), span)
    }
    fn inspect(&mut self, e: &Expr) -> Result<Operand> {
        if matches!(
            &e.kind,
            E::Local(_) | E::Global { .. } | E::Deref(_) | E::Field { .. } | E::Index { .. }
        ) {
            let place = self.place(e)?;
            Ok(self.inspect_read(place, e.span))
        } else {
            let value = self.value(e)?;
            if let Operand::Slot(slot) = value {
                let place = self.slot_place(slot)?;
                self.event(SemanticEvent::Inspect(place), e.span)
            }
            Ok(value)
        }
    }
    fn address(&mut self, place: Place, ty: TyId, span: Span) -> Operand {
        self.compute(ty, Rvalue::Address(place), span)
    }
    fn pointer_ty(&mut self, ty: TyId) -> TyId {
        self.types.intern(TypeKind::Pointer {
            target: ty,
            space: AddressSpace::Kernel,
            const_: false,
        })
    }
    fn write(&mut self, dst: Place, value: Operand, span: Span) -> Result<()> {
        let size = self.layout(dst.ty)?.size;
        if size == 0 {
            return Ok(());
        }
        if scalar(self.types.kind(dst.ty)) {
            self.emit(InstructionKind::Store { dst, value }, span)
        } else {
            let src = self.operand_place(value)?;
            self.emit(InstructionKind::Copy { dst, src, size }, span)
        }
        Ok(())
    }
    fn binary(&mut self, op: &str, left: Operand, right: Operand, ty: TyId, span: Span) -> Operand {
        self.compute(
            ty,
            Rvalue::Binary {
                op: op.into(),
                left,
                right,
            },
            span,
        )
    }
    fn guard(&mut self, condition: Operand, span: Span) {
        if self.current.is_none() {
            return;
        }
        let yes = self.block(span);
        let no = self.block(span);
        self.terminate(Terminator::Branch { condition, yes, no });
        self.enter(no);
        self.terminate(Terminator::Panic(None));
        self.enter(yes)
    }
    fn cleanup(&mut self, depth: usize, span: Span) -> Result<()> {
        let states = self.scopes[depth..].to_vec();
        for state in states.iter().rev() {
            for (id, expr) in state.defers.iter().rev() {
                self.event(
                    SemanticEvent::Defer {
                        source: *id,
                        scope: state.id,
                        registration: false,
                    },
                    expr.span,
                );
                let (full, before) = self.full_begin(expr.span);
                self.value(expr)?;
                self.full_end(full, &before, None, expr.span);
            }
            for id in state.live.iter().rev() {
                self.event(SemanticEvent::StorageDead(*id), span)
            }
        }
        Ok(())
    }
    fn field_place(
        &mut self,
        mut base: Place,
        variant: Option<usize>,
        field: usize,
        ty: TyId,
    ) -> Result<Place> {
        let layout = self.layout(base.ty)?;
        let offset = if let Some(v) = variant {
            layout.payload_offset.unwrap_or(0)
                + layout
                    .variants
                    .get(v)
                    .and_then(|v| v.fields.get(field))
                    .copied()
                    .ok_or_else(|| {
                        self.error(self.function.span, "invalid enum field projection")
                    })?
        } else {
            *layout
                .fields
                .get(field)
                .ok_or_else(|| self.error(self.function.span, "invalid field projection"))?
        };
        base.offset += offset;
        base.ty = ty;
        base.align = base.align.min(self.layout(ty)?.align);
        if offset != 0 {
            base.align = base.align.min(1u64 << offset.trailing_zeros());
        }
        Ok(base)
    }
    fn fields(&mut self, ty: TyId, variant: Option<usize>) -> Result<Vec<TyId>> {
        Ok(match self.types.kind(ty).clone() {
            TypeKind::Tuple(v) => v,
            TypeKind::Nominal(id, args) => {
                let d = self.types.definition(id).unwrap().clone();
                let fields = match d.kind {
                    DefKind::Struct(f) => f,
                    DefKind::Enum(v) => v[variant.unwrap_or(0)].fields.clone(),
                };
                fields
                    .into_iter()
                    .map(|f| self.types.substitute(f.ty, &args))
                    .collect()
            }
            TypeKind::Option(t) => {
                if variant == Some(0) {
                    vec![t]
                } else {
                    vec![]
                }
            }
            TypeKind::Result(a, b) => vec![if variant == Some(0) { a } else { b }],
            TypeKind::MaybeUninit(t) | TypeKind::UnsafeCell(t) | TypeKind::Atomic(t) => vec![t],
            _ => return Err(self.error(self.function.span, "type has no aggregate fields")),
        })
    }
    fn index_value(&mut self, e: &Expr) -> Result<Operand> {
        let value = self.value(e)?;
        if matches!(self.types.kind(e.ty),TypeKind::Int(i) if i.signed()) {
            let valid = self.binary(
                ">=",
                value.clone(),
                Operand::Integer { bits: 0, ty: e.ty },
                self.bool_,
                e.span,
            );
            self.guard(valid, e.span);
        }
        if e.ty == self.usize_ {
            Ok(value)
        } else {
            Ok(self.compute(self.usize_, Rvalue::Cast { value }, e.span))
        }
    }
    fn full_begin(&mut self, span: Span) -> (u32, Vec<SlotId>) {
        let id = self.expression;
        self.expression += 1;
        let before = self
            .scopes
            .iter()
            .flat_map(|scope| scope.live.iter().copied())
            .collect();
        self.event(SemanticEvent::FullExpression { id, entering: true }, span);
        (id, before)
    }
    fn end_temporaries(&mut self, before: &[SlotId], keep: Option<SlotId>, span: Span) {
        let dead: Vec<_> = self
            .scopes
            .iter()
            .flat_map(|scope| scope.live.iter().copied())
            .filter(|slot| {
                !before.contains(slot)
                    && Some(*slot) != keep
                    && self.function.slots[slot.0 as usize].name.is_none()
            })
            .collect();
        for slot in dead.iter().rev() {
            self.event(SemanticEvent::StorageDead(*slot), span)
        }
        for scope in &mut self.scopes {
            scope.live.retain(|slot| !dead.contains(slot));
        }
    }
    fn full_end(&mut self, id: u32, before: &[SlotId], keep: Option<SlotId>, span: Span) {
        self.end_temporaries(before, keep, span);
        self.event(
            SemanticEvent::FullExpression {
                id,
                entering: false,
            },
            span,
        );
    }
    fn full_into(&mut self, e: &Expr, saved: SlotId) -> Result<()> {
        let (id, before) = self.full_begin(e.span);
        let value = self.value(e)?;
        self.set(saved, Rvalue::Use(value), e.span);
        self.full_end(id, &before, None, e.span);
        Ok(())
    }
    fn full_value(&mut self, e: &Expr) -> Result<Operand> {
        // A saved result is owned by the enclosing expression, outside the
        // source full-expression's temporary storage. Consumers later end it.
        let saved = if matches!(self.types.kind(e.ty), TypeKind::Never | TypeKind::Range(_)) {
            None
        } else {
            Some(self.temporary(e.ty, e.span))
        };
        let (id, before) = self.full_begin(e.span);
        let value = self.value(e)?;
        if let Some(saved) = saved {
            self.set(saved, Rvalue::Use(value.clone()), e.span)
        }
        self.full_end(id, &before, None, e.span);
        Ok(saved.map(Operand::Slot).unwrap_or(value))
    }
    fn place(&mut self, e: &Expr) -> Result<Place> {
        match &e.kind {
            E::Local(id) => self.slot_place(SlotId(id.0)),
            E::Global { symbol, .. } => Ok(Place {
                base: PlaceBase::Global(symbol.clone()),
                offset: 0,
                ty: e.ty,
                align: self.layout(e.ty)?.align,
            }),
            E::Deref(value) => {
                let pointer = if matches!(self.types.kind(value.ty), TypeKind::Ref { .. }) {
                    self.inspect(value)?
                } else {
                    self.value(value)?
                };
                Ok(Place {
                    base: PlaceBase::Pointer(pointer),
                    offset: 0,
                    ty: e.ty,
                    align: self.layout(e.ty)?.align,
                })
            }
            E::Field { base, field } => {
                let base = self.place_or_value(base)?;
                self.field_place(base, None, *field, e.ty)
            }
            E::Index { base, index } => {
                let (address, len, element, align) = self.sequence(base)?;
                let idx = self.index_value(index)?;
                let condition = self.binary("<", idx.clone(), len, self.bool_, e.span);
                self.guard(condition, e.span);
                let stride = self.layout(element)?.size;
                let off = self.binary(
                    "*",
                    idx,
                    Operand::Integer {
                        bits: stride,
                        ty: self.usize_,
                    },
                    self.usize_,
                    e.span,
                );
                let pointer_ty = self.pointer_ty(element);
                let pointer = self.binary("+", address, off, pointer_ty, e.span);
                Ok(Place {
                    base: PlaceBase::Pointer(pointer),
                    offset: 0,
                    ty: element,
                    align,
                })
            }
            _ => Err(self.error(e.span, "expression is not a place")),
        }
    }
    fn place_or_value(&mut self, e: &Expr) -> Result<Place> {
        if matches!(
            e.kind,
            E::Local(_) | E::Global { .. } | E::Deref(_) | E::Field { .. } | E::Index { .. }
        ) {
            self.place(e)
        } else {
            let value = self.value(e)?;
            self.operand_place(value)
        }
    }
    fn sequence(&mut self, e: &Expr) -> Result<(Operand, Operand, TyId, u64)> {
        let mut ty = e.ty;
        let mut base = if let TypeKind::Ref { target, .. } = self.types.kind(ty).clone() {
            ty = target;
            let pointer = self.inspect(e)?;
            Place {
                base: PlaceBase::Pointer(pointer),
                offset: 0,
                ty,
                align: self.layout(ty)?.align,
            }
        } else {
            self.place_or_value(e)?
        };
        match self.types.kind(ty).clone() {
            TypeKind::Array(element, n) => {
                let ptr = self.pointer_ty(element);
                let address = self.address(base, ptr, e.span);
                Ok((
                    address,
                    Operand::Integer {
                        bits: n,
                        ty: self.usize_,
                    },
                    element,
                    self.layout(element)?.align,
                ))
            }
            TypeKind::Slice { element, .. } => {
                let align = self.layout(element)?.align;
                let pointer_ty = self.pointer_ty(element);
                let mut data = base.clone();
                data.ty = pointer_ty;
                data.align = 8;
                let address = self.inspect_read(data, e.span);
                base.offset += 8;
                base.ty = self.usize_;
                base.align = 8;
                let len = self.inspect_read(base, e.span);
                Ok((address, len, element, align))
            }
            _ => Err(self.error(e.span, "sequence lowering requires array or slice")),
        }
    }
    fn slice_value(
        &mut self,
        ty: TyId,
        address: Operand,
        len: Operand,
        span: Span,
    ) -> Result<Operand> {
        let dst = self.temporary(ty, span);
        let mut p = self.slot_place(dst)?;
        p.ty = address.ty(&self.function);
        self.write(p.clone(), address, span)?;
        p.offset = 8;
        p.ty = self.usize_;
        self.write(p, len, span)?;
        Ok(Operand::Slot(dst))
    }
    fn tag(&mut self, base: Place, span: Span) -> Result<Operand> {
        let ty = self.types.intern(TypeKind::Int(Integer::U32));
        let mut tag = base;
        tag.ty = ty;
        tag.align = tag.align.min(4);
        self.read(tag, span)
    }
    fn bind_pattern(&mut self, p: &Pattern, base: Place) -> Result<()> {
        match &p.kind {
            P::Bind(id) => {
                let slot = SlotId(id.0);
                self.scopes.last_mut().unwrap().live.push(slot);
                self.event(SemanticEvent::StorageLive(slot), p.span);
                let value = self.read(base, p.span)?;
                let dst = self.slot_place(slot)?;
                self.write(dst, value, p.span)?
            }
            P::BorrowBind { local, mutable } => {
                let slot = SlotId(local.0);
                self.scopes.last_mut().unwrap().live.push(slot);
                self.event(SemanticEvent::StorageLive(slot), p.span);
                self.event(
                    SemanticEvent::Borrow {
                        place: base.clone(),
                        mutable: *mutable,
                        reborrow: true,
                        result: slot,
                    },
                    p.span,
                );
                self.set(slot, Rvalue::Address(base), p.span);
            }
            P::Fields { variant, fields } => {
                let tys = self.fields(base.ty, *variant)?;
                for (i, p) in fields {
                    let field = self.field_place(base.clone(), *variant, *i, tys[*i])?;
                    self.bind_pattern(p, field)?
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn pattern_condition(
        &mut self,
        p: &Pattern,
        base: Place,
        yes: BlockId,
        no: BlockId,
    ) -> Result<()> {
        match &p.kind {
            P::Wildcard | P::Bind(_) | P::BorrowBind { .. } => {
                self.terminate(Terminator::Goto(yes))
            }
            P::Integer(bits) => {
                let value = self.read(base, p.span)?;
                let c = self.binary(
                    "==",
                    value,
                    Operand::Integer {
                        bits: *bits,
                        ty: p.ty,
                    },
                    self.bool_,
                    p.span,
                );
                self.terminate(Terminator::Branch {
                    condition: c,
                    yes,
                    no,
                });
            }
            P::Fields { variant, fields } => {
                if let Some(v) = variant {
                    let tag = self.tag(base.clone(), p.span)?;
                    let next = self.block(p.span);
                    self.terminate(Terminator::Switch {
                        value: tag,
                        cases: vec![(*v as u64, next)],
                        otherwise: no,
                    });
                    self.enter(next);
                }
                let tys = self.fields(base.ty, *variant)?;
                for (index, (i, pattern)) in fields.iter().enumerate() {
                    let next = if index + 1 == fields.len() {
                        yes
                    } else {
                        self.block(p.span)
                    };
                    let field = self.field_place(base.clone(), *variant, *i, tys[*i])?;
                    self.pattern_condition(pattern, field, next, no)?;
                    if next != yes {
                        self.enter(next)
                    }
                }
                if fields.is_empty() {
                    self.terminate(Terminator::Goto(yes))
                }
            }
        }
        Ok(())
    }
    fn value(&mut self, e: &Expr) -> Result<Operand> {
        if self.current.is_none() {
            return Ok(self.dummy(e.ty));
        }
        let value = match &e.kind {
            E::Unit => self.dummy(self.unit),
            E::Integer(bits) => Operand::Integer {
                bits: *bits,
                ty: e.ty,
            },
            E::Function { symbol } => Operand::Symbol {
                name: symbol.clone(),
                addend: 0,
                ty: e.ty,
            },
            E::String(bytes) => {
                let name = format!("__biomiya_string_{}", self.data.len());
                self.data.push(Data {
                    symbol: name.clone(),
                    bytes: if bytes.is_empty() {
                        vec![0]
                    } else {
                        bytes.clone()
                    },
                    relocations: vec![],
                    align: 1,
                    writable: false,
                    exported: false,
                    section: None,
                });
                let byte = self.types.intern(TypeKind::Int(Integer::U8));
                let pointer = self.pointer_ty(byte);
                self.slice_value(
                    e.ty,
                    Operand::Symbol {
                        name,
                        addend: 0,
                        ty: pointer,
                    },
                    Operand::Integer {
                        bits: bytes.len() as u64,
                        ty: self.usize_,
                    },
                    e.span,
                )?
            }
            E::Local(_) | E::Global { .. } | E::Deref(_) | E::Field { .. } | E::Index { .. } => {
                let p = self.place(e)?;
                self.read(p, e.span)?
            }
            E::Unary { op, value } => {
                let value = self.value(value)?;
                self.compute(
                    e.ty,
                    Rvalue::Unary {
                        op: op.clone(),
                        value,
                    },
                    e.span,
                )
            }
            E::Binary { op, left, right } => {
                if op == "&&" || op == "||" {
                    let l = self.value(left)?;
                    let rhs = self.block(e.span);
                    let short = self.block(e.span);
                    let join = self.block(e.span);
                    let dst = self.temporary(e.ty, e.span);
                    self.terminate(Terminator::Branch {
                        condition: l,
                        yes: if op == "&&" { rhs } else { short },
                        no: if op == "&&" { short } else { rhs },
                    });
                    self.enter(short);
                    self.set(
                        dst,
                        Rvalue::Use(Operand::Integer {
                            bits: (op == "||") as u64,
                            ty: e.ty,
                        }),
                        e.span,
                    );
                    self.terminate(Terminator::Goto(join));
                    self.enter(rhs);
                    let r = self.value(right)?;
                    self.set(dst, Rvalue::Use(r), e.span);
                    self.terminate(Terminator::Goto(join));
                    self.enter(join);
                    Operand::Slot(dst)
                } else {
                    let l = if op == "==" || op == "!=" {
                        self.inspect(left)?
                    } else {
                        self.value(left)?
                    };
                    let r = if op == "==" || op == "!=" {
                        self.inspect(right)?
                    } else {
                        self.value(right)?
                    };
                    if (op == "==" || op == "!=") && !scalar(self.types.kind(left.ty)) {
                        let lp = self.operand_place(l)?;
                        let rp = self.operand_place(r)?;
                        let result = self.equal(lp, rp, left.ty, e.span)?;
                        if op == "!=" {
                            self.compute(
                                self.bool_,
                                Rvalue::Unary {
                                    op: "!".into(),
                                    value: result,
                                },
                                e.span,
                            )
                        } else {
                            result
                        }
                    } else {
                        self.binary(op, l, r, e.ty, e.span)
                    }
                }
            }
            E::Assign { op, place, value } => {
                // Assignment evaluates and snapshots RHS before computing the destination.
                let rhs = self.value(value)?;
                if self.current.is_none() {
                    return Ok(self.dummy(e.ty));
                }
                let original = self.place(place)?;
                let pointer = self.pointer_ty(original.ty);
                let address = self.address(original.clone(), pointer, e.span);
                let dst = Place {
                    base: PlaceBase::Pointer(address),
                    offset: 0,
                    ty: original.ty,
                    align: original.align,
                };
                let rhs = if op != "=" {
                    let old = self.read(dst.clone(), e.span)?;
                    self.binary(op.trim_end_matches('='), old, rhs, place.ty, e.span)
                } else {
                    rhs
                };
                self.write(dst, rhs, e.span)?;
                self.dummy(e.ty)
            }
            E::Cast(value) => {
                let v = self.value(value)?;
                self.compute(e.ty, Rvalue::Cast { value: v }, e.span)
            }
            E::Adapt(value) => {
                let result = match (
                    self.types.kind(value.ty).clone(),
                    self.types.kind(e.ty).clone(),
                ) {
                    (
                        TypeKind::Ref {
                            target,
                            mutable: true,
                        },
                        TypeKind::Ref { mutable: false, .. },
                    ) => {
                        let pointer = self.inspect(value)?;
                        let dst = self.temporary(e.ty, e.span);
                        let place = Place {
                            base: PlaceBase::Pointer(pointer.clone()),
                            offset: 0,
                            ty: target,
                            align: self.layout(target)?.align,
                        };
                        self.event(
                            SemanticEvent::Borrow {
                                place,
                                mutable: false,
                                reborrow: true,
                                result: dst,
                            },
                            e.span,
                        );
                        self.set(dst, Rvalue::Use(pointer), e.span);
                        Operand::Slot(dst)
                    }
                    (
                        TypeKind::Slice { const_: false, .. },
                        TypeKind::Slice { const_: true, .. },
                    ) => {
                        let source = self.inspect(value)?;
                        let result = self.compute(e.ty, Rvalue::Use(source.clone()), e.span);
                        if let Operand::Slot(result) = result {
                            self.event(
                                SemanticEvent::ReborrowView {
                                    source,
                                    result,
                                    from: value.ty,
                                    to: e.ty,
                                },
                                e.span,
                            )
                        }
                        result
                    }
                    (TypeKind::Array(..) | TypeKind::Ref { .. }, TypeKind::Slice { .. }) => {
                        let (address, len, _, _) = self.sequence(value)?;
                        self.slice_value(e.ty, address, len, e.span)?
                    }
                    _ => {
                        let value = self.value(value)?;
                        self.compute(e.ty, Rvalue::Use(value), e.span)
                    }
                };
                if let Operand::Slot(result) = result {
                    self.event(
                        SemanticEvent::Conversion {
                            from: value.ty,
                            to: e.ty,
                            result,
                        },
                        e.span,
                    )
                }
                result
            }
            E::Borrow {
                value,
                mutable,
                reborrow,
            } => {
                let place = self.place_or_value(value)?;
                let result = self.temporary(e.ty, e.span);
                self.event(
                    SemanticEvent::Borrow {
                        place: place.clone(),
                        mutable: *mutable,
                        reborrow: *reborrow,
                        result,
                    },
                    e.span,
                );
                self.set(result, Rvalue::Address(place), e.span);
                Operand::Slot(result)
            }
            E::RawAddress { value, .. } => {
                let place = self.place(value)?;
                let result = self.temporary(e.ty, e.span);
                self.event(
                    SemanticEvent::RawAddress {
                        place: place.clone(),
                        result,
                    },
                    e.span,
                );
                self.set(result, Rvalue::Address(place), e.span);
                Operand::Slot(result)
            }
            E::Len(base) => {
                let (_, len, _, _) = self.sequence(base)?;
                len
            }
            E::Slice { base, start, end } => {
                let (address, len, element, _) = self.sequence(base)?;
                let start = self.index_value(start)?;
                let end = self.index_value(end)?;
                let c = self.binary("<=", start.clone(), end.clone(), self.bool_, e.span);
                self.guard(c, e.span);
                let c = self.binary("<=", end.clone(), len, self.bool_, e.span);
                self.guard(c, e.span);
                let stride = self.layout(element)?.size;
                let off = self.binary(
                    "*",
                    start.clone(),
                    Operand::Integer {
                        bits: stride,
                        ty: self.usize_,
                    },
                    self.usize_,
                    e.span,
                );
                let ptr = self.pointer_ty(element);
                let address = self.binary("+", address, off, ptr, e.span);
                let len = self.binary("-", end, start, self.usize_, e.span);
                self.slice_value(e.ty, address, len, e.span)?
            }
            E::Range { .. } => return Err(self.error(e.span, "range value outside for lowering")),
            E::Aggregate { variant, fields } => {
                let dst = self.temporary(e.ty, e.span);
                let base = self.slot_place(dst)?;
                if let Some(v) = variant {
                    let mut tag = base.clone();
                    let u32_ = self.types.intern(TypeKind::Int(Integer::U32));
                    tag.ty = u32_;
                    self.write(
                        tag,
                        Operand::Integer {
                            bits: *v as u64,
                            ty: u32_,
                        },
                        e.span,
                    )?;
                }
                let tys = self.fields(e.ty, *variant)?;
                for (i, field) in fields {
                    let value = self.value(field)?;
                    let p = if matches!(
                        self.types.kind(e.ty),
                        TypeKind::UnsafeCell(_) | TypeKind::MaybeUninit(_) | TypeKind::Atomic(_)
                    ) {
                        Place {
                            ty: tys[*i],
                            ..base.clone()
                        }
                    } else {
                        self.field_place(base.clone(), *variant, *i, tys[*i])?
                    };
                    self.write(p, value, field.span)?;
                }
                Operand::Slot(dst)
            }
            E::Array(values) => {
                let dst = self.temporary(e.ty, e.span);
                let base = self.slot_place(dst)?;
                let element = match self.types.kind(e.ty) {
                    TypeKind::Array(t, _) => *t,
                    _ => return Err(self.error(e.span, "array HIR has wrong type")),
                };
                let layout = self.layout(element)?;
                for (i, v) in values.iter().enumerate() {
                    let value = self.value(v)?;
                    let p = Place {
                        offset: base.offset + i as u64 * layout.size,
                        ty: element,
                        align: layout.align,
                        ..base.clone()
                    };
                    self.write(p, value, v.span)?
                }
                Operand::Slot(dst)
            }
            E::Repeat { value, length } => {
                let v = self.value(value)?;
                let dst = self.temporary(e.ty, e.span);
                let base = self.slot_place(dst)?;
                let layout = self.layout(value.ty)?;
                for i in 0..if layout.size == 0 { 0 } else { *length } {
                    let p = Place {
                        offset: base.offset + i * layout.size,
                        ty: value.ty,
                        align: layout.align,
                        ..base.clone()
                    };
                    self.write(p, v.clone(), e.span)?
                }
                Operand::Slot(dst)
            }
            E::Call {
                callee,
                signature,
                arguments,
            } => {
                let target = if let E::Function { symbol } = &callee.kind {
                    CallTarget::Direct(symbol.clone())
                } else {
                    CallTarget::Indirect(self.value(callee)?)
                };
                let mut args = vec![];
                for arg in arguments {
                    let v = self.value(arg)?;
                    args.push(v)
                }
                let result = if matches!(self.types.kind(e.ty), TypeKind::Never) {
                    None
                } else {
                    Some(self.temporary(e.ty, e.span))
                };
                self.emit(
                    InstructionKind::Call {
                        target,
                        signature: signature.clone(),
                        arguments: args,
                        result,
                    },
                    e.span,
                );
                if let Some(result) = result {
                    Operand::Slot(result)
                } else {
                    self.terminate(Terminator::Unreachable);
                    self.dummy(e.ty)
                }
            }
            E::Intrinsic {
                name,
                arguments,
                type_arguments,
            } => self.intrinsic(e, name, arguments, type_arguments)?,
            E::Block {
                scope,
                statements,
                tail,
            } => {
                let depth = self.scopes.len();
                self.scopes.push(ScopeState {
                    id: *scope,
                    defers: vec![],
                    live: vec![],
                });
                for statement in statements {
                    if self.current.is_none() {
                        break;
                    }
                    match statement {
                        Statement::Bind { pattern, value } => {
                            let (id, before) = self.full_begin(value.span);
                            let v = self.value(value)?;
                            if self.current.is_some() {
                                let materialized = self.materialize(v, value.ty, value.span);
                                let base = self.operand_place(materialized)?;
                                self.bind_pattern(pattern, base)?;
                            }
                            self.full_end(id, &before, None, value.span);
                        }
                        Statement::Expr(expr) => {
                            let (id, before) = self.full_begin(expr.span);
                            self.value(expr)?;
                            self.full_end(id, &before, None, expr.span);
                        }
                        Statement::Defer { id, expression, .. } => {
                            self.scopes
                                .last_mut()
                                .unwrap()
                                .defers
                                .push((*id, expression.clone()));
                            self.event(
                                SemanticEvent::Defer {
                                    source: *id,
                                    scope: *scope,
                                    registration: true,
                                },
                                expression.span,
                            )
                        }
                    }
                }
                let v = if let Some(tail) = tail {
                    self.full_value(tail)?
                } else {
                    self.dummy(e.ty)
                };
                let result = if self.current.is_some() {
                    // result storage belongs to enclosing scope
                    let state = self.scopes.pop().unwrap();
                    let result = self.snapshot(v, e.ty, e.span);
                    self.scopes.push(state);
                    self.cleanup(depth, e.span)?;
                    result
                } else {
                    self.dummy(e.ty)
                };
                self.scopes.truncate(depth);
                result
            }
            E::If {
                condition,
                then,
                else_,
            } => {
                let c = self.full_value(condition)?;
                let yes = self.block(e.span);
                let no = self.block(e.span);
                let join = self.block(e.span);
                let dst = if matches!(self.types.kind(e.ty), TypeKind::Never) {
                    None
                } else {
                    Some(self.temporary(e.ty, e.span))
                };
                self.terminate(Terminator::Branch {
                    condition: c,
                    yes,
                    no,
                });
                self.enter(yes);
                let a = self.value(then)?;
                let live_yes = self.current.is_some();
                if let Some(dst) = dst {
                    self.set(dst, Rvalue::Use(a), e.span)
                }
                self.terminate(Terminator::Goto(join));
                self.enter(no);
                let b = if let Some(other) = else_ {
                    self.value(other)?
                } else {
                    self.dummy(self.unit)
                };
                let live_no = self.current.is_some();
                if let Some(dst) = dst {
                    self.set(dst, Rvalue::Use(b), e.span)
                }
                self.terminate(Terminator::Goto(join));
                if live_yes || live_no {
                    self.enter(join)
                } else {
                    self.current = None
                }
                dst.map(Operand::Slot).unwrap_or_else(|| self.dummy(e.ty))
            }
            E::Match { value, arms } => {
                let match_before: Vec<_> = self
                    .scopes
                    .iter()
                    .flat_map(|scope| scope.live.iter().copied())
                    .collect();
                let base = if matches!(&value.kind,E::Deref(inner) if matches!(self.types.kind(inner.ty),TypeKind::Ref{..}))
                {
                    let (input_full, _) = self.full_begin(value.span);
                    let base = self.place(value)?;
                    let extended: Vec<_> = self
                        .scopes
                        .iter()
                        .flat_map(|scope| scope.live.iter().copied())
                        .collect();
                    self.full_end(input_full, &extended, None, value.span);
                    base
                } else {
                    let v = self.full_value(value)?;
                    let v = self.materialize(v, value.ty, value.span);
                    self.operand_place(v)?
                };
                let join = self.block(e.span);
                let result = if matches!(self.types.kind(e.ty), TypeKind::Never) {
                    None
                } else {
                    Some(self.temporary(e.ty, e.span))
                };
                let mut live = false;
                for (pattern, arm) in arms {
                    let matched = self.block(pattern.span);
                    let next = self.block(pattern.span);
                    self.pattern_condition(pattern, base.clone(), matched, next)?;
                    self.enter(matched);
                    let depth = self.scopes.len();
                    let scope = ScopeId(self.function.slots.len() as u32 + 0x80000000);
                    self.scopes.push(ScopeState {
                        id: scope,
                        defers: vec![],
                        live: vec![],
                    });
                    self.bind_pattern(pattern, base.clone())?;
                    let value = self.value(arm)?;
                    if let Some(dst) = result {
                        self.set(dst, Rvalue::Use(value), arm.span)
                    }
                    if self.current.is_some() {
                        self.cleanup(depth, arm.span)?;
                        live = true;
                    }
                    self.scopes.truncate(depth);
                    self.terminate(Terminator::Goto(join));
                    self.enter(next)
                }
                self.terminate(Terminator::Unreachable);
                if live {
                    self.enter(join)
                }
                self.end_temporaries(&match_before, result, e.span);
                result
                    .map(Operand::Slot)
                    .unwrap_or_else(|| self.dummy(e.ty))
            }
            E::Loop { id, body } => {
                let body_block = self.block(e.span);
                let exit = self.block(e.span);
                let result = if matches!(self.types.kind(e.ty), TypeKind::Never) {
                    None
                } else {
                    Some(self.temporary(e.ty, e.span))
                };
                self.loops.insert(
                    id.0,
                    LoopState {
                        exit,
                        continue_: body_block,
                        depth: self.scopes.len(),
                        result,
                    },
                );
                self.terminate(Terminator::Goto(body_block));
                self.enter(body_block);
                self.value(body)?;
                self.terminate(Terminator::Goto(body_block));
                self.loops.remove(&id.0);
                if result.is_some() {
                    self.enter(exit)
                }
                result
                    .map(Operand::Slot)
                    .unwrap_or_else(|| self.dummy(e.ty))
            }
            E::While {
                id,
                condition,
                body,
            } => {
                let control = self.temporary(self.bool_, condition.span);
                let test = self.block(e.span);
                let body_block = self.block(e.span);
                let exit = self.block(e.span);
                self.loops.insert(
                    id.0,
                    LoopState {
                        exit,
                        continue_: test,
                        depth: self.scopes.len(),
                        result: None,
                    },
                );
                self.terminate(Terminator::Goto(test));
                self.enter(test);
                self.full_into(condition, control)?;
                let c = Operand::Slot(control);
                self.terminate(Terminator::Branch {
                    condition: c,
                    yes: body_block,
                    no: exit,
                });
                self.enter(body_block);
                self.value(body)?;
                self.terminate(Terminator::Goto(test));
                self.enter(exit);
                self.loops.remove(&id.0);
                self.dummy(e.ty)
            }
            E::For {
                id,
                binding,
                input,
                body,
                move_,
            } => {
                self.for_loop(id.0, *binding, input, body, *move_, e.span)?;
                self.dummy(e.ty)
            }
            E::Return(expr) => {
                let v = if let Some(expr) = expr {
                    self.full_value(expr)?
                } else {
                    self.dummy(self.unit)
                };
                let result = self.snapshot(v, self.function.signature.result, e.span);
                if let Operand::Slot(id) = result {
                    self.protect(id)
                }
                self.cleanup(0, e.span)?;
                self.terminate(Terminator::Return(Some(result)));
                self.dummy(e.ty)
            }
            E::Break { id, value } => {
                let state = self
                    .loops
                    .get(&id.0)
                    .cloned()
                    .ok_or_else(|| self.error(e.span, "unresolved loop break"))?;
                let value = if let Some(v) = value {
                    self.full_value(v)?
                } else {
                    self.dummy(self.unit)
                };
                if let Some(dst) = state.result {
                    self.set(dst, Rvalue::Use(value), e.span)
                }
                self.cleanup(state.depth, e.span)?;
                self.terminate(Terminator::Goto(state.exit));
                self.dummy(e.ty)
            }
            E::Continue(id) => {
                let state = self
                    .loops
                    .get(&id.0)
                    .cloned()
                    .ok_or_else(|| self.error(e.span, "unresolved continue"))?;
                self.cleanup(state.depth, e.span)?;
                self.terminate(Terminator::Goto(state.continue_));
                self.dummy(e.ty)
            }
            E::Try(value) => self.try_value(e, value)?,
            E::Unsafe { scope, body } => {
                self.event(
                    SemanticEvent::Unsafe {
                        scope: *scope,
                        entering: true,
                    },
                    e.span,
                );
                let value = self.value(body)?;
                self.event(
                    SemanticEvent::Unsafe {
                        scope: *scope,
                        entering: false,
                    },
                    e.span,
                );
                value
            }
            E::Asm {
                template,
                inputs,
                outputs,
                clobbers,
                may_block,
            } => {
                let mut input_values = vec![];
                for (register, input) in inputs {
                    let value = self.value(input)?;
                    input_values.push(AsmInput {
                        register: register.clone(),
                        value,
                    });
                }
                let mut output_values = vec![];
                for (register, output) in outputs {
                    let place = self.place(output)?;
                    let ptr = self.pointer_ty(place.ty);
                    let address = self.address(place.clone(), ptr, output.span);
                    output_values.push(AsmOutput {
                        register: register.clone(),
                        place: Place {
                            base: PlaceBase::Pointer(address),
                            offset: 0,
                            ty: place.ty,
                            align: place.align,
                        },
                    });
                }
                self.emit(
                    InstructionKind::Asm {
                        template: template.clone(),
                        inputs: input_values,
                        outputs: output_values,
                        clobbers: clobbers.clone(),
                        may_block: *may_block,
                    },
                    e.span,
                );
                self.dummy(e.ty)
            }
            E::SpecialTransfer {
                frame,
                handler,
                irq,
            } => {
                let frame = self.value(frame)?;
                let frame_ty = frame.ty(&self.function);
                let frame = self.snapshot(frame, frame_ty, e.span);
                if let Operand::Slot(id) = frame {
                    self.protect(id)
                }
                self.cleanup(0, e.span)?;
                self.terminate(Terminator::SpecialTransfer {
                    frame,
                    handler: handler.clone(),
                    irq: *irq,
                });
                self.dummy(e.ty)
            }
            E::Uninit => Operand::Slot(self.temporary(e.ty, e.span)),
        };
        Ok(value)
    }
    fn materialize(&mut self, v: Operand, ty: TyId, span: Span) -> Operand {
        if matches!(v, Operand::Slot(_)) {
            v
        } else {
            self.snapshot(v, ty, span)
        }
    }
    fn receiver_place(&mut self, e: &Expr, payload: TyId) -> Result<Place> {
        match self.types.kind(e.ty).clone() {
            TypeKind::Ref { .. } | TypeKind::Pointer { .. } => {
                let pointer = self.value(e)?;
                Ok(Place {
                    base: PlaceBase::Pointer(pointer),
                    offset: 0,
                    ty: payload,
                    align: self.layout(payload)?.align,
                })
            }
            _ => {
                let mut p = self.place_or_value(e)?;
                p.ty = payload;
                p.align = p.align.min(self.layout(payload)?.align);
                Ok(p)
            }
        }
    }
    fn intrinsic(
        &mut self,
        e: &Expr,
        name: &str,
        args: &[Expr],
        types: &[TyId],
    ) -> Result<Operand> {
        match name {
            "panic" => {
                let value = args.first().map(|a| self.value(a)).transpose()?;
                self.terminate(Terminator::Panic(value));
                return Ok(self.dummy(e.ty));
            }
            "sizeof" | "alignof" => {
                let ty = *types
                    .first()
                    .ok_or_else(|| self.error(e.span, "layout query missing type"))?;
                let l = self.layout(ty)?;
                return Ok(Operand::Integer {
                    bits: if name == "sizeof" { l.size } else { l.align },
                    ty: e.ty,
                });
            }
            "MaybeUninit.uninit" => return Ok(Operand::Slot(self.temporary(e.ty, e.span))),
            "MaybeUninit.write" => {
                let payload = *types
                    .first()
                    .ok_or_else(|| self.error(e.span, "MaybeUninit.write missing payload type"))?;
                let p = self.receiver_place(&args[0], payload)?;
                let v = self.value(&args[1])?;
                self.write(p, v, e.span)?;
                return Ok(self.dummy(e.ty));
            }
            "MaybeUninit.as_ptr" | "MaybeUninit.as_mut_ptr" | "UnsafeCell.get" => {
                let payload = *types
                    .first()
                    .ok_or_else(|| self.error(e.span, "storage pointer missing payload type"))?;
                let p = self.receiver_place(&args[0], payload)?;
                return Ok(self.address(p, e.ty, e.span));
            }
            "MaybeUninit.assume_init" => {
                let mut place = self.place_or_value(&args[0])?;
                self.event(SemanticEvent::Move(place.clone()), e.span);
                place.ty = e.ty;
                place.align = place.align.min(self.layout(e.ty)?.align);
                // One ownership transfer consumes the wrapper; the machine read
                // exposes its initialized payload without a second semantic move.
                return Ok(self.compute(e.ty, Rvalue::Load(place), e.span));
            }
            "UnsafeCell.new" | "atomic.new" => {
                let v = self.value(&args[0])?;
                let dst = self.temporary(e.ty, e.span);
                let mut p = self.slot_place(dst)?;
                if name == "atomic.new" {
                    let size = self.layout(e.ty)?.size;
                    self.emit(
                        InstructionKind::Zero {
                            dst: p.clone(),
                            size,
                        },
                        e.span,
                    )
                }
                p.ty = args[0].ty;
                self.write(p, v, e.span)?;
                return Ok(Operand::Slot(dst));
            }
            "split_at" | "split_at_mut" => {
                let original = self.value(&args[0])?;
                let original = self.materialize(original, args[0].ty, args[0].span);
                let slot = match original {
                    Operand::Slot(slot) => slot,
                    _ => unreachable!(),
                };
                let input = Expr {
                    ty: args[0].ty,
                    kind: E::Local(hir::LocalId(slot.0)),
                    span: args[0].span,
                };
                let (address, len, element, _) = self.sequence(&input)?;
                let mid = self.index_value(&args[1])?;
                let c = self.binary("<=", mid.clone(), len.clone(), self.bool_, e.span);
                self.guard(c, e.span);
                let fields = self.fields(e.ty, None)?;
                let left = self.slice_value(fields[0], address.clone(), mid.clone(), e.span)?;
                let stride = self.layout(element)?.size;
                let delta = self.binary(
                    "*",
                    mid.clone(),
                    Operand::Integer {
                        bits: stride,
                        ty: self.usize_,
                    },
                    self.usize_,
                    e.span,
                );
                let pointer = self.pointer_ty(element);
                let right_address = self.binary("+", address, delta, pointer, e.span);
                let right_length = self.binary("-", len, mid, self.usize_, e.span);
                let right = self.slice_value(fields[1], right_address, right_length, e.span)?;
                let dst = self.temporary(e.ty, e.span);
                let base = self.slot_place(dst)?;
                let p = self.field_place(base.clone(), None, 0, fields[0])?;
                self.write(p, left, e.span)?;
                let p = self.field_place(base, None, 1, fields[1])?;
                self.write(p, right, e.span)?;
                self.event(
                    SemanticEvent::SplitSlice {
                        input: original,
                        result: dst,
                    },
                    e.span,
                );
                return Ok(Operand::Slot(dst));
            }
            "slice_from_raw_parts"
            | "slice_from_raw_parts_mut"
            | "slice_from_raw"
            | "slice_from_raw_mut" => {
                let address = self.value(&args[0])?;
                let len = self.value(&args[1])?;
                return self.slice_value(e.ty, address, len, e.span);
            }
            "slice_ptr" | "slice_mut_ptr" | "as_ptr" | "as_mut_ptr" | "slice.as_ptr"
            | "slice.as_mut_ptr" => {
                let (address, _, _, _) = self.sequence(&args[0])?;
                return Ok(self.compute(e.ty, Rvalue::Cast { value: address }, e.span));
            }
            "ptr_add" | "ptr_add_bytes" => {
                let address = self.value(&args[0])?;
                let count = self.value(&args[1])?;
                let count = if name == "ptr_add" {
                    let size = self.layout(types[0])?.size;
                    self.binary(
                        "*%",
                        count,
                        Operand::Integer {
                            bits: size,
                            ty: self.usize_,
                        },
                        self.usize_,
                        e.span,
                    )
                } else {
                    count
                };
                return Ok(self.binary("+%", address, count, e.ty, e.span));
            }
            "replace" | "swap" => {
                let target = types[0];
                let a = self.value(&args[0])?;
                let b = self.value(&args[1])?;
                let pa = Place {
                    base: PlaceBase::Pointer(a),
                    offset: 0,
                    ty: target,
                    align: self.layout(target)?.align,
                };
                let old = self.read(pa.clone(), e.span)?;
                if name == "swap" {
                    let pb = Place {
                        base: PlaceBase::Pointer(b),
                        offset: 0,
                        ty: target,
                        align: self.layout(target)?.align,
                    };
                    let second = self.read(pb.clone(), e.span)?;
                    self.write(pa, second, e.span)?;
                    self.write(pb, old, e.span)?;
                    return Ok(self.dummy(e.ty));
                }
                self.write(pa, b, e.span)?;
                return Ok(old);
            }
            "checked_cast" => return self.checked_cast(e, &args[0], types[0]),
            "addr" | "ptr" => {
                let value = self.value(&args[0])?;
                return Ok(self.compute(e.ty, Rvalue::Cast { value }, e.span));
            }
            "addr_of" | "addr_of_mut" => {
                let p = self.place(&args[0])?;
                let dst = self.temporary(e.ty, e.span);
                self.event(
                    SemanticEvent::RawAddress {
                        place: p.clone(),
                        result: dst,
                    },
                    e.span,
                );
                self.set(dst, Rvalue::Address(p), e.span);
                return Ok(Operand::Slot(dst));
            }
            "forget" => {
                for a in args {
                    self.value(a)?;
                }
                return Ok(self.dummy(e.ty));
            }
            _ => {}
        }
        let mut arguments = vec![];
        for (i, arg) in args.iter().enumerate() {
            if i == 0
                && name.starts_with("atomic.")
                && matches!(self.types.kind(arg.ty), TypeKind::Atomic(_))
            {
                let p = self.place(arg)?;
                let ptr = self.pointer_ty(arg.ty);
                arguments.push(self.address(p, ptr, arg.span));
            } else {
                arguments.push(self.value(arg)?);
            }
        }
        // CSR immediates must remain immediates even though ordinary arguments snapshot.
        let result = if matches!(self.types.kind(e.ty), TypeKind::Unit | TypeKind::Never) {
            None
        } else {
            Some(self.temporary(e.ty, e.span))
        };
        if matches!(
            name,
            "mmio_load" | "mmio_store" | "volatile_load" | "volatile_store"
        ) && types
            .first()
            .is_some_and(|t| self.layout(*t).is_ok_and(|l| l.size == 0))
        {
            return Ok(result
                .map(Operand::Slot)
                .unwrap_or_else(|| self.dummy(e.ty)));
        }
        self.emit(
            InstructionKind::Intrinsic {
                name: name.into(),
                arguments,
                result,
                type_arguments: types.to_vec(),
            },
            e.span,
        );
        Ok(result
            .map(Operand::Slot)
            .unwrap_or_else(|| self.dummy(e.ty)))
    }
    fn checked_cast(&mut self, e: &Expr, source: &Expr, target: TyId) -> Result<Operand> {
        let value = self.value(source)?;
        let from = match self.types.kind(source.ty) {
            TypeKind::Int(i) => *i,
            _ => return Err(self.error(e.span, "checked_cast source must be integer")),
        };
        let to = match self.types.kind(target) {
            TypeKind::Int(i) => *i,
            _ => return Err(self.error(e.span, "checked_cast target must be integer")),
        };
        let mut condition = Operand::Integer {
            bits: 1,
            ty: self.bool_,
        };
        let mut checks = vec![];
        if from.signed() && !to.signed() {
            checks.push(self.binary(
                ">=",
                value.clone(),
                Operand::Integer {
                    bits: 0,
                    ty: source.ty,
                },
                self.bool_,
                e.span,
            ));
        }
        let upper_needed = if !from.signed() && to.signed() {
            from.bits() >= to.bits()
        } else {
            from.bits() > to.bits()
        };
        if upper_needed {
            let max = if to.signed() {
                (1u64 << (to.bits() - 1)) - 1
            } else if to.bits() == 64 {
                u64::MAX
            } else {
                (1u64 << to.bits()) - 1
            };
            checks.push(self.binary(
                "<=",
                value.clone(),
                Operand::Integer {
                    bits: max,
                    ty: source.ty,
                },
                self.bool_,
                e.span,
            ));
        }
        if from.signed() && to.signed() && from.bits() > to.bits() {
            let min = 0u64.wrapping_sub(1u64 << (to.bits() - 1));
            checks.push(self.binary(
                ">=",
                value.clone(),
                Operand::Integer {
                    bits: min,
                    ty: source.ty,
                },
                self.bool_,
                e.span,
            ));
        }
        for c in checks {
            condition = self.binary("&", condition, c, self.bool_, e.span)
        }
        let dst = self.temporary(e.ty, e.span);
        let base = self.slot_place(dst)?;
        let u32_ = self.types.intern(TypeKind::Int(Integer::U32));
        let mut tag = base.clone();
        tag.ty = u32_;
        let success = self.block(e.span);
        let failure = self.block(e.span);
        let join = self.block(e.span);
        self.terminate(Terminator::Branch {
            condition,
            yes: success,
            no: failure,
        });
        self.enter(success);
        self.write(tag.clone(), Operand::Integer { bits: 0, ty: u32_ }, e.span)?;
        let cast = self.compute(target, Rvalue::Cast { value }, e.span);
        let payload = self.field_place(base, Some(0), 0, target)?;
        self.write(payload, cast, e.span)?;
        self.terminate(Terminator::Goto(join));
        self.enter(failure);
        self.write(tag, Operand::Integer { bits: 1, ty: u32_ }, e.span)?;
        self.terminate(Terminator::Goto(join));
        self.enter(join);
        Ok(Operand::Slot(dst))
    }
    fn try_value(&mut self, e: &Expr, source: &Expr) -> Result<Operand> {
        let value = self.value(source)?;
        let base = self.operand_place(value)?;
        let tag = self.tag(base.clone(), e.span)?;
        let success = self.block(e.span);
        let error = self.block(e.span);
        self.terminate(Terminator::Switch {
            value: tag,
            cases: vec![(0, success)],
            otherwise: error,
        });
        self.enter(error);
        let result_ty = self.function.signature.result;
        let dst = self.temporary(result_ty, e.span);
        let dst_base = self.slot_place(dst)?;
        let u32_ = self.types.intern(TypeKind::Int(Integer::U32));
        let mut tag_place = dst_base.clone();
        tag_place.ty = u32_;
        self.write(tag_place, Operand::Integer { bits: 1, ty: u32_ }, e.span)?;
        let source_fields = self.fields(source.ty, Some(1))?;
        let source_error = self.field_place(base.clone(), Some(1), 0, source_fields[0])?;
        let error_value = self.read(source_error, e.span)?;
        let dest_fields = self.fields(result_ty, Some(1))?;
        let dest_error = self.field_place(dst_base, Some(1), 0, dest_fields[0])?;
        self.write(dest_error, error_value, e.span)?;
        self.protect(dst);
        self.cleanup(0, e.span)?;
        self.terminate(Terminator::Return(Some(Operand::Slot(dst))));
        self.enter(success);
        let value = self.field_place(base, Some(0), 0, e.ty)?;
        self.read(value, e.span)
    }
    fn protect(&mut self, id: SlotId) {
        for scope in &mut self.scopes {
            scope.live.retain(|s| *s != id)
        }
    }
    fn equal(&mut self, a: Place, b: Place, ty: TyId, span: Span) -> Result<Operand> {
        if scalar(self.types.kind(ty)) {
            let a = self.inspect_read(a, span);
            let b = self.inspect_read(b, span);
            return Ok(self.binary("==", a, b, self.bool_, span));
        }
        let mut values = vec![];
        match self.types.kind(ty).clone() {
            TypeKind::Array(element, n) => {
                let l = self.layout(element)?;
                for i in 0..if l.size == 0 { 0 } else { n } {
                    let pa = Place {
                        offset: a.offset + i * l.size,
                        ty: element,
                        align: a.align.min(l.align),
                        ..a.clone()
                    };
                    let pb = Place {
                        offset: b.offset + i * l.size,
                        ty: element,
                        align: b.align.min(l.align),
                        ..b.clone()
                    };
                    values.push(self.equal(pa, pb, element, span)?)
                }
            }
            TypeKind::Unit => {}
            TypeKind::Slice { .. } => {
                for offset in [0, 8] {
                    let pa = Place {
                        offset: a.offset + offset,
                        ty: self.usize_,
                        ..a.clone()
                    };
                    let pb = Place {
                        offset: b.offset + offset,
                        ty: self.usize_,
                        ..b.clone()
                    };
                    values.push(self.equal(pa, pb, self.usize_, span)?)
                }
            }
            TypeKind::Option(_) | TypeKind::Result(..) => return self.enum_equal(a, b, ty, span),
            TypeKind::Nominal(id, _)
                if matches!(self.types.definition(id).unwrap().kind, DefKind::Enum(_)) =>
            {
                return self.enum_equal(a, b, ty, span);
            }
            _ => {
                let fields = self.fields(ty, None)?;
                for (i, field) in fields.into_iter().enumerate() {
                    let pa = self.field_place(a.clone(), None, i, field)?;
                    let pb = self.field_place(b.clone(), None, i, field)?;
                    values.push(self.equal(pa, pb, field, span)?);
                }
            }
        }
        let mut result = Operand::Integer {
            bits: 1,
            ty: self.bool_,
        };
        for value in values {
            result = self.binary("&", result, value, self.bool_, span)
        }
        Ok(result)
    }
    fn enum_equal(&mut self, a: Place, b: Place, ty: TyId, span: Span) -> Result<Operand> {
        let ta = self.tag(a.clone(), span)?;
        let tb = self.tag(b.clone(), span)?;
        let same = self.binary("==", ta.clone(), tb, self.bool_, span);
        let compare = self.block(span);
        let different = self.block(span);
        let join = self.block(span);
        let result = self.temporary(self.bool_, span);
        self.terminate(Terminator::Branch {
            condition: same,
            yes: compare,
            no: different,
        });
        self.enter(different);
        self.set(
            result,
            Rvalue::Use(Operand::Integer {
                bits: 0,
                ty: self.bool_,
            }),
            span,
        );
        self.terminate(Terminator::Goto(join));
        self.enter(compare);
        let count = match self.types.kind(ty) {
            TypeKind::Option(_) | TypeKind::Result(..) => 2,
            TypeKind::Nominal(id, _) => match &self.types.definition(*id).unwrap().kind {
                DefKind::Enum(v) => v.len(),
                _ => 0,
            },
            _ => 0,
        };
        let mut cases = vec![];
        for v in 0..count {
            cases.push((v as u64, self.block(span)))
        }
        let invalid = self.block(span);
        self.terminate(Terminator::Switch {
            value: ta,
            cases: cases.clone(),
            otherwise: invalid,
        });
        for (v, block) in cases {
            self.enter(block);
            let fields = self.fields(ty, Some(v as usize))?;
            let mut value = Operand::Integer {
                bits: 1,
                ty: self.bool_,
            };
            for (i, t) in fields.into_iter().enumerate() {
                let pa = self.field_place(a.clone(), Some(v as usize), i, t)?;
                let pb = self.field_place(b.clone(), Some(v as usize), i, t)?;
                let equal = self.equal(pa, pb, t, span)?;
                value = self.binary("&", value, equal, self.bool_, span)
            }
            self.set(result, Rvalue::Use(value), span);
            self.terminate(Terminator::Goto(join));
        }
        self.enter(invalid);
        self.terminate(Terminator::Unreachable);
        self.enter(join);
        Ok(Operand::Slot(result))
    }
    fn for_loop(
        &mut self,
        id: u32,
        binding: Option<hir::LocalId>,
        input: &Expr,
        body: &Expr,
        move_: bool,
        span: Span,
    ) -> Result<()> {
        let (input_full, input_before) = self.full_begin(input.span);
        let (initial, end, inclusive, sequence) = if let E::Range {
            start,
            end,
            inclusive,
        } = &input.kind
        {
            (self.value(start)?, self.value(end)?, *inclusive, None)
        } else {
            let (address, len, element, align) = if move_ {
                let v = self.value(input)?;
                let materialized = self.materialize(v, input.ty, input.span);
                let fake = Expr {
                    ty: input.ty,
                    kind: E::Local(hir::LocalId(match materialized {
                        Operand::Slot(id) => id.0,
                        _ => unreachable!(),
                    })),
                    span: input.span,
                };
                self.sequence(&fake)?
            } else {
                self.sequence(input)?
            };
            (
                Operand::Integer {
                    bits: 0,
                    ty: self.usize_,
                },
                len,
                false,
                Some((address, element, align)),
            )
        };
        let extended: Vec<_> = self
            .scopes
            .iter()
            .flat_map(|scope| scope.live.iter().copied())
            .collect();
        self.full_end(input_full, &extended, None, input.span);
        let index_ty = initial.ty(&self.function);
        let index = self.temporary(index_ty, span);
        self.set(index, Rvalue::Use(initial), span);
        let control = self.temporary(self.bool_, span);
        let test = self.block(span);
        let body_block = self.block(span);
        let step = self.block(span);
        let exit = self.block(span);
        self.loops.insert(
            id,
            LoopState {
                exit,
                continue_: step,
                depth: self.scopes.len(),
                result: None,
            },
        );
        self.terminate(Terminator::Goto(test));
        self.enter(test);
        let (test_full, test_before) = self.full_begin(span);
        let condition = self.binary(
            if inclusive { "<=" } else { "<" },
            Operand::Slot(index),
            end.clone(),
            self.bool_,
            span,
        );
        self.set(control, Rvalue::Use(condition), span);
        self.full_end(test_full, &test_before, None, span);
        self.terminate(Terminator::Branch {
            condition: Operand::Slot(control),
            yes: body_block,
            no: exit,
        });
        self.enter(body_block);
        let iteration_depth = self.scopes.len();
        self.scopes.push(ScopeState {
            id: ScopeId(0x70000000 + id),
            defers: vec![],
            live: vec![],
        });
        if let Some(binding) = binding {
            let slot = SlotId(binding.0);
            self.scopes.last_mut().unwrap().live.push(slot);
            self.event(SemanticEvent::StorageLive(slot), span);
            let dst = self.slot_place(slot)?;
            let value = if let Some((address, element, align)) = &sequence {
                let stride = self.layout(*element)?.size;
                let off = self.binary(
                    "*",
                    Operand::Slot(index),
                    Operand::Integer {
                        bits: stride,
                        ty: self.usize_,
                    },
                    self.usize_,
                    span,
                );
                let ptr = self.pointer_ty(*element);
                let address = self.binary("+", address.clone(), off, ptr, span);
                if matches!(self.types.kind(dst.ty), TypeKind::Ref { .. }) {
                    self.compute(dst.ty, Rvalue::Cast { value: address }, span)
                } else {
                    self.read(
                        Place {
                            base: PlaceBase::Pointer(address),
                            offset: 0,
                            ty: *element,
                            align: *align,
                        },
                        span,
                    )?
                }
            } else {
                Operand::Slot(index)
            };
            self.write(dst, value, span)?;
        }
        self.value(body)?;
        self.cleanup(iteration_depth, span)?;
        self.scopes.truncate(iteration_depth);
        self.terminate(Terminator::Goto(step));
        self.enter(step);

        if inclusive {
            let (final_full, final_before) = self.full_begin(span);
            let done = self.binary("==", Operand::Slot(index), end, self.bool_, span);
            self.set(control, Rvalue::Use(done), span);
            self.full_end(final_full, &final_before, None, span);
            let increment = self.block(span);
            self.terminate(Terminator::Branch {
                condition: Operand::Slot(control),
                yes: exit,
                no: increment,
            });
            self.enter(increment);
        }
        let (step_full, step_before) = self.full_begin(span);
        let next = self.binary(
            "+",
            Operand::Slot(index),
            Operand::Integer {
                bits: 1,
                ty: index_ty,
            },
            index_ty,
            span,
        );
        self.set(index, Rvalue::Use(next), span);
        self.full_end(step_full, &step_before, None, span);
        self.terminate(Terminator::Goto(test));
        self.enter(exit);
        self.loops.remove(&id);
        // The iteration input owns temporaries through the whole structure.
        self.end_temporaries(&input_before, None, span);
        Ok(())
    }
}
fn scalar(ty: &TypeKind) -> bool {
    matches!(
        ty,
        TypeKind::Int(_)
            | TypeKind::Bool
            | TypeKind::Ref { .. }
            | TypeKind::Pointer { .. }
            | TypeKind::Function(_)
    )
}
