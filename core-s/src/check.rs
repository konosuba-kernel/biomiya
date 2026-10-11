//! One semantic pass from source bodies into concrete, ordered CFG operations.
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    ast::{self as a, BinaryOp as Bin, ExprKind as Expr, StatementKind as Stmt},
    diagnostic::{Diagnostic, Result},
    model::*,
    source::Span,
};

#[derive(Clone)]
struct Binding {
    slot: SlotId,
    mutable: bool,
}

#[derive(Clone)]
struct Location {
    place: Place,
    mutable: bool,
}

#[derive(Default)]
struct Scope {
    names: BTreeMap<String, Binding>,
    locals: Vec<SlotId>,
    defers: Vec<Fragment>,
}

#[derive(Clone)]
struct Fragment {
    slots: Vec<Slot>,
    blocks: Vec<Block>,
    slot_base: u32,
    block_base: u32,
    entry: BlockId,
    tail: Option<BlockId>,
    location: Option<Location>,
}

struct Loop {
    exit: BlockId,
    again: BlockId,
    scope_depth: usize,
    has_break: bool,
}

struct Checker<'p, 't> {
    program: &'p ResolvedProgram,
    types: &'t mut TypeTable,
    data: &'t mut Vec<crate::machine::DataObject>,
    names: &'p BTreeMap<String, FunctionId>,
    function: Function,
    current: Option<BlockId>,
    scopes: Vec<Scope>,
    loops: Vec<Loop>,
    unsafe_depth: usize,
    in_defer: bool,
}

fn error(code: &'static str, span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(code, span, message)
}

pub fn check(program: &ResolvedProgram) -> Result<Compilation> {
    let mut types = program.types.clone();
    let names: BTreeMap<_, _> = program
        .functions
        .iter()
        .enumerate()
        .map(|(i, f)| (f.name.clone(), FunctionId(i as u32)))
        .collect();
    if names.len() != program.functions.len() {
        return Err(error(
            "I_INTERNAL",
            Span::default(),
            "duplicate resolved function",
        ));
    }
    let mut data = static_data(program)?;
    let mut functions = Vec::new();
    for (index, source) in program.functions.iter().enumerate() {
        let mut c = Checker {
            program,
            types: &mut types,
            data: &mut data,
            names: &names,
            function: Function {
                name: source.name.clone(),
                symbol: format!("__core_s_f{index}"),
                signature: source.signature.clone(),
                parameter_slots: vec![],
                slots: vec![],
                blocks: vec![],
                entry: BlockId(0),
                span: source.span,
            },
            current: None,
            scopes: vec![Scope::default()],
            loops: vec![],
            unsafe_depth: 0,
            in_defer: false,
        };
        if program.externs.contains_key(&FunctionId(index as u32)) {
            for (i, ty) in source
                .signature
                .parameters
                .iter()
                .chain(std::iter::once(&source.signature.result))
                .enumerate()
            {
                if c.aggregate(*ty)
                    || (i < source.signature.parameters.len()
                        && matches!(c.kind(*ty), TypeKind::Unit))
                {
                    return Err(error(
                        "E_TYPE",
                        source.span,
                        "extern C requires scalar parameters and scalar/unit result",
                    ));
                }
            }
            functions.push(c.function);
            continue;
        }
        let entry = c.block_id(source.span);
        c.function.entry = entry;
        c.current = Some(entry);
        for (parameter, &ty) in source.parameters.iter().zip(&source.signature.parameters) {
            let slot = c.bind(&parameter.name, ty, false, parameter.span)?;
            c.function.parameter_slots.push(slot);
        }
        if source.parameters.len() != source.signature.parameters.len() {
            return Err(error(
                "I_INTERNAL",
                source.span,
                "parameter signature mismatch",
            ));
        }
        if c.block(&source.body)? {
            return Err(error(
                "E_TYPE",
                source.span,
                "function may reach its end; explicit return required",
            ));
        }
        functions.push(c.function);
    }
    Ok(Compilation {
        types,
        functions,
        data,
        externs: program.externs.clone(),
    })
}

impl Checker<'_, '_> {
    fn kind(&self, ty: TypeId) -> &TypeKind {
        &self.types.entries[ty.0 as usize].kind
    }
    fn layout(&self, ty: TypeId) -> &Layout {
        &self.types.entries[ty.0 as usize].layout
    }
    fn builtin(&self, name: &str) -> TypeId {
        self.types.names[name]
    }
    fn source_type(&self, ty: &a::Type) -> Result<TypeId> {
        self.program
            .type_uses
            .get(&ty.span)
            .copied()
            .ok_or_else(|| error("I_INTERNAL", ty.span, "missing resolved source type"))
    }
    fn result_type(&self, ok: TypeId, err: TypeId, span: Span) -> Result<TypeId> {
        self.types
            .entries
            .iter()
            .position(|t| t.kind == TypeKind::Result { ok, error: err })
            .map(|i| TypeId(i as u32))
            .ok_or_else(|| error("I_INTERNAL", span, "missing resolved Result combination"))
    }
    fn op_type(&self, op: &Operand) -> TypeId {
        match op {
            Operand::Slot(s) => self.function.slots[s.0 as usize].ty,
            Operand::Immediate { ty, .. } => *ty,
        }
    }
    fn aggregate(&self, ty: TypeId) -> bool {
        matches!(
            self.kind(ty),
            TypeKind::Struct { .. }
                | TypeKind::Array { .. }
                | TypeKind::Result { .. }
                | TypeKind::Slice { .. }
        )
    }
    fn integer(&self, ty: TypeId) -> bool {
        matches!(self.kind(ty), TypeKind::Int(_))
    }
    fn same(&self, actual: TypeId, expected: TypeId, span: Span) -> Result<()> {
        if actual == expected {
            Ok(())
        } else {
            Err(error("E_TYPE", span, "types must match exactly"))
        }
    }
    fn unit(&self) -> Operand {
        Operand::Immediate {
            bits: 0,
            ty: self.builtin("()"),
        }
    }
    fn immediate(&self, bits: u64, name: &str) -> Operand {
        Operand::Immediate {
            bits,
            ty: self.builtin(name),
        }
    }
    fn block_id(&mut self, span: Span) -> BlockId {
        let id = BlockId(self.function.blocks.len() as u32);
        self.function.blocks.push(Block {
            instructions: vec![],
            terminator: Terminator::Unreachable,
            span,
        });
        id
    }
    fn ensure_block(&mut self, span: Span) -> BlockId {
        if let Some(b) = self.current {
            b
        } else {
            let b = self.block_id(span);
            self.current = Some(b);
            b
        }
    }
    fn emit(&mut self, kind: InstructionKind, span: Span) {
        let b = self.ensure_block(span);
        self.function.blocks[b.0 as usize]
            .instructions
            .push(Instruction { kind, span });
    }
    fn terminate(&mut self, term: Terminator, span: Span) {
        let b = self.ensure_block(span);
        self.function.blocks[b.0 as usize].terminator = term;
        self.current = None;
    }
    fn jump_open(&mut self, to: BlockId, span: Span) {
        if self.current.is_some() {
            self.terminate(Terminator::Jump(to), span);
        }
    }
    fn slot(&mut self, ty: TypeId, name: Option<String>, span: Span) -> SlotId {
        let id = SlotId(self.function.slots.len() as u32);
        self.function.slots.push(Slot { ty, name, span });
        id
    }
    fn local(&self, name: &str) -> Option<Binding> {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.names.get(name).cloned())
    }
    fn bind(&mut self, name: &str, ty: TypeId, mutable: bool, span: Span) -> Result<SlotId> {
        if self.scopes.last().unwrap().names.contains_key(name) {
            return Err(error("E_NAME", span, "duplicate binding in this scope"));
        }
        let slot = self.slot(ty, Some(name.into()), span);
        let scope = self.scopes.last_mut().unwrap();
        scope.names.insert(name.into(), Binding { slot, mutable });
        scope.locals.push(slot);
        self.emit(InstructionKind::StorageLive(slot), span);
        Ok(slot)
    }
    fn slot_place(&self, slot: SlotId) -> Place {
        Place {
            base: PlaceBase::Slot(slot),
            byte_offset: 0,
            ty: self.function.slots[slot.0 as usize].ty,
        }
    }
    fn operand_place(&self, value: &Operand, span: Span) -> Result<Place> {
        if let Operand::Slot(s) = value {
            Ok(self.slot_place(*s))
        } else {
            Err(error("I_INTERNAL", span, "aggregate must occupy a slot"))
        }
    }
    fn write(&mut self, destination: Place, value: Operand, span: Span) -> Result<()> {
        self.same(self.op_type(&value), destination.ty, span)?;
        let kind = if self.aggregate(destination.ty) {
            InstructionKind::Copy {
                destination,
                source: self.operand_place(&value, span)?,
            }
        } else {
            InstructionKind::Store { destination, value }
        };
        self.emit(kind, span);
        Ok(())
    }
    fn read(&mut self, place: Place, span: Span) -> Operand {
        let slot = self.slot(place.ty, None, span);
        if self.aggregate(place.ty) {
            self.emit(
                InstructionKind::Copy {
                    destination: self.slot_place(slot),
                    source: place,
                },
                span,
            );
        } else {
            self.emit(
                InstructionKind::Set {
                    dst: slot,
                    value: Rvalue::Load(place),
                },
                span,
            );
        }
        Operand::Slot(slot)
    }
    fn value(&mut self, ty: TypeId, value: Rvalue, span: Span) -> Operand {
        let slot = self.slot(ty, None, span);
        self.emit(InstructionKind::Set { dst: slot, value }, span);
        Operand::Slot(slot)
    }
    fn require_unsafe(&self, span: Span) -> Result<()> {
        if self.unsafe_depth == 0 {
            Err(error(
                "E_UNSAFE",
                span,
                "raw access requires an unsafe block",
            ))
        } else {
            Ok(())
        }
    }
    fn block(&mut self, body: &a::Block) -> Result<bool> {
        self.scopes.push(Scope::default());
        let mut falls = true;
        for stmt in &body.statements {
            self.ensure_block(stmt.span);
            let next = self.statement(stmt)?;
            falls &= next;
        }
        if falls {
            self.cleanup(self.scopes.len() - 1, body.span)?;
        } else {
            self.current = None;
        }
        self.scopes.pop();
        Ok(falls)
    }
    fn cleanup(&mut self, depth: usize, span: Span) -> Result<()> {
        // Capture the registration list; emitting it never mutates lexical state.
        let actions: Vec<_> = self.scopes[depth..]
            .iter()
            .rev()
            .map(|s| (s.defers.clone(), s.locals.clone()))
            .collect();
        for (defers, locals) in actions {
            for fragment in defers.iter().rev() {
                if self.current.is_none() {
                    return Ok(());
                }
                self.instantiate(fragment, span);
            }
            for slot in locals.into_iter().rev() {
                if self.current.is_some() {
                    self.emit(InstructionKind::StorageDead(slot), span);
                }
            }
        }
        Ok(())
    }
    fn capture(
        &mut self,
        span: Span,
        f: impl FnOnce(&mut Self) -> Result<Option<Location>>,
    ) -> Result<Fragment> {
        let old_current = self.current;
        let slot_base = self.function.slots.len() as u32;
        let block_base = self.function.blocks.len() as u32;
        let entry = self.block_id(span);
        self.current = Some(entry);
        let location = f(self)?;
        let tail = self.current;
        let slots = self.function.slots.split_off(slot_base as usize);
        let blocks = self.function.blocks.split_off(block_base as usize);
        self.current = old_current;
        Ok(Fragment {
            slots,
            blocks,
            slot_base,
            block_base,
            entry,
            tail,
            location,
        })
    }
    fn instantiate(&mut self, fragment: &Fragment, span: Span) -> Option<Location> {
        let mut remap = Remap {
            slot_old: fragment.slot_base,
            slot_new: self.function.slots.len() as u32,
            block_old: fragment.block_base,
            block_new: self.function.blocks.len() as u32,
        };
        self.function.slots.extend(fragment.slots.iter().cloned());
        for block in &fragment.blocks {
            self.function.blocks.push(remap.block(block.clone()));
        }
        self.jump_open(remap.block_id(fragment.entry), span);
        self.current = fragment.tail.map(|b| remap.block_id(b));
        fragment.location.clone().map(|mut l| {
            remap.place(&mut l.place);
            l
        })
    }
    fn statement(&mut self, stmt: &a::Statement) -> Result<bool> {
        let span = stmt.span;
        match &stmt.kind {
            Stmt::Bind {
                name,
                ty,
                mutable,
                value,
            } => {
                if value.is_none() && (!mutable || self.in_defer) {
                    return Err(error("E_TYPE", span, "initializer required here"));
                }
                let ty = self.source_type(ty)?;
                let value = value.as_ref().map(|e| self.expr(e, Some(ty))).transpose()?;
                let slot = self.bind(name, ty, *mutable, span)?;
                if let Some(value) = value {
                    self.write(self.slot_place(slot), value, span)?;
                }
            }
            Stmt::Assign { target, value } => {
                let fragment = self.capture(target.span, |c| c.place(target).map(Some))?;
                let location = fragment.location.as_ref().unwrap();
                if !location.mutable {
                    return Err(error("E_TYPE", target.span, "cannot write immutable place"));
                }
                let rhs = self.expr(value, Some(location.place.ty))?;
                let location = self.instantiate(&fragment, target.span).unwrap();
                self.write(location.place, rhs, span)?;
            }
            Stmt::Expr(e) => {
                self.expr(e, None)?;
            }
            Stmt::Block(b) => return self.block(b),
            Stmt::Unsafe(b) => {
                self.unsafe_depth += 1;
                let result = self.block(b);
                self.unsafe_depth -= 1;
                return result;
            }
            Stmt::If {
                condition,
                then,
                else_,
            } => {
                let condition = self.expr(condition, Some(self.builtin("bool")))?;
                let yes = self.block_id(then.span);
                let no = self.block_id(span);
                let join = self.block_id(span);
                self.terminate(Terminator::Branch { condition, yes, no }, span);
                self.current = Some(yes);
                let a = self.block(then)?;
                self.jump_open(join, span);
                self.current = Some(no);
                let b = if let Some(b) = else_ {
                    self.block(b)?
                } else {
                    true
                };
                self.jump_open(join, span);
                self.current = (a || b).then_some(join);
                return Ok(a || b);
            }
            Stmt::While { condition, body } => {
                self.loop_allowed(span)?;
                let header = self.block_id(span);
                let enter = self.block_id(span);
                let exit = self.block_id(span);
                self.jump_open(header, span);
                self.current = Some(header);
                let condition = self.expr(condition, Some(self.builtin("bool")))?;
                self.terminate(
                    Terminator::Branch {
                        condition,
                        yes: enter,
                        no: exit,
                    },
                    span,
                );
                self.loops.push(Loop {
                    exit,
                    again: header,
                    scope_depth: self.scopes.len(),
                    has_break: false,
                });
                self.current = Some(enter);
                self.block(body)?;
                self.jump_open(header, span);
                self.loops.pop();
                self.current = Some(exit);
            }
            Stmt::Loop(body) => {
                self.loop_allowed(span)?;
                let header = self.block_id(span);
                let exit = self.block_id(span);
                self.jump_open(header, span);
                self.loops.push(Loop {
                    exit,
                    again: header,
                    scope_depth: self.scopes.len(),
                    has_break: false,
                });
                self.current = Some(header);
                self.block(body)?;
                self.jump_open(header, span);
                let falls = self.loops.pop().unwrap().has_break;
                self.current = falls.then_some(exit);
                return Ok(falls);
            }
            Stmt::Match { value, arms } => return self.match_statement(value, arms, span),
            Stmt::Return(e) => {
                if self.in_defer {
                    return Err(error("E_TYPE", span, "return is forbidden in defer"));
                }
                let value = self.expr(e, Some(self.function.signature.result))?;
                self.cleanup(0, span)?;
                if self.current.is_some() {
                    self.terminate(Terminator::Return(value), span);
                }
                return Ok(false);
            }
            Stmt::Break | Stmt::Continue => {
                self.loop_allowed(span)?;
                let is_break = matches!(stmt.kind, Stmt::Break);
                let Some(loop_) = self.loops.last_mut() else {
                    return Err(error("E_TYPE", span, "loop exit outside loop"));
                };
                if is_break {
                    loop_.has_break = true;
                }
                let target = if is_break { loop_.exit } else { loop_.again };
                let depth = loop_.scope_depth;
                self.cleanup(depth, span)?;
                self.jump_open(target, span);
                return Ok(false);
            }
            Stmt::Defer(body) => {
                if self.in_defer {
                    return Err(error("E_TYPE", span, "nested defer is forbidden"));
                }
                self.in_defer = true;
                let fragment = self.capture(span, |c| {
                    c.block(body)?;
                    Ok(None)
                });
                self.in_defer = false;
                self.scopes.last_mut().unwrap().defers.push(fragment?);
            }
            Stmt::Empty => {}
        }
        Ok(true)
    }
    fn loop_allowed(&self, span: Span) -> Result<()> {
        if self.in_defer {
            Err(error(
                "E_TYPE",
                span,
                "loop or loop exit is forbidden in defer",
            ))
        } else {
            Ok(())
        }
    }
}

impl Checker<'_, '_> {
    fn literal(&self, text: &str, expected: Option<TypeId>, span: Span) -> Result<Operand> {
        let mut digits = text;
        let mut ty = None;
        for suffix in [
            "usize", "u32", "u8", "isize", "i64", "i32", "i16", "i8", "u64", "u16",
        ] {
            if let Some(prefix) = text.strip_suffix(&format!("_{suffix}")) {
                digits = prefix;
                ty =
                    Some(*self.types.names.get(suffix).ok_or_else(|| {
                        error("I_UNSUPPORTED", span, "integer width outside pilot")
                    })?);
                break;
            }
        }
        let ty = ty
            .or(expected)
            .ok_or_else(|| error("E_TYPE", span, "integer literal needs known integer type"))?;
        let limit = match self.kind(ty) {
            TypeKind::Int(Integer::U8) => u8::MAX as u64,
            TypeKind::Int(Integer::U32) => u32::MAX as u64,
            TypeKind::Int(Integer::Usize) => u64::MAX,
            _ => {
                return Err(error(
                    "E_TYPE",
                    span,
                    "integer literal in noninteger context",
                ));
            }
        };
        let digits = digits.replace('_', "");
        let (radix, digits) = if let Some(s) = digits.strip_prefix("0x") {
            (16, s)
        } else if let Some(s) = digits.strip_prefix("0b") {
            (2, s)
        } else {
            (10, digits.as_str())
        };
        let bits = u64::from_str_radix(digits, radix)
            .map_err(|_| error("E_TYPE", span, "integer literal out of range"))?;
        if bits > limit {
            return Err(error("E_TYPE", span, "integer literal out of range"));
        }
        if let Some(expected) = expected {
            self.same(ty, expected, span)?;
        }
        Ok(Operand::Immediate { bits, ty })
    }
    fn expr(&mut self, e: &a::Expr, expected: Option<TypeId>) -> Result<Operand> {
        let span = e.span;
        let value = match &e.kind {
            Expr::String(bytes) => {
                let symbol = string_data(self.data, bytes);
                let u8_ = self.builtin("u8");
                let ptr = self.types.pointer(u8_, true, span)?;
                let address = self.value(ptr, Rvalue::SymbolAddress { symbol }, span);
                let len = self.immediate(bytes.len() as u64, "usize");
                self.slice(u8_, true, address, len, span)?
            }
            Expr::Pointer { target, address } => {
                self.require_unsafe(span)?;
                let target = self.source_type(target)?;
                let a = self.expr(address, Some(self.builtin("usize")))?;
                let ty = self.types.pointer(target, false, span)?;
                self.value(ty, Rvalue::Bitcast { value: a }, span)
            }
            Expr::Integer(text) => self.literal(text, expected, span)?,
            Expr::Bool(value) => self.immediate(u64::from(*value), "bool"),
            Expr::Unit => self.unit(),
            Expr::Null => {
                let ty = expected
                    .filter(|&t| matches!(self.kind(t), TypeKind::Pointer { .. }))
                    .ok_or_else(|| error("E_TYPE", span, "null requires a known pointer type"))?;
                Operand::Immediate { bits: 0, ty }
            }
            Expr::Name(name) => {
                if let Some(binding) = self.local(name) {
                    self.read(self.slot_place(binding.slot), span)
                } else if self.program.globals.iter().any(|g| g.name == *name) {
                    let loc = self.global(name, span, false)?;
                    self.read(loc.place, span)
                } else if let Some(c) = self.program.constants.get(name) {
                    Operand::Immediate {
                        bits: c.bits,
                        ty: c.ty,
                    }
                } else if self.names.contains_key(name) {
                    return Err(error(
                        "I_UNSUPPORTED",
                        span,
                        "function values outside pilot",
                    ));
                } else {
                    return Err(error("E_NAME", span, format!("unknown value {name}")));
                }
            }
            Expr::Unary {
                op: a::UnaryOp::Deref,
                ..
            } => {
                let place = self.place(e)?.place;
                self.read(place, span)
            }
            Expr::Unary { op, value } => {
                let want = if *op == a::UnaryOp::Not {
                    Some(self.builtin("bool"))
                } else {
                    expected.filter(|&t| self.integer(t))
                };
                let value = self.expr(value, want)?;
                let ty = self.op_type(&value);
                match op {
                    a::UnaryOp::Not if ty == self.builtin("bool") => {}
                    a::UnaryOp::BitNot if self.integer(ty) => {}
                    _ => {
                        return Err(error(
                            "E_TYPE",
                            span,
                            "invalid unary operand; pilot integers are unsigned",
                        ));
                    }
                }
                self.value(ty, Rvalue::Unary { op: *op, value }, span)
            }
            Expr::Binary { op, left, right } => self.binary(*op, left, right, expected, span)?,
            Expr::Cast { value, ty } => {
                let to = self.source_type(ty)?;
                let value = self.expr(value, None)?;
                if !self.integer(to) || !self.integer(self.op_type(&value)) {
                    return Err(error("E_TYPE", span, "as only converts integers"));
                }
                self.value(to, Rvalue::Cast { value }, span)
            }
            Expr::Call { name, arguments } => self.call(name, arguments, span)?,
            Expr::Field { base, name } => {
                let location = self.projection_base(base)?;
                if let TypeKind::Array { length, .. } = self.kind(location.place.ty) {
                    if name != "len" {
                        return Err(error("E_NAME", span, "array has only len field"));
                    }
                    self.immediate(*length, "usize")
                } else {
                    let field = self.field(location, name, span)?;
                    self.read(field.place, span)
                }
            }
            Expr::Index { base, index } => {
                let location = self.projection_base(base)?;
                let location = self.index(location, index, span)?;
                self.read(location.place, span)
            }
            Expr::Struct { name, fields } => {
                let ty = *self
                    .types
                    .names
                    .get(name)
                    .ok_or_else(|| error("E_NAME", span, "unknown structure"))?;
                let TypeKind::Struct {
                    fields: declared, ..
                } = self.kind(ty).clone()
                else {
                    return Err(error("E_TYPE", span, "constructor requires structure type"));
                };
                let offsets = self.layout(ty).field_offsets.clone();
                let slot = self.slot(ty, None, span);
                let mut seen = BTreeSet::new();
                for (name, expression) in fields {
                    let index = declared
                        .iter()
                        .position(|f| &f.name == name)
                        .ok_or_else(|| error("E_NAME", expression.span, "unknown field"))?;
                    if !seen.insert(index) {
                        return Err(error("E_TYPE", span, "duplicate constructor field"));
                    }
                    let value = self.expr(expression, Some(declared[index].ty))?;
                    self.write(
                        Place {
                            base: PlaceBase::Slot(slot),
                            byte_offset: offsets[index],
                            ty: declared[index].ty,
                        },
                        value,
                        expression.span,
                    )?;
                }
                if seen.len() != declared.len() {
                    return Err(error("E_TYPE", span, "missing constructor field"));
                }
                Operand::Slot(slot)
            }
            Expr::Array(elements) => {
                let (ty, element, length) = self.array_expected(expected, span)?;
                if elements.len() as u64 != length {
                    return Err(error("E_TYPE", span, "array length mismatch"));
                }
                let slot = self.slot(ty, None, span);
                let stride = self.layout(element).size;
                for (i, e) in elements.iter().enumerate() {
                    let value = self.expr(e, Some(element))?;
                    self.write(
                        Place {
                            base: PlaceBase::Slot(slot),
                            byte_offset: i as u64 * stride,
                            ty: element,
                        },
                        value,
                        e.span,
                    )?;
                }
                Operand::Slot(slot)
            }
            Expr::Repeat { value, length } => {
                let (ty, element, count) = self.array_expected(expected, span)?;
                let n = *self
                    .program
                    .repeat_lengths
                    .get(&length.span)
                    .ok_or_else(|| {
                        error("I_INTERNAL", length.span, "missing resolved repeat length")
                    })?;
                if n != count {
                    return Err(error("E_TYPE", span, "repeat length mismatch"));
                }
                let value = self.expr(value, Some(element))?; // Also evaluated for zero repeats.
                let slot = self.slot(ty, None, span);
                self.repeat(slot, element, count, value, span)?;
                Operand::Slot(slot)
            }
            Expr::Result {
                ok,
                error,
                is_ok,
                value,
            } => {
                let ok = self.source_type(ok)?;
                let err = self.source_type(error)?;
                let ty = self.result_type(ok, err, span)?;
                let payload = self.expr(value, Some(if *is_ok { ok } else { err }))?;
                self.make_result(ty, *is_ok, payload, span)?
            }
            Expr::Try(value) => self.try_value(value, span)?,
            Expr::Sizeof(ty) => {
                let ty = self.source_type(ty)?;
                self.immediate(self.layout(ty).size, "usize")
            }
            Expr::Alignof(ty) => {
                let ty = self.source_type(ty)?;
                self.immediate(self.layout(ty).align, "usize")
            }
        };
        if let Some(expected) = expected {
            self.same(self.op_type(&value), expected, span)?;
        }
        Ok(value)
    }
    fn array_expected(
        &self,
        expected: Option<TypeId>,
        span: Span,
    ) -> Result<(TypeId, TypeId, u64)> {
        if let Some(ty) = expected {
            if let TypeKind::Array { element, length } = self.kind(ty) {
                return Ok((ty, *element, *length));
            }
        }
        Err(error(
            "E_TYPE",
            span,
            "array construction requires known array type",
        ))
    }
    fn call(&mut self, name: &str, arguments: &[a::Expr], span: Span) -> Result<Operand> {
        if self.local(name).is_some() || self.program.constants.contains_key(name) {
            return Err(error("E_TYPE", span, "value is not callable"));
        }
        if let Some(&id) = self.names.get(name) {
            let signature = self.program.functions[id.0 as usize].signature.clone();
            if signature.unsafe_ {
                self.require_unsafe(span)?;
            }
            if arguments.len() != signature.parameters.len() {
                return Err(error("E_TYPE", span, "argument count mismatch"));
            }
            let mut values = vec![];
            for (e, ty) in arguments.iter().zip(signature.parameters) {
                values.push(self.expr(e, Some(ty))?);
            }
            let result = if signature.result == self.builtin("()") {
                None
            } else {
                Some(self.slot(signature.result, None, span))
            };
            self.emit(
                InstructionKind::Call {
                    function: id,
                    arguments: values,
                    result,
                },
                span,
            );
            return Ok(result.map(Operand::Slot).unwrap_or_else(|| self.unit()));
        }
        let name = name.strip_prefix("core::").unwrap_or(name);
        if let Some(value) = self.builtin_call(name, arguments, span)? {
            return Ok(value);
        }
        if name == "addr_of" || name == "addr_of_mut" {
            if arguments.len() != 1 {
                return Err(error(
                    "E_TYPE",
                    span,
                    "address operation needs one argument",
                ));
            }
            let location = if let Expr::Name(n) = &arguments[0].kind {
                if self.local(n).is_none() && self.program.globals.iter().any(|g| g.name == *n) {
                    self.global(n, span, true)?
                } else {
                    self.place(&arguments[0])?
                }
            } else {
                self.place(&arguments[0])?
            };
            let mutable = name == "addr_of_mut";
            if mutable && !location.mutable {
                return Err(error("E_TYPE", span, "mutable address of immutable place"));
            }
            let ty = self.types.pointer(location.place.ty, !mutable, span)?;
            return Ok(self.value(ty, Rvalue::Address(location.place), span));
        }
        if [
            "panic",
            "read",
            "write",
            "ptr_const",
            "addr",
            "ptr_add",
            "copy_bytes",
            "fill_bytes",
            "slice_from_raw_parts",
            "slice_from_raw_parts_mut",
            "atomic_load",
            "atomic_store",
            "atomic_compare_exchange",
        ]
        .contains(&name)
        {
            return Err(error("I_UNSUPPORTED", span, "Core S builtin outside pilot"));
        }
        Err(error("E_NAME", span, format!("unknown function {name}")))
    }
    fn binary(
        &mut self,
        op: Bin,
        left: &a::Expr,
        right: &a::Expr,
        expected: Option<TypeId>,
        span: Span,
    ) -> Result<Operand> {
        if matches!(op, Bin::And | Bin::Or) {
            let ty = self.builtin("bool");
            let left = self.expr(left, Some(ty))?;
            let slot = self.slot(ty, None, span);
            self.write(self.slot_place(slot), left.clone(), span)?;
            let rhs = self.block_id(span);
            let end = self.block_id(span);
            let (yes, no) = if op == Bin::And {
                (rhs, end)
            } else {
                (end, rhs)
            };
            self.terminate(
                Terminator::Branch {
                    condition: left,
                    yes,
                    no,
                },
                span,
            );
            self.current = Some(rhs);
            let right = self.expr(right, Some(ty))?;
            self.write(self.slot_place(slot), right, span)?;
            self.jump_open(end, span);
            self.current = Some(end);
            return Ok(Operand::Slot(slot));
        }
        let comparison = matches!(
            op,
            Bin::Eq | Bin::Ne | Bin::Lt | Bin::Le | Bin::Gt | Bin::Ge
        );
        let want = if comparison {
            None
        } else {
            expected.filter(|&t| self.integer(t))
        };
        let left = self.expr(left, want)?;
        let right = self.expr(right, want)?;
        let ty = self.op_type(&left);
        self.same(self.op_type(&right), ty, span)?;
        let equality = matches!(op, Bin::Eq | Bin::Ne);
        if !(self.integer(ty)
            || equality
                && matches!(
                    self.kind(ty),
                    TypeKind::Bool | TypeKind::Unit | TypeKind::Pointer { .. }
                ))
        {
            return Err(error("E_TYPE", span, "invalid binary operand types"));
        }
        let result = if comparison { self.builtin("bool") } else { ty };
        Ok(self.value(result, Rvalue::Binary { op, left, right }, span))
    }
    fn is_place(&self, e: &a::Expr) -> bool {
        match &e.kind {
            Expr::Name(n) => {
                self.local(n).is_some() || self.program.globals.iter().any(|g| g.name == *n)
            }
            Expr::Field { base, .. } | Expr::Index { base, .. } => self.is_place(base),
            Expr::Unary {
                op: a::UnaryOp::Deref,
                ..
            } => true,
            _ => false,
        }
    }
    fn projection_base(&mut self, e: &a::Expr) -> Result<Location> {
        if self.is_place(e) {
            self.place(e)
        } else {
            let value = self.expr(e, None)?;
            if !self.aggregate(self.op_type(&value)) {
                return Err(error("E_TYPE", e.span, "projection requires aggregate"));
            }
            Ok(Location {
                place: self.operand_place(&value, e.span)?,
                mutable: false,
            })
        }
    }
    fn field(&mut self, mut location: Location, name: &str, span: Span) -> Result<Location> {
        if let TypeKind::Slice { element, const_ } = *self.kind(location.place.ty) {
            location.place.ty = match name {
                "data" => self.types.pointer(element, const_, span)?,
                "len" => {
                    location.place.byte_offset += 8;
                    self.builtin("usize")
                }
                _ => return Err(error("E_NAME", span, "slice has only data and len")),
            };
            location.mutable = false;
            return Ok(location);
        }
        let TypeKind::Struct { fields, .. } = self.kind(location.place.ty) else {
            return Err(error(
                "E_TYPE",
                span,
                "field access requires an explicit structure value",
            ));
        };
        let index = fields
            .iter()
            .position(|f| f.name == name)
            .ok_or_else(|| error("E_NAME", span, "unknown field"))?;
        location.place.byte_offset += self.layout(location.place.ty).field_offsets[index];
        location.place.ty = fields[index].ty;
        Ok(location)
    }
    fn place(&mut self, e: &a::Expr) -> Result<Location> {
        let span = e.span;
        match &e.kind {
            Expr::Name(name) => {
                if self.local(name).is_none()
                    && self.program.globals.iter().any(|g| g.name == *name)
                {
                    return self.global(name, span, false);
                }
                let b = self
                    .local(name)
                    .ok_or_else(|| error("E_TYPE", span, "expression has no storage place"))?;
                Ok(Location {
                    place: self.slot_place(b.slot),
                    mutable: b.mutable,
                })
            }
            Expr::Unary {
                op: a::UnaryOp::Deref,
                value,
            } => {
                self.require_unsafe(span)?;
                let pointer = self.expr(value, None)?;
                let TypeKind::Pointer { target, const_ } = *self.kind(self.op_type(&pointer))
                else {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "dereference requires a kernel pointer",
                    ));
                };
                Ok(Location {
                    place: Place {
                        base: PlaceBase::Pointer(pointer),
                        byte_offset: 0,
                        ty: target,
                    },
                    mutable: !const_,
                })
            }
            Expr::Field { base, name } => {
                let location = self.place(base)?;
                self.field(location, name, span)
            }
            Expr::Index { base, index } => {
                let location = self.place(base)?;
                self.index(location, index, span)
            }
            _ => Err(error(
                "E_TYPE",
                span,
                "expression is not an addressable place",
            )),
        }
    }
    fn index(&mut self, location: Location, index: &a::Expr, span: Span) -> Result<Location> {
        if let TypeKind::Slice { element, const_ } = *self.kind(location.place.ty) {
            self.require_unsafe(span)?;
            let p = self.field(location.clone(), "data", span)?;
            let base = self.read(p.place, span);
            let l = self.field(location, "len", span)?;
            let length = self.read(l.place, span);
            let index = self.expr(index, Some(self.builtin("usize")))?;
            self.emit(
                InstructionKind::BoundsDynamic {
                    index: index.clone(),
                    length,
                },
                span,
            );
            let bytes = self.value(
                self.builtin("usize"),
                Rvalue::Binary {
                    op: Bin::Mul,
                    left: index,
                    right: self.immediate(self.layout(element).size, "usize"),
                },
                span,
            );
            let pointer_ty = self.types.pointer(element, const_, span)?;
            let pointer = self.value(pointer_ty, Rvalue::PointerOffset { base, bytes }, span);
            return Ok(Location {
                place: Place {
                    base: PlaceBase::Pointer(pointer),
                    byte_offset: 0,
                    ty: element,
                },
                mutable: !const_,
            });
        }
        let TypeKind::Array { element, length } = *self.kind(location.place.ty) else {
            return Err(error("E_TYPE", span, "index requires fixed array in pilot"));
        };
        // Freeze the base address before the index expression can modify any pointer.
        let base_ty = self
            .types
            .pointer(location.place.ty, !location.mutable, span)?;
        let base = self.value(base_ty, Rvalue::Address(location.place), span);
        let index = self.expr(index, Some(self.builtin("usize")))?;
        self.emit(
            InstructionKind::Bounds {
                index: index.clone(),
                length,
            },
            span,
        );
        let stride = self.immediate(self.layout(element).size, "usize");
        let bytes = self.value(
            self.builtin("usize"),
            Rvalue::Binary {
                op: Bin::Mul,
                left: index,
                right: stride,
            },
            span,
        );
        let pointer_ty = self.types.pointer(element, !location.mutable, span)?;
        let pointer = self.value(pointer_ty, Rvalue::PointerOffset { base, bytes }, span);
        Ok(Location {
            place: Place {
                base: PlaceBase::Pointer(pointer),
                byte_offset: 0,
                ty: element,
            },
            mutable: location.mutable,
        })
    }
    fn repeat(
        &mut self,
        slot: SlotId,
        element: TypeId,
        count: u64,
        value: Operand,
        span: Span,
    ) -> Result<()> {
        if count == 0 {
            return Ok(());
        }
        let usize_ = self.builtin("usize");
        let counter = self.slot(usize_, None, span);
        self.write(self.slot_place(counter), self.immediate(0, "usize"), span)?;
        let pointer_ty = self.types.pointer(element, false, span)?;
        let array_ty = self.function.slots[slot.0 as usize].ty;
        let base_ty = self.types.pointer(array_ty, false, span)?;
        let base = self.value(base_ty, Rvalue::Address(self.slot_place(slot)), span);
        let header = self.block_id(span);
        let body = self.block_id(span);
        let end = self.block_id(span);
        self.jump_open(header, span);
        self.current = Some(header);
        let condition = self.value(
            self.builtin("bool"),
            Rvalue::Binary {
                op: Bin::Lt,
                left: Operand::Slot(counter),
                right: self.immediate(count, "usize"),
            },
            span,
        );
        self.terminate(
            Terminator::Branch {
                condition,
                yes: body,
                no: end,
            },
            span,
        );
        self.current = Some(body);
        let offset = self.value(
            usize_,
            Rvalue::Binary {
                op: Bin::Mul,
                left: Operand::Slot(counter),
                right: self.immediate(self.layout(element).size, "usize"),
            },
            span,
        );
        let pointer = self.value(
            pointer_ty,
            Rvalue::PointerOffset {
                base,
                bytes: offset,
            },
            span,
        );
        self.write(
            Place {
                base: PlaceBase::Pointer(pointer),
                byte_offset: 0,
                ty: element,
            },
            value,
            span,
        )?;
        self.emit(
            InstructionKind::Set {
                dst: counter,
                value: Rvalue::Binary {
                    op: Bin::Add,
                    left: Operand::Slot(counter),
                    right: self.immediate(1, "usize"),
                },
            },
            span,
        );
        self.jump_open(header, span);
        self.current = Some(end);
        Ok(())
    }
    fn make_result(
        &mut self,
        ty: TypeId,
        ok: bool,
        payload: Operand,
        span: Span,
    ) -> Result<Operand> {
        let slot = self.slot(ty, None, span);
        let payload_ty = self.op_type(&payload);
        self.write(
            Place {
                base: PlaceBase::Slot(slot),
                byte_offset: 0,
                ty: self.builtin("u32"),
            },
            self.immediate(u64::from(!ok), "u32"),
            span,
        )?;
        let offset = self
            .layout(ty)
            .payload_offset
            .ok_or_else(|| error("I_INTERNAL", span, "Result missing payload offset"))?;
        self.write(
            Place {
                base: PlaceBase::Slot(slot),
                byte_offset: offset,
                ty: payload_ty,
            },
            payload,
            span,
        )?;
        Ok(Operand::Slot(slot))
    }
    fn result_payload(&self, value: &Operand, payload: TypeId, span: Span) -> Result<Place> {
        let mut place = self.operand_place(value, span)?;
        place.byte_offset += self
            .layout(place.ty)
            .payload_offset
            .ok_or_else(|| error("I_INTERNAL", span, "Result missing payload offset"))?;
        place.ty = payload;
        Ok(place)
    }
    fn try_value(&mut self, e: &a::Expr, span: Span) -> Result<Operand> {
        if self.in_defer {
            return Err(error("E_TYPE", span, "? is forbidden in defer"));
        }
        let value = self.expr(e, None)?;
        let TypeKind::Result { ok, error: err } = *self.kind(self.op_type(&value)) else {
            return Err(error("E_TYPE", span, "? operand must be Result"));
        };
        let result = self.function.signature.result;
        let TypeKind::Result { error: out, .. } = *self.kind(result) else {
            return Err(error("E_TYPE", span, "? requires Result function result"));
        };
        self.same(err, out, span)?;
        let mut tag = self.operand_place(&value, span)?;
        tag.ty = self.builtin("u32");
        let tag = self.read(tag, span);
        let condition = self.value(
            self.builtin("bool"),
            Rvalue::Binary {
                op: Bin::Eq,
                left: tag,
                right: self.immediate(0, "u32"),
            },
            span,
        );
        let yes = self.block_id(span);
        let no = self.block_id(span);
        self.terminate(Terminator::Branch { condition, yes, no }, span);
        self.current = Some(no);
        let payload = self.read(self.result_payload(&value, err, span)?, span);
        let returned = self.make_result(result, false, payload, span)?;
        self.cleanup(0, span)?;
        if self.current.is_some() {
            self.terminate(Terminator::Return(returned), span);
        }
        self.current = Some(yes);
        Ok(self.read(self.result_payload(&value, ok, span)?, span))
    }
}

impl Checker<'_, '_> {
    fn match_statement(
        &mut self,
        expression: &a::Expr,
        arms: &[a::Arm],
        span: Span,
    ) -> Result<bool> {
        let value = self.expr(expression, None)?;
        let ty = self.op_type(&value);
        let kind = self.kind(ty).clone();
        if !matches!(
            kind,
            TypeKind::Bool | TypeKind::Int(_) | TypeKind::Result { .. }
        ) {
            return Err(error(
                "E_PATTERN",
                span,
                "match requires bool, integer or Result",
            ));
        }
        let mut seen = BTreeSet::new();
        let mut wildcard = false;
        let mut patterns = vec![];
        // Pattern metadata is checked once; generation only uses these decisions.
        for arm in arms {
            if wildcard {
                return Err(error("E_PATTERN", arm.pattern.span, "arm after wildcard"));
            }
            let pattern_span = arm.pattern.span;
            let (key, binding) = match &arm.pattern.kind {
                a::PatternKind::Wildcard => {
                    wildcard = true;
                    (None, None)
                }
                a::PatternKind::Bool(b) if kind == TypeKind::Bool => (Some(u64::from(*b)), None),
                a::PatternKind::Integer(text) if self.integer(ty) => {
                    let Operand::Immediate { bits, .. } =
                        self.literal(text, Some(ty), pattern_span)?
                    else {
                        unreachable!()
                    };
                    (Some(bits), None)
                }
                a::PatternKind::Result {
                    ok,
                    error,
                    is_ok,
                    binding,
                } => {
                    let ok = self.source_type(ok)?;
                    let err = self.source_type(error)?;
                    let pattern_ty = self.result_type(ok, err, pattern_span)?;
                    self.same(pattern_ty, ty, pattern_span)?;
                    let payload = if *is_ok { ok } else { err };
                    let binding = if let Some(parameter) = binding {
                        self.same(self.source_type(&parameter.ty)?, payload, parameter.span)?;
                        Some((parameter.name.clone(), payload, parameter.span))
                    } else {
                        None
                    };
                    (Some(u64::from(!is_ok)), binding)
                }
                _ => {
                    return Err(error(
                        "E_PATTERN",
                        pattern_span,
                        "pattern does not match scrutinee type",
                    ));
                }
            };
            if let Some(key) = key {
                if !seen.insert(key) {
                    return Err(error("E_PATTERN", pattern_span, "duplicate pattern"));
                }
            }
            patterns.push((key, binding));
        }
        if !wildcard && (self.integer(ty) || seen.len() != 2) {
            return Err(error("E_PATTERN", span, "match is not exhaustive"));
        }
        let tested = if matches!(kind, TypeKind::Result { .. }) {
            let mut tag = self.operand_place(&value, span)?;
            tag.ty = self.builtin("u32");
            self.read(tag, span)
        } else {
            value.clone()
        };
        let end = self.block_id(span);
        let mut falls = false;
        for (arm, (key, binding)) in arms.iter().zip(patterns) {
            let body = self.block_id(arm.body.span);
            let next = self.block_id(span);
            if let Some(bits) = key {
                let literal = Operand::Immediate {
                    bits,
                    ty: self.op_type(&tested),
                };
                let condition = self.value(
                    self.builtin("bool"),
                    Rvalue::Binary {
                        op: Bin::Eq,
                        left: tested.clone(),
                        right: literal,
                    },
                    arm.pattern.span,
                );
                self.terminate(
                    Terminator::Branch {
                        condition,
                        yes: body,
                        no: next,
                    },
                    span,
                );
            } else {
                self.terminate(Terminator::Jump(body), span);
            }
            self.current = Some(body);
            self.scopes.push(Scope::default());
            if let Some((name, payload, binding_span)) = binding {
                let payload_value = self.read(
                    self.result_payload(&value, payload, binding_span)?,
                    binding_span,
                );
                let slot = self.bind(&name, payload, false, binding_span)?;
                self.write(self.slot_place(slot), payload_value, binding_span)?;
            }
            let arm_falls = self.block(&arm.body)?;
            if arm_falls {
                self.cleanup(self.scopes.len() - 1, span)?;
                self.jump_open(end, span);
            }
            self.scopes.pop();
            falls |= arm_falls;
            self.current = Some(next);
        }
        self.terminate(Terminator::Unreachable, span);
        self.current = falls.then_some(end);
        Ok(falls)
    }
}

/// Fragment-local identifiers move; captures keep their original identity.
struct Remap {
    slot_old: u32,
    slot_new: u32,
    block_old: u32,
    block_new: u32,
}
impl Remap {
    fn slot(&self, s: SlotId) -> SlotId {
        if s.0 < self.slot_old {
            s
        } else {
            SlotId(s.0 - self.slot_old + self.slot_new)
        }
    }
    fn block_id(&self, b: BlockId) -> BlockId {
        BlockId(b.0 - self.block_old + self.block_new)
    }
    fn operand(&self, o: &mut Operand) {
        if let Operand::Slot(s) = o {
            *s = self.slot(*s);
        }
    }
    fn place(&self, p: &mut Place) {
        match &mut p.base {
            PlaceBase::Slot(s) => *s = self.slot(*s),
            PlaceBase::Pointer(o) => self.operand(o),
        }
    }
    fn block(&mut self, mut b: Block) -> Block {
        for instruction in &mut b.instructions {
            match &mut instruction.kind {
                InstructionKind::Set { dst, value } => {
                    *dst = self.slot(*dst);
                    match value {
                        Rvalue::SymbolAddress { .. } => {}
                        Rvalue::Use(o)
                        | Rvalue::Bitcast { value: o }
                        | Rvalue::Cast { value: o }
                        | Rvalue::Unary { value: o, .. } => self.operand(o),
                        Rvalue::Load(p) | Rvalue::Address(p) => self.place(p),
                        Rvalue::Binary { left, right, .. } => {
                            self.operand(left);
                            self.operand(right);
                        }
                        Rvalue::PointerOffset { base, bytes } => {
                            self.operand(base);
                            self.operand(bytes);
                        }
                    }
                }
                InstructionKind::Store { destination, value } => {
                    self.place(destination);
                    self.operand(value);
                }
                InstructionKind::Copy {
                    destination,
                    source,
                } => {
                    self.place(destination);
                    self.place(source);
                }
                InstructionKind::BoundsDynamic { index, length } => {
                    self.operand(index);
                    self.operand(length);
                }
                InstructionKind::Bounds { index, .. } => self.operand(index),
                InstructionKind::Call {
                    arguments, result, ..
                } => {
                    for o in arguments {
                        self.operand(o);
                    }
                    if let Some(s) = result {
                        *s = self.slot(*s);
                    }
                }
                InstructionKind::StorageLive(s) | InstructionKind::StorageDead(s) => {
                    *s = self.slot(*s)
                }
            }
        }
        match &mut b.terminator {
            Terminator::Jump(b) => *b = self.block_id(*b),
            Terminator::Branch { condition, yes, no } => {
                self.operand(condition);
                *yes = self.block_id(*yes);
                *no = self.block_id(*no);
            }
            Terminator::Return(o) => self.operand(o),
            Terminator::Panic | Terminator::Unreachable => {}
        }
        b
    }
}

fn string_data(data: &mut Vec<crate::machine::DataObject>, bytes: &[u8]) -> String {
    let symbol = format!(
        "__core_s_string{}",
        data.iter()
            .filter(|d| d.symbol.starts_with("__core_s_string"))
            .count()
    );
    data.push(crate::machine::DataObject {
        symbol: symbol.clone(),
        mutable: false,
        align: 1,
        bytes: if bytes.is_empty() {
            vec![0]
        } else {
            bytes.to_vec()
        },
        zero_fill: 0,
        relocations: vec![],
    });
    symbol
}
impl Checker<'_, '_> {
    fn global(&mut self, name: &str, span: Span, address_only: bool) -> Result<Location> {
        let (i, g) = self
            .program
            .globals
            .iter()
            .enumerate()
            .find(|(_, g)| g.name == name)
            .ok_or_else(|| error("E_NAME", span, "unknown static"))?;
        let (ty, mutable) = (g.ty, g.mutable);
        if mutable && !address_only {
            self.require_unsafe(span)?;
        }
        let ptr = self.types.pointer(ty, !mutable, span)?;
        let base = self.value(
            ptr,
            Rvalue::SymbolAddress {
                symbol: format!("__core_s_g{i}"),
            },
            span,
        );
        Ok(Location {
            place: Place {
                base: PlaceBase::Pointer(base),
                byte_offset: 0,
                ty,
            },
            mutable,
        })
    }
    fn slice(
        &mut self,
        element: TypeId,
        const_: bool,
        address: Operand,
        len: Operand,
        span: Span,
    ) -> Result<Operand> {
        let ty = self.types.intern(
            TypeKind::Slice { element, const_ },
            Layout {
                size: 16,
                align: 8,
                field_offsets: vec![0, 8],
                payload_offset: None,
            },
            span,
        )?;
        let slot = self.slot(ty, None, span);
        let ptr = self.types.pointer(element, const_, span)?;
        self.write(
            Place {
                base: PlaceBase::Slot(slot),
                byte_offset: 0,
                ty: ptr,
            },
            address,
            span,
        )?;
        self.write(
            Place {
                base: PlaceBase::Slot(slot),
                byte_offset: 8,
                ty: self.builtin("usize"),
            },
            len,
            span,
        )?;
        Ok(Operand::Slot(slot))
    }
    fn builtin_call(
        &mut self,
        name: &str,
        args: &[a::Expr],
        span: Span,
    ) -> Result<Option<Operand>> {
        if ![
            "addr",
            "ptr_const",
            "ptr_add",
            "ptr_add_bytes",
            "slice_of",
            "slice_of_mut",
            "slice_from_raw_parts",
            "slice_from_raw_parts_mut",
            "slice_const",
        ]
        .contains(&name)
        {
            return Ok(None);
        }
        let count = if [
            "ptr_add",
            "ptr_add_bytes",
            "slice_from_raw_parts",
            "slice_from_raw_parts_mut",
        ]
        .contains(&name)
        {
            2
        } else {
            1
        };
        if args.len() != count {
            return Err(error("E_TYPE", span, "builtin argument count mismatch"));
        }
        if name == "slice_of" || name == "slice_of_mut" {
            let loc = self.place(&args[0])?;
            let mutable = name == "slice_of_mut";
            if mutable && !loc.mutable {
                return Err(error("E_TYPE", span, "mutable view requires mutable array"));
            }
            let TypeKind::Array { element, length } = *self.kind(loc.place.ty) else {
                return Err(error("E_TYPE", span, "slice_of requires an array place"));
            };
            let ptr = self.types.pointer(element, !mutable, span)?;
            let address = self.value(
                ptr,
                Rvalue::Address(Place {
                    ty: element,
                    ..loc.place
                }),
                span,
            );
            let len = self.immediate(length, "usize");
            return self.slice(element, !mutable, address, len, span).map(Some);
        }
        let value = self.expr(&args[0], None)?;
        if name == "slice_const" {
            let TypeKind::Slice {
                element,
                const_: false,
            } = *self.kind(self.op_type(&value))
            else {
                return Err(error("E_TYPE", span, "slice_const requires mutable slice"));
            };
            let location = Location {
                place: self.operand_place(&value, span)?,
                mutable: false,
            };
            let p = self.field(location.clone(), "data", span)?;
            let p = self.read(p.place, span);
            let target = self.types.pointer(element, true, span)?;
            let p = self.value(target, Rvalue::Bitcast { value: p }, span);
            let len = self.field(location, "len", span)?;
            let len = self.read(len.place, span);
            return self.slice(element, true, p, len, span).map(Some);
        }
        let TypeKind::Pointer { target, const_ } = *self.kind(self.op_type(&value)) else {
            return Err(error("E_TYPE", span, "builtin requires a pointer"));
        };
        if name == "addr" {
            return Ok(Some(self.value(
                self.builtin("usize"),
                Rvalue::Bitcast { value },
                span,
            )));
        }
        if name == "ptr_const" {
            if const_ {
                return Err(error("E_TYPE", span, "ptr_const requires mutable pointer"));
            }
            let ty = self.types.pointer(target, true, span)?;
            return Ok(Some(self.value(ty, Rvalue::Bitcast { value }, span)));
        }
        self.require_unsafe(span)?;
        let n = self.expr(&args[1], Some(self.builtin("usize")))?;
        if name.starts_with("slice_from") {
            let want_const = name == "slice_from_raw_parts";
            if const_ != want_const {
                return Err(error("E_TYPE", span, "slice pointer permission mismatch"));
            }
            return self.slice(target, const_, value, n, span).map(Some);
        }
        let n = if name == "ptr_add" {
            self.value(
                self.builtin("usize"),
                Rvalue::Binary {
                    op: Bin::Mul,
                    left: n,
                    right: self.immediate(self.layout(target).size, "usize"),
                },
                span,
            )
        } else {
            n
        };
        // Integer addition gives the specified checked address arithmetic.
        let bits = self.value(
            self.builtin("usize"),
            Rvalue::Bitcast {
                value: value.clone(),
            },
            span,
        );
        let bits = self.value(
            self.builtin("usize"),
            Rvalue::Binary {
                op: Bin::Add,
                left: bits,
                right: n,
            },
            span,
        );
        Ok(Some(self.value(
            self.op_type(&value),
            Rvalue::Bitcast { value: bits },
            span,
        )))
    }
}

fn static_data(program: &ResolvedProgram) -> Result<Vec<crate::machine::DataObject>> {
    let mut data = vec![];
    let mut types = program.types.clone();
    let names = BTreeMap::new();
    let unit = types.names["()"];
    let mut c = Checker {
        program,
        types: &mut types,
        data: &mut data,
        names: &names,
        function: Function {
            name: String::new(),
            symbol: String::new(),
            signature: Signature {
                parameters: vec![],
                result: unit,
                unsafe_: false,
            },
            parameter_slots: vec![],
            slots: vec![],
            blocks: vec![],
            entry: BlockId(0),
            span: Span::default(),
        },
        current: None,
        scopes: vec![Scope::default()],
        loops: vec![],
        unsafe_depth: 0,
        in_defer: false,
    };
    for (i, g) in program.globals.iter().enumerate() {
        let size = c.layout(g.ty).size;
        if size > 64 * 1024 * 1024 {
            return Err(error("I_LIMIT", g.span, "static exceeds image limit"));
        }
        let mut object = crate::machine::DataObject {
            symbol: format!("__core_s_g{i}"),
            mutable: g.mutable,
            align: c.layout(g.ty).align,
            bytes: vec![0; size.max(1) as usize],
            zero_fill: 0,
            relocations: vec![],
        };
        c.static_init(&g.initializer, g.ty, 0, &mut object)?;
        if g.mutable && object.relocations.is_empty() && object.bytes.iter().all(|b| *b == 0) {
            object.zero_fill = object.bytes.len() as u64;
            object.bytes.clear();
        }
        c.data.push(object);
    }
    Ok(data)
}
impl Checker<'_, '_> {
    fn static_init(
        &mut self,
        e: &a::Expr,
        ty: TypeId,
        offset: u64,
        out: &mut crate::machine::DataObject,
    ) -> Result<()> {
        let span = e.span;
        match &e.kind {
            Expr::Integer(_) | Expr::Bool(_) | Expr::Unit | Expr::Null | Expr::Name(_) => {
                if let Expr::Name(n) = &e.kind {
                    if !self.program.constants.contains_key(n) {
                        return Err(error(
                            "E_CONST",
                            span,
                            "static values cannot read other statics",
                        ));
                    }
                }
                let value = self.expr(e, Some(ty))?;
                let Operand::Immediate { bits, .. } = value else {
                    return Err(error("E_CONST", span, "initializer must be constant"));
                };
                let size = self.layout(ty).size as usize;
                if size > 8 {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "scalar initializer requires scalar type",
                    ));
                }
                out.bytes[offset as usize..offset as usize + size]
                    .copy_from_slice(&bits.to_le_bytes()[..size]);
            }
            Expr::String(bytes) => {
                let TypeKind::Slice {
                    element,
                    const_: true,
                } = *self.kind(ty)
                else {
                    return Err(error("E_TYPE", span, "string requires []const u8"));
                };
                self.same(element, self.builtin("u8"), span)?;
                let symbol = string_data(self.data, bytes);
                out.relocations.push(crate::machine::DataRelocation {
                    offset,
                    target: symbol,
                    addend: 0,
                });
                out.bytes[offset as usize + 8..offset as usize + 16]
                    .copy_from_slice(&(bytes.len() as u64).to_le_bytes());
            }
            Expr::Array(values) => {
                let (_, element, length) = self.array_expected(Some(ty), span)?;
                if values.len() as u64 != length {
                    return Err(error("E_TYPE", span, "static array length mismatch"));
                }
                let stride = self.layout(element).size;
                for (i, v) in values.iter().enumerate() {
                    self.static_init(v, element, offset + i as u64 * stride, out)?;
                }
            }
            Expr::Repeat { value, length } => {
                let (_, element, count) = self.array_expected(Some(ty), span)?;
                if self.program.repeat_lengths[&length.span] != count {
                    return Err(error("E_TYPE", span, "static repeat length mismatch"));
                }
                let stride = self.layout(element).size;
                // Validate the repeated initializer even for zero-sized/zero-count arrays.
                let mut item = crate::machine::DataObject {
                    symbol: String::new(),
                    mutable: false,
                    align: 1,
                    bytes: vec![0; stride.max(1) as usize],
                    zero_fill: 0,
                    relocations: vec![],
                };
                self.static_init(value, element, 0, &mut item)?;
                if stride == 0 {
                    return Ok(());
                }
                for i in 0..count {
                    let at = offset + i * stride;
                    out.bytes[at as usize..(at + stride) as usize]
                        .copy_from_slice(&item.bytes[..stride as usize]);
                    for r in &item.relocations {
                        out.relocations.push(crate::machine::DataRelocation {
                            offset: at + r.offset,
                            target: r.target.clone(),
                            addend: r.addend,
                        });
                    }
                }
            }
            Expr::Struct { name, fields } => {
                let actual = *self
                    .types
                    .names
                    .get(name)
                    .ok_or_else(|| error("E_NAME", span, "unknown structure"))?;
                self.same(actual, ty, span)?;
                let TypeKind::Struct {
                    fields: declared, ..
                } = self.kind(ty).clone()
                else {
                    return Err(error("E_TYPE", span, "expected structure"));
                };
                let offsets = self.layout(ty).field_offsets.clone();
                let mut seen = BTreeSet::new();
                for (name, v) in fields {
                    let i = declared
                        .iter()
                        .position(|f| f.name == *name)
                        .ok_or_else(|| error("E_NAME", v.span, "unknown static field"))?;
                    if !seen.insert(i) {
                        return Err(error("E_TYPE", v.span, "duplicate static field"));
                    }
                    self.static_init(v, declared[i].ty, offset + offsets[i], out)?;
                }
                if seen.len() != declared.len() {
                    return Err(error("E_TYPE", span, "missing static field"));
                }
            }
            Expr::Call { name, arguments }
                if [
                    "core::addr_of",
                    "core::addr_of_mut",
                    "addr_of",
                    "addr_of_mut",
                ]
                .contains(&name.as_str()) =>
            {
                if arguments.len() != 1 {
                    return Err(error("E_TYPE", span, "address requires one argument"));
                }
                let Expr::Name(n) = &arguments[0].kind else {
                    return Err(error(
                        "E_CONST",
                        span,
                        "static address must name a root static",
                    ));
                };
                let (i, g) = self
                    .program
                    .globals
                    .iter()
                    .enumerate()
                    .find(|(_, g)| g.name == *n)
                    .ok_or_else(|| error("E_NAME", span, "address must name a static"))?;
                let mutable = name.ends_with("_mut");
                if mutable && !g.mutable {
                    return Err(error("E_TYPE", span, "mutable address of immutable static"));
                }
                let TypeKind::Pointer { target, const_ } = *self.kind(ty) else {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "address requires pointer initializer type",
                    ));
                };
                if target != g.ty || const_ == mutable {
                    return Err(error("E_TYPE", span, "static pointer type mismatch"));
                }
                out.relocations.push(crate::machine::DataRelocation {
                    offset,
                    target: format!("__core_s_g{i}"),
                    addend: 0,
                });
            }
            _ => {
                return Err(error(
                    "I_UNSUPPORTED",
                    span,
                    "B2 static initializer is outside the implemented subset",
                ));
            }
        }
        Ok(())
    }
}
