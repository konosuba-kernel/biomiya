//! Stack-slot RV64IMA emission for the frozen Core S pilot CFG.
//!
//! Mechanical stack/copy/LP64/check sequences are adapted from the archived
//! src/target/rv64.rs and src/abi.rs (8feff82). No old compiler service is linked.
//! Source checking, snapshots, cleanup, and layout decisions belong to P/C.

use crate::{
    ast::{BinaryOp, UnaryOp},
    diagnostic::{Diagnostic, Result},
    machine::{
        Condition, IOp, Inst, LoadOp, Program, RA, ROp, Register, SCRATCH, SP, StoreOp, ZERO,
    },
    model::*,
    source::Span,
};

fn error(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("I_INTERNAL", span, message)
}

fn aligned(value: u64, align: u64, span: Span) -> Result<u64> {
    value
        .checked_add(align - 1)
        .map(|n| n & !(align - 1))
        .ok_or_else(|| error(span, "machine frame size overflow"))
}

fn scalar(kind: &TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::Unit | TypeKind::Bool | TypeKind::Int(_) | TypeKind::Pointer { .. }
    )
}

fn width(kind: &TypeKind) -> Option<u32> {
    match kind {
        TypeKind::Int(Integer::U8) => Some(8),
        TypeKind::Int(Integer::U32) => Some(32),
        TypeKind::Int(Integer::Usize) => Some(64),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum Return {
    Ignore,
    Scalar,
    Aggregate,
}

struct Plan {
    // Machine positions count the hidden result and omit zero-size values.
    arguments: Vec<Option<usize>>,
    result: Return,
    stack_size: u64,
}

struct Frame {
    offsets: Vec<u64>,
    save: u64,
    result_pointer: u64,
    size: u64,
    align: u64,
    plan: Plan,
}

/// Lower the checked CFG once, into operations consumed by both output paths.
pub fn machine(compilation: &Compilation) -> Result<Program> {
    let mut emitter = Emitter {
        compilation,
        program: Program {
            instructions: Vec::new(),
            data: compilation.data.clone(),
        },
        serial: 0,
    };
    emitter.validate()?;
    for (id, function) in compilation.functions.iter().enumerate() {
        if !compilation.externs.contains_key(&FunctionId(id as u32)) {
            emitter.function(id, function)?;
        }
    }
    Ok(emitter.program)
}

/// Reference/debug output uses exactly the same lowering as direct encoding.
pub fn emit(compilation: &Compilation) -> Result<String> {
    assembly(&machine(compilation)?)
}

const T0: Register = 5;
const T1: Register = 6;
const T2: Register = 7;
const S0: Register = 8;
const A0: Register = 10;
const T3: Register = 28;
const T4: Register = 29;
const T5: Register = 30;

struct Emitter<'a> {
    compilation: &'a Compilation,
    program: Program,
    serial: u64,
}

impl Emitter<'_> {
    fn info(&self, ty: TypeId, span: Span) -> Result<&TypeInfo> {
        self.compilation
            .types
            .entries
            .get(ty.0 as usize)
            .ok_or_else(|| error(span, "invalid IR type id"))
    }

    fn slot_type(&self, function: &Function, id: SlotId, span: Span) -> Result<TypeId> {
        function
            .slots
            .get(id.0 as usize)
            .map(|slot| slot.ty)
            .ok_or_else(|| error(span, "invalid IR slot id"))
    }

    fn operand_type(&self, function: &Function, value: &Operand, span: Span) -> Result<TypeId> {
        match value {
            Operand::Slot(id) => self.slot_type(function, *id, span),
            Operand::Immediate { bits, ty } => {
                let kind = &self.info(*ty, span)?.kind;
                let valid = match kind {
                    TypeKind::Unit | TypeKind::Pointer { .. } => *bits == 0,
                    TypeKind::Bool => *bits <= 1,
                    TypeKind::Int(_) => width(kind).is_some_and(|w| w == 64 || *bits < 1_u64 << w),
                    _ => false,
                };
                if !valid {
                    return Err(error(span, "invalid typed immediate"));
                }
                Ok(*ty)
            }
        }
    }

    fn same(&self, actual: TypeId, expected: TypeId, span: Span) -> Result<()> {
        if actual != expected {
            return Err(error(span, "IR type mismatch"));
        }
        Ok(())
    }

    fn place_type(&self, function: &Function, place: &Place, span: Span) -> Result<TypeId> {
        let size = self.info(place.ty, span)?.layout.size;
        match &place.base {
            PlaceBase::Slot(slot) => {
                let container = self.info(self.slot_type(function, *slot, span)?, span)?;
                if place
                    .byte_offset
                    .checked_add(size)
                    .is_none_or(|end| end > container.layout.size)
                {
                    return Err(error(span, "slot projection is outside its storage"));
                }
            }
            PlaceBase::Pointer(value) => {
                let ty = self.operand_type(function, value, span)?;
                if !matches!(self.info(ty, span)?.kind, TypeKind::Pointer { .. }) {
                    return Err(error(span, "pointer place has a nonpointer base"));
                }
                place
                    .byte_offset
                    .checked_add(size)
                    .ok_or_else(|| error(span, "place range overflow"))?;
            }
        }
        Ok(place.ty)
    }

    fn validate(&self) -> Result<()> {
        for info in &self.compilation.types.entries {
            let span = Span::default();
            if !info.layout.align.is_power_of_two() || info.layout.align > 4096 {
                return Err(error(span, "invalid completed alignment"));
            }
            let expected = match &info.kind {
                TypeKind::Slice { .. } => Some((16, 8)),
                TypeKind::Unit => Some((0, 1)),
                TypeKind::Bool | TypeKind::Int(Integer::U8) => Some((1, 1)),
                TypeKind::Int(Integer::U32) => Some((4, 4)),
                TypeKind::Int(Integer::Usize) | TypeKind::Pointer { .. } => Some((8, 8)),
                _ => None,
            };
            if expected.is_some_and(|pair| pair != (info.layout.size, info.layout.align)) {
                return Err(error(span, "invalid completed scalar layout"));
            }
            match &info.kind {
                TypeKind::Pointer { target, .. } => {
                    self.info(*target, span)?;
                }
                TypeKind::Array { element, .. } | TypeKind::Slice { element, .. } => {
                    self.info(*element, span)?;
                }
                TypeKind::Struct { fields, .. } => {
                    if fields.len() != info.layout.field_offsets.len() {
                        return Err(error(span, "missing completed field offsets"));
                    }
                    for field in fields {
                        self.info(field.ty, span)?;
                    }
                }
                TypeKind::Result { ok, error: err } => {
                    self.info(*ok, span)?;
                    self.info(*err, span)?;
                    if info.layout.payload_offset.is_none() {
                        return Err(error(span, "missing completed Result payload offset"));
                    }
                }
                _ => {}
            }
        }
        for (id, symbol) in &self.compilation.externs {
            if id.0 as usize >= self.compilation.functions.len() || !valid_symbol(symbol) {
                return Err(error(Span::default(), "invalid external function mapping"));
            }
        }
        for (id, function) in self.compilation.functions.iter().enumerate() {
            let span = function.span;
            if function.symbol != format!("__core_s_f{id}") {
                return Err(error(span, "invalid pilot function symbol"));
            }
            self.info(function.signature.result, span)?;
            if self
                .compilation
                .externs
                .contains_key(&FunctionId(id as u32))
            {
                if !function.signature.unsafe_
                    || !function.blocks.is_empty()
                    || !function.slots.is_empty()
                    || !function.parameter_slots.is_empty()
                {
                    return Err(error(
                        span,
                        "external function must have unsafe signature and no body",
                    ));
                }
                for ty in function
                    .signature
                    .parameters
                    .iter()
                    .chain(std::iter::once(&function.signature.result))
                {
                    if !scalar(&self.info(*ty, span)?.kind) {
                        return Err(error(span, "C extern requires scalar types"));
                    }
                }
                if function.signature.parameters.iter().any(|ty| {
                    matches!(
                        self.compilation.types.entries[ty.0 as usize].kind,
                        TypeKind::Unit
                    )
                }) {
                    return Err(error(span, "unit is not a C parameter"));
                }
                continue;
            }
            for slot in &function.slots {
                self.info(slot.ty, slot.span)?;
            }
            if function.parameter_slots.len() != function.signature.parameters.len() {
                return Err(error(span, "parameter slots do not match signature"));
            }
            for (index, (&slot, &ty)) in function
                .parameter_slots
                .iter()
                .zip(&function.signature.parameters)
                .enumerate()
            {
                self.same(self.slot_type(function, slot, span)?, ty, span)?;
                if function.parameter_slots[..index].contains(&slot) {
                    return Err(error(span, "parameters must have independent slots"));
                }
            }
            self.block(function, function.entry, span)?;
            for block in &function.blocks {
                for instruction in &block.instructions {
                    self.validate_instruction(function, instruction)?;
                }
                match &block.terminator {
                    Terminator::Jump(target) => self.block(function, *target, block.span)?,
                    Terminator::Branch { condition, yes, no } => {
                        let ty = self.operand_type(function, condition, block.span)?;
                        if !matches!(self.info(ty, block.span)?.kind, TypeKind::Bool) {
                            return Err(error(block.span, "branch condition must be bool"));
                        }
                        self.block(function, *yes, block.span)?;
                        self.block(function, *no, block.span)?;
                    }
                    Terminator::Return(value) => self.same(
                        self.operand_type(function, value, block.span)?,
                        function.signature.result,
                        block.span,
                    )?,
                    Terminator::Panic | Terminator::Unreachable => {}
                }
            }
        }
        Ok(())
    }

    fn block(&self, function: &Function, id: BlockId, span: Span) -> Result<()> {
        if id.0 as usize >= function.blocks.len() {
            return Err(error(span, "invalid IR block id"));
        }
        Ok(())
    }

    fn validate_instruction(&self, function: &Function, instruction: &Instruction) -> Result<()> {
        let span = instruction.span;
        match &instruction.kind {
            InstructionKind::Set { dst, value } => {
                let ty = self.slot_type(function, *dst, span)?;
                let kind = &self.info(ty, span)?.kind;
                if !scalar(kind) {
                    return Err(error(span, "aggregate Set requires typed Copy"));
                }
                match value {
                    Rvalue::SymbolAddress { symbol } => {
                        if !valid_symbol(symbol) || !matches!(kind, TypeKind::Pointer { .. }) {
                            return Err(error(
                                span,
                                "symbol address requires pointer and valid symbol",
                            ));
                        }
                    }
                    Rvalue::Bitcast { value } => {
                        let from = self.operand_type(function, value, span)?;
                        let from = &self.info(from, span)?.kind;
                        let allowed = match (from, kind) {
                            (TypeKind::Pointer { .. }, TypeKind::Int(Integer::Usize))
                            | (TypeKind::Int(Integer::Usize), TypeKind::Pointer { .. }) => true,
                            (
                                TypeKind::Pointer {
                                    target: a,
                                    const_: ac,
                                    ..
                                },
                                TypeKind::Pointer {
                                    target: b,
                                    const_: bc,
                                    ..
                                },
                            ) => a == b && (!ac || *bc),
                            _ => false,
                        };
                        if !allowed {
                            return Err(error(span, "invalid pointer bitcast"));
                        }
                    }
                    Rvalue::Use(value) => {
                        self.same(self.operand_type(function, value, span)?, ty, span)?
                    }
                    Rvalue::Load(place) => {
                        self.same(self.place_type(function, place, span)?, ty, span)?
                    }
                    Rvalue::Address(place) => {
                        let target = self.place_type(function, place, span)?;
                        if !matches!(kind, TypeKind::Pointer { target: t, .. } if *t == target) {
                            return Err(error(span, "address result must point to place type"));
                        }
                    }
                    Rvalue::PointerOffset { base, bytes } => {
                        let base_ty = self.operand_type(function, base, span)?;
                        let bytes_ty = self.operand_type(function, bytes, span)?;
                        if !matches!(kind, TypeKind::Pointer { .. })
                            || !matches!(self.info(base_ty, span)?.kind, TypeKind::Pointer { .. })
                            || !matches!(
                                self.info(bytes_ty, span)?.kind,
                                TypeKind::Int(Integer::Usize)
                            )
                        {
                            return Err(error(span, "invalid pointer offset types"));
                        }
                    }
                    Rvalue::Cast { value } => {
                        let from = self.operand_type(function, value, span)?;
                        if width(kind).is_none() || width(&self.info(from, span)?.kind).is_none() {
                            return Err(error(span, "Cast must be integer to integer"));
                        }
                    }
                    Rvalue::Unary { op, value } => {
                        self.same(self.operand_type(function, value, span)?, ty, span)?;
                        if !match op {
                            UnaryOp::Not => matches!(kind, TypeKind::Bool),
                            UnaryOp::BitNot => width(kind).is_some(),
                            UnaryOp::Neg | UnaryOp::Deref => false,
                        } {
                            return Err(error(span, "invalid pilot unary opcode"));
                        }
                    }
                    Rvalue::Binary { op, left, right } => {
                        let input = self.operand_type(function, left, span)?;
                        self.same(self.operand_type(function, right, span)?, input, span)?;
                        let input_kind = &self.info(input, span)?.kind;
                        let comparison = matches!(
                            op,
                            BinaryOp::Eq
                                | BinaryOp::Ne
                                | BinaryOp::Lt
                                | BinaryOp::Le
                                | BinaryOp::Gt
                                | BinaryOp::Ge
                        );
                        if comparison {
                            if !matches!(kind, TypeKind::Bool)
                                || (matches!(op, BinaryOp::Eq | BinaryOp::Ne)
                                    && !scalar(input_kind))
                                || (!matches!(op, BinaryOp::Eq | BinaryOp::Ne)
                                    && width(input_kind).is_none())
                            {
                                return Err(error(span, "invalid comparison types"));
                            }
                        } else {
                            self.same(input, ty, span)?;
                            if width(kind).is_none() || matches!(op, BinaryOp::And | BinaryOp::Or) {
                                return Err(error(span, "invalid integer opcode"));
                            }
                        }
                    }
                }
            }
            InstructionKind::Store { destination, value } => {
                let ty = self.place_type(function, destination, span)?;
                if !scalar(&self.info(ty, span)?.kind) {
                    return Err(error(span, "aggregate Store requires typed Copy"));
                }
                self.same(self.operand_type(function, value, span)?, ty, span)?;
            }
            InstructionKind::Copy {
                destination,
                source,
            } => {
                self.same(
                    self.place_type(function, source, span)?,
                    self.place_type(function, destination, span)?,
                    span,
                )?;
            }
            InstructionKind::BoundsDynamic { index, length } => {
                for operand in [index, length] {
                    let ty = self.operand_type(function, operand, span)?;
                    if !matches!(self.info(ty, span)?.kind, TypeKind::Int(Integer::Usize)) {
                        return Err(error(span, "dynamic bounds operands must be usize"));
                    }
                }
            }
            InstructionKind::Bounds { index, .. } => {
                let ty = self.operand_type(function, index, span)?;
                if !matches!(self.info(ty, span)?.kind, TypeKind::Int(Integer::Usize)) {
                    return Err(error(span, "bounds index must be usize"));
                }
            }
            InstructionKind::Call {
                function: target,
                arguments,
                result,
            } => {
                let callee = self
                    .compilation
                    .functions
                    .get(target.0 as usize)
                    .ok_or_else(|| error(span, "invalid IR function id"))?;
                if arguments.len() != callee.signature.parameters.len() {
                    return Err(error(span, "IR call arity mismatch"));
                }
                for (value, ty) in arguments.iter().zip(&callee.signature.parameters) {
                    self.same(self.operand_type(function, value, span)?, *ty, span)?;
                }
                if let Some(slot) = result {
                    self.same(
                        self.slot_type(function, *slot, span)?,
                        callee.signature.result,
                        span,
                    )?;
                } else if self.info(callee.signature.result, span)?.layout.size != 0 {
                    return Err(error(span, "nonzero call result needs a slot"));
                }
            }
            InstructionKind::StorageLive(slot) | InstructionKind::StorageDead(slot) => {
                self.slot_type(function, *slot, span)?;
            }
        }
        Ok(())
    }

    fn plan(&self, function: &Function) -> Result<Plan> {
        let info = self.info(function.signature.result, function.span)?;
        let result = if info.layout.size == 0 {
            Return::Ignore
        } else if scalar(&info.kind) {
            Return::Scalar
        } else {
            Return::Aggregate
        };
        let mut position = usize::from(matches!(result, Return::Aggregate));
        let mut arguments = vec![];
        for ty in &function.signature.parameters {
            if self.info(*ty, function.span)?.layout.size == 0 {
                arguments.push(None);
            } else {
                arguments.push(Some(position));
                position += 1;
            }
        }
        let bytes = u64::try_from(position.saturating_sub(8))
            .ok()
            .and_then(|n| n.checked_mul(8))
            .ok_or_else(|| error(function.span, "argument size overflow"))?;
        Ok(Plan {
            arguments,
            result,
            stack_size: aligned(bytes, 16, function.span)?,
        })
    }

    fn frame(&self, function: &Function) -> Result<Frame> {
        let plan = self.plan(function)?;
        let mut outgoing = 0;
        for block in &function.blocks {
            for instruction in &block.instructions {
                if let InstructionKind::Call {
                    function: target, ..
                } = instruction.kind
                {
                    outgoing = outgoing.max(
                        self.plan(&self.compilation.functions[target.0 as usize])?
                            .stack_size,
                    );
                }
            }
        }
        let save = outgoing;
        let result_pointer = outgoing + 16;
        let mut size = outgoing
            .checked_add(24)
            .ok_or_else(|| error(function.span, "frame size overflow"))?;
        let mut align = 16;
        let mut offsets = vec![];
        for slot in &function.slots {
            let layout = &self.info(slot.ty, slot.span)?.layout;
            align = align.max(layout.align);
            size = aligned(size, layout.align, slot.span)?;
            offsets.push(size);
            size = size
                .checked_add(layout.size.max(1))
                .ok_or_else(|| error(slot.span, "frame size overflow"))?;
        }
        Ok(Frame {
            offsets,
            save,
            result_pointer,
            size: aligned(size, 16, function.span)?,
            align,
            plan,
        })
    }

    fn push(&mut self, instruction: Inst) {
        self.program.instructions.push(instruction);
    }
    fn r(&mut self, op: ROp, rd: Register, rs1: Register, rs2: Register) {
        self.push(Inst::R { op, rd, rs1, rs2 });
    }
    fn i(&mut self, op: IOp, rd: Register, rs1: Register, imm: i64) {
        self.push(Inst::I { op, rd, rs1, imm });
    }
    fn li(&mut self, rd: Register, bits: u64) {
        self.push(Inst::Li { rd, bits });
    }
    fn mov(&mut self, rd: Register, rs: Register) {
        self.i(IOp::Addi, rd, rs, 0);
    }
    fn label(&mut self, name: impl Into<String>) {
        self.push(Inst::Label(name.into()));
    }
    fn branch(&mut self, condition: Condition, rs1: Register, rs2: Register, target: &str) {
        self.push(Inst::Branch {
            condition,
            rs1,
            rs2,
            target: target.into(),
        });
    }
    fn fresh(&mut self) -> String {
        self.serial += 1;
        format!(".Lcore_s_{}", self.serial)
    }

    fn address(&mut self, register: Register, base: Register, offset: u64) {
        if offset <= 2047 {
            self.i(IOp::Addi, register, base, offset as i64);
        } else {
            let temporary = if register == base { T4 } else { register };
            self.li(temporary, offset);
            self.r(ROp::Add, register, base, temporary);
        }
    }

    fn memory_base(&mut self, base: Register, offset: u64) -> (Register, i64) {
        if offset <= 2047 {
            (base, offset as i64)
        } else {
            self.address(SCRATCH, base, offset);
            (SCRATCH, 0)
        }
    }

    fn normalize(&mut self, register: Register, ty: TypeId) {
        if let Some(bits) = width(&self.compilation.types.entries[ty.0 as usize].kind) {
            if bits < 64 {
                self.i(IOp::Slli, register, register, (64 - bits) as i64);
                self.i(IOp::Srli, register, register, (64 - bits) as i64);
            }
        }
    }

    fn abi_extension(&mut self, register: Register, ty: TypeId) {
        if matches!(
            self.compilation.types.entries[ty.0 as usize].kind,
            TypeKind::Int(Integer::U8 | Integer::U32) | TypeKind::Bool
        ) {
            self.i(IOp::Addiw, register, register, 0);
        }
    }

    fn load(&mut self, register: Register, base: Register, offset: u64, size: u64) {
        let op = match size {
            1 => LoadOp::ByteUnsigned,
            4 => LoadOp::WordUnsigned,
            8 => LoadOp::Double,
            0 => {
                self.li(register, 0);
                return;
            }
            _ => unreachable!("validated scalar size"),
        };
        let (base, offset) = self.memory_base(base, offset);
        self.push(Inst::Load {
            op,
            rd: register,
            base,
            offset,
        });
    }

    fn store(&mut self, register: Register, base: Register, offset: u64, size: u64) {
        let op = match size {
            1 => StoreOp::Byte,
            4 => StoreOp::Word,
            8 => StoreOp::Double,
            0 => return,
            _ => unreachable!("validated scalar size"),
        };
        let (base, offset) = self.memory_base(base, offset);
        self.push(Inst::Store {
            op,
            rs: register,
            base,
            offset,
        });
    }

    fn operand(&mut self, function: &Function, frame: &Frame, value: &Operand, register: Register) {
        match value {
            Operand::Immediate { bits, .. } => self.li(register, *bits),
            Operand::Slot(slot) => {
                let layout = &self.compilation.types.entries
                    [function.slots[slot.0 as usize].ty.0 as usize]
                    .layout;
                self.load(register, SP, frame.offsets[slot.0 as usize], layout.size);
            }
        }
    }

    fn place(&mut self, function: &Function, frame: &Frame, place: &Place, register: Register) {
        match &place.base {
            PlaceBase::Slot(slot) => self.address(register, SP, frame.offsets[slot.0 as usize]),
            PlaceBase::Pointer(value) => self.operand(function, frame, value, register),
        }
        if place.byte_offset != 0 {
            self.address(register, register, place.byte_offset);
        }
    }

    // t0=destination, t1=source. Copy raw bytes, preserving padding and overlap.
    fn copy(&mut self, size: u64) {
        if size == 0 {
            return;
        }
        let backward = self.fresh();
        let forward = self.fresh();
        let reverse = self.fresh();
        let done = self.fresh();
        self.branch(Condition::Eq, T0, T1, &done);
        self.li(T2, size);
        self.branch(Condition::LtUnsigned, T1, T0, &backward);
        self.label(forward.clone());
        self.load(T3, T1, 0, 1);
        self.store(T3, T0, 0, 1);
        self.i(IOp::Addi, T0, T0, 1);
        self.i(IOp::Addi, T1, T1, 1);
        self.i(IOp::Addi, T2, T2, -1);
        self.branch(Condition::Ne, T2, ZERO, &forward);
        self.jump(&done);
        self.label(backward);
        self.r(ROp::Add, T0, T0, T2);
        self.r(ROp::Add, T1, T1, T2);
        self.label(reverse.clone());
        self.i(IOp::Addi, T0, T0, -1);
        self.i(IOp::Addi, T1, T1, -1);
        self.load(T3, T1, 0, 1);
        self.store(T3, T0, 0, 1);
        self.i(IOp::Addi, T2, T2, -1);
        self.branch(Condition::Ne, T2, ZERO, &reverse);
        self.label(done);
    }

    fn incoming(&mut self, position: usize, register: Register) {
        if position < 8 {
            self.mov(register, A0 + position as u8);
        } else {
            self.load(register, S0, (position - 8) as u64 * 8, 8);
        }
    }
    fn outgoing(&mut self, position: usize, register: Register) {
        if position < 8 {
            self.mov(A0 + position as u8, register);
        } else {
            self.store(register, SP, (position - 8) as u64 * 8, 8);
        }
    }
    fn jump(&mut self, label: &str) {
        self.push(Inst::Jump {
            rd: ZERO,
            target: label.into(),
        });
    }
    fn call(&mut self, label: &str) {
        self.push(Inst::Jump {
            rd: RA,
            target: label.into(),
        });
    }
    fn panic(&mut self) {
        self.call("__core_s_panic");
        self.push(Inst::Unimp);
    }
    fn guard(&mut self, condition: Condition, rs1: Register, rs2: Register) {
        let good = self.fresh();
        self.branch(condition, rs1, rs2, &good);
        self.panic();
        self.label(good);
    }

    fn function(&mut self, id: usize, function: &Function) -> Result<()> {
        let frame = self.frame(function)?;
        self.label(function.symbol.clone());
        self.mov(T5, SP);
        self.li(SCRATCH, frame.size);
        self.r(ROp::Sub, SP, SP, SCRATCH);
        if frame.align > 16 {
            self.li(SCRATCH, 0u64.wrapping_sub(frame.align));
            self.r(ROp::And, SP, SP, SCRATCH);
        }
        self.store(RA, SP, frame.save, 8);
        self.store(S0, SP, frame.save + 8, 8);
        self.mov(S0, T5);
        if matches!(frame.plan.result, Return::Aggregate) {
            self.store(A0, SP, frame.result_pointer, 8);
        }
        for (index, slot) in function.parameter_slots.iter().enumerate() {
            let Some(position) = frame.plan.arguments[index] else {
                continue;
            };
            let ty = function.slots[slot.0 as usize].ty;
            let info = &self.compilation.types.entries[ty.0 as usize];
            if scalar(&info.kind) {
                self.incoming(position, T0);
                self.store(T0, SP, frame.offsets[slot.0 as usize], info.layout.size);
            } else {
                self.incoming(position, T1);
                self.address(T0, SP, frame.offsets[slot.0 as usize]);
                self.copy(info.layout.size);
            }
        }
        self.jump(&format!(".Lf{id}_b{}", function.entry.0));
        for (block_id, block) in function.blocks.iter().enumerate() {
            self.label(format!(".Lf{id}_b{block_id}"));
            for instruction in &block.instructions {
                self.instruction(function, &frame, instruction)?;
            }
            match &block.terminator {
                Terminator::Jump(target) => self.jump(&format!(".Lf{id}_b{}", target.0)),
                Terminator::Branch { condition, yes, no } => {
                    self.operand(function, &frame, condition, T0);
                    let next = self.fresh();
                    self.branch(Condition::Eq, T0, ZERO, &next);
                    self.jump(&format!(".Lf{id}_b{}", yes.0));
                    self.label(next);
                    self.jump(&format!(".Lf{id}_b{}", no.0));
                }
                Terminator::Return(value) => {
                    match frame.plan.result {
                        Return::Ignore => {}
                        Return::Scalar => {
                            self.operand(function, &frame, value, A0);
                            self.abi_extension(A0, function.signature.result);
                        }
                        Return::Aggregate => {
                            let Operand::Slot(slot) = value else {
                                unreachable!("validated aggregate operand");
                            };
                            self.load(T0, SP, frame.result_pointer, 8);
                            self.address(T1, SP, frame.offsets[slot.0 as usize]);
                            self.copy(
                                self.info(function.signature.result, function.span)?
                                    .layout
                                    .size,
                            );
                        }
                    }
                    self.mov(T5, S0);
                    self.load(RA, SP, frame.save, 8);
                    self.load(S0, SP, frame.save + 8, 8);
                    self.mov(SP, T5);
                    self.push(Inst::Jalr {
                        rd: ZERO,
                        base: RA,
                        offset: 0,
                    });
                }
                Terminator::Panic | Terminator::Unreachable => self.panic(),
            }
        }
        Ok(())
    }

    fn instruction(
        &mut self,
        function: &Function,
        frame: &Frame,
        instruction: &Instruction,
    ) -> Result<()> {
        match &instruction.kind {
            InstructionKind::Set { dst, value } => {
                let ty = function.slots[dst.0 as usize].ty;
                let size = self.info(ty, instruction.span)?.layout.size;
                match value {
                    Rvalue::Use(value) | Rvalue::Bitcast { value } => {
                        self.operand(function, frame, value, T0)
                    }
                    Rvalue::SymbolAddress { symbol } => {
                        self.program.instructions.push(Inst::Address {
                            rd: T0,
                            target: symbol.clone(),
                        })
                    }
                    Rvalue::Load(place) => {
                        self.place(function, frame, place, T1);
                        self.load(T0, T1, 0, size);
                    }
                    Rvalue::Address(place) => self.place(function, frame, place, T0),
                    Rvalue::PointerOffset { base, bytes } => {
                        self.operand(function, frame, base, T1);
                        self.operand(function, frame, bytes, T2);
                        self.r(ROp::Add, T0, T1, T2);
                        self.guard(Condition::GeUnsigned, T0, T1);
                    }
                    Rvalue::Cast { value } => {
                        self.operand(function, frame, value, T0);
                        self.normalize(T0, ty);
                    }
                    Rvalue::Unary { op, value } => {
                        self.operand(function, frame, value, T0);
                        match op {
                            UnaryOp::Not => self.i(IOp::Sltiu, T0, T0, 1),
                            UnaryOp::BitNot => self.i(IOp::Xori, T0, T0, -1),
                            _ => unreachable!("validated unary opcode"),
                        }
                        self.normalize(T0, ty);
                    }
                    Rvalue::Binary { op, left, right } => {
                        self.binary(function, frame, *op, left, right)
                    }
                }
                self.store(T0, SP, frame.offsets[dst.0 as usize], size);
            }
            InstructionKind::Store { destination, value } => {
                self.operand(function, frame, value, T0);
                self.place(function, frame, destination, T1);
                self.store(
                    T0,
                    T1,
                    0,
                    self.info(destination.ty, instruction.span)?.layout.size,
                );
            }
            InstructionKind::Copy {
                destination,
                source,
            } => {
                self.place(function, frame, destination, T0);
                self.place(function, frame, source, T1);
                self.copy(self.info(destination.ty, instruction.span)?.layout.size);
            }
            InstructionKind::BoundsDynamic { index, length } => {
                self.operand(function, frame, index, T0);
                self.operand(function, frame, length, T1);
                self.guard(Condition::LtUnsigned, T0, T1);
            }
            InstructionKind::Bounds { index, length } => {
                self.operand(function, frame, index, T0);
                self.li(T1, *length);
                self.guard(Condition::LtUnsigned, T0, T1);
            }
            InstructionKind::Call {
                function: target,
                arguments,
                result,
            } => {
                let callee = &self.compilation.functions[target.0 as usize];
                let plan = self.plan(callee)?;
                if matches!(plan.result, Return::Aggregate) {
                    let slot = result.expect("validated result slot");
                    self.address(A0, SP, frame.offsets[slot.0 as usize]);
                }
                for (index, value) in arguments.iter().enumerate() {
                    let Some(position) = plan.arguments[index] else {
                        continue;
                    };
                    let ty = callee.signature.parameters[index];
                    if scalar(&self.info(ty, instruction.span)?.kind) {
                        self.operand(function, frame, value, T0);
                        self.abi_extension(T0, ty);
                    } else {
                        let Operand::Slot(slot) = value else {
                            unreachable!("validated aggregate operand");
                        };
                        self.address(T0, SP, frame.offsets[slot.0 as usize]);
                    }
                    self.outgoing(position, T0);
                }
                let symbol = self
                    .compilation
                    .externs
                    .get(target)
                    .unwrap_or(&callee.symbol)
                    .clone();
                self.call(&symbol);
                if let (Return::Scalar, Some(slot)) = (plan.result, result) {
                    self.store(
                        A0,
                        SP,
                        frame.offsets[slot.0 as usize],
                        self.info(callee.signature.result, instruction.span)?
                            .layout
                            .size,
                    );
                }
            }
            InstructionKind::StorageLive(_) | InstructionKind::StorageDead(_) => {}
        }
        Ok(())
    }

    fn binary(
        &mut self,
        function: &Function,
        frame: &Frame,
        op: BinaryOp,
        left: &Operand,
        right: &Operand,
    ) {
        let ty = self
            .operand_type(function, left, function.span)
            .expect("validated operand");
        let bits = width(&self.compilation.types.entries[ty.0 as usize].kind).unwrap_or(64);
        self.operand(function, frame, left, T1);
        self.operand(function, frame, right, T2);
        let wrapping = matches!(
            op,
            BinaryOp::WrappingAdd | BinaryOp::WrappingSub | BinaryOp::WrappingMul
        );
        match op {
            BinaryOp::Add | BinaryOp::WrappingAdd => {
                self.r(ROp::Add, T0, T1, T2);
                if !wrapping {
                    self.guard(Condition::GeUnsigned, T0, T1);
                }
            }
            BinaryOp::Sub | BinaryOp::WrappingSub => {
                if !wrapping {
                    self.guard(Condition::GeUnsigned, T1, T2);
                }
                self.r(ROp::Sub, T0, T1, T2);
            }
            BinaryOp::Mul | BinaryOp::WrappingMul => {
                self.r(ROp::Mul, T0, T1, T2);
                if !wrapping {
                    self.r(ROp::Mulhu, T3, T1, T2);
                    self.guard(Condition::Eq, T3, ZERO);
                }
            }
            BinaryOp::Div | BinaryOp::Rem => {
                self.guard(Condition::Ne, T2, ZERO);
                self.r(
                    if op == BinaryOp::Div {
                        ROp::Divu
                    } else {
                        ROp::Remu
                    },
                    T0,
                    T1,
                    T2,
                );
            }
            BinaryOp::Shl | BinaryOp::Shr => {
                self.li(T3, bits as u64);
                self.guard(Condition::LtUnsigned, T2, T3);
                self.r(
                    if op == BinaryOp::Shl {
                        ROp::Sll
                    } else {
                        ROp::Srl
                    },
                    T0,
                    T1,
                    T2,
                );
            }
            BinaryOp::BitAnd => self.r(ROp::And, T0, T1, T2),
            BinaryOp::BitOr => self.r(ROp::Or, T0, T1, T2),
            BinaryOp::BitXor => self.r(ROp::Xor, T0, T1, T2),
            BinaryOp::Eq | BinaryOp::Ne => {
                self.r(ROp::Xor, T0, T1, T2);
                if op == BinaryOp::Eq {
                    self.i(IOp::Sltiu, T0, T0, 1);
                } else {
                    self.r(ROp::Sltu, T0, ZERO, T0);
                }
            }
            BinaryOp::Lt => self.r(ROp::Sltu, T0, T1, T2),
            BinaryOp::Gt => self.r(ROp::Sltu, T0, T2, T1),
            BinaryOp::Le => {
                self.r(ROp::Sltu, T0, T2, T1);
                self.i(IOp::Xori, T0, T0, 1);
            }
            BinaryOp::Ge => {
                self.r(ROp::Sltu, T0, T1, T2);
                self.i(IOp::Xori, T0, T0, 1);
            }
            BinaryOp::And | BinaryOp::Or => unreachable!("short circuit must be CFG"),
        }
        if !wrapping && matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul) && bits < 64 {
            self.mov(T3, T0);
            self.normalize(T3, ty);
            self.guard(Condition::Eq, T0, T3);
        }
        // Slots keep narrow integers unsigned; LP64 extends u32 only at calls.
        self.normalize(T0, ty);
    }
}

/// Render typed operations for a reference assembler; external targets are legal.
/// No source or assembly text is parsed here, and no subprocess is involved.
pub fn assembly(program: &Program) -> Result<String> {
    use std::{collections::BTreeSet, fmt::Write};
    fn register(value: Register) -> Result<()> {
        if value < 32 {
            Ok(())
        } else {
            Err(error(Span::default(), "invalid machine register"))
        }
    }
    fn immediate(value: i64) -> Result<()> {
        if (-2048..=2047).contains(&value) {
            Ok(())
        } else {
            Err(error(Span::default(), "invalid machine immediate"))
        }
    }
    fn name(value: &str) -> Result<()> {
        let mut bytes = value.bytes();
        let first = bytes
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_' || c == b'.');
        if first
            && value != "."
            && bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'.')
        {
            Ok(())
        } else {
            Err(error(Span::default(), "invalid machine symbol"))
        }
    }
    let mut labels = BTreeSet::new();
    let mut reserved = BTreeSet::new();
    for instruction in &program.instructions {
        let target = match instruction {
            Inst::Label(label) => {
                if !labels.insert(label.clone()) {
                    return Err(error(Span::default(), "duplicate machine symbol"));
                }
                Some(label)
            }
            Inst::Jump { target, .. }
            | Inst::Branch { target, .. }
            | Inst::Address { target, .. } => Some(target),
            _ => None,
        };
        if let Some(target) = target {
            name(target)?;
            reserved.insert(target.clone());
        }
    }
    crate::machine::validate_data(&program.data)?;
    for object in &program.data {
        if !labels.insert(object.symbol.clone()) {
            return Err(error(Span::default(), "duplicate machine symbol"));
        }
        reserved.insert(object.symbol.clone());
        for relocation in &object.relocations {
            name(&relocation.target)?;
            reserved.insert(relocation.target.clone());
        }
    }
    let mut serial = 0u64;
    let mut fresh = || loop {
        serial += 1;
        let label = format!(".Lcore_s_render_{serial}");
        if reserved.insert(label.clone()) {
            break label;
        }
    };
    let mut output = String::from(".option norvc\n.option norelax\n.text\n.balign 4\n");
    for instruction in &program.instructions {
        match instruction {
            Inst::Label(label) => {
                if !label.starts_with('.') {
                    writeln!(output, ".globl {label}").unwrap();
                }
                writeln!(output, "{label}:").unwrap();
            }
            Inst::R { op, rd, rs1, rs2 } => {
                register(*rd)?;
                register(*rs1)?;
                register(*rs2)?;
                let opcode = match op {
                    ROp::Add => "add",
                    ROp::Sub => "sub",
                    ROp::Mul => "mul",
                    ROp::Mulhu => "mulhu",
                    ROp::Divu => "divu",
                    ROp::Remu => "remu",
                    ROp::And => "and",
                    ROp::Or => "or",
                    ROp::Xor => "xor",
                    ROp::Sltu => "sltu",
                    ROp::Sll => "sll",
                    ROp::Srl => "srl",
                };
                writeln!(output, "{opcode} x{rd}, x{rs1}, x{rs2}").unwrap();
            }
            Inst::I { op, rd, rs1, imm } => {
                register(*rd)?;
                register(*rs1)?;
                if matches!(op, IOp::Slli | IOp::Srli) {
                    if !(0..64).contains(imm) {
                        return Err(error(Span::default(), "invalid machine shift"));
                    }
                } else {
                    immediate(*imm)?;
                }
                let opcode = match op {
                    IOp::Addi => "addi",
                    IOp::Addiw => "addiw",
                    IOp::Xori => "xori",
                    IOp::Sltiu => "sltiu",
                    IOp::Slli => "slli",
                    IOp::Srli => "srli",
                };
                writeln!(output, "{opcode} x{rd}, x{rs1}, {imm}").unwrap();
            }
            Inst::Load {
                op,
                rd,
                base,
                offset,
            } => {
                register(*rd)?;
                register(*base)?;
                immediate(*offset)?;
                let opcode = match op {
                    LoadOp::ByteUnsigned => "lbu",
                    LoadOp::WordUnsigned => "lwu",
                    LoadOp::Double => "ld",
                };
                writeln!(output, "{opcode} x{rd}, {offset}(x{base})").unwrap();
            }
            Inst::Store {
                op,
                rs,
                base,
                offset,
            } => {
                register(*rs)?;
                register(*base)?;
                immediate(*offset)?;
                let opcode = match op {
                    StoreOp::Byte => "sb",
                    StoreOp::Word => "sw",
                    StoreOp::Double => "sd",
                };
                writeln!(output, "{opcode} x{rs}, {offset}(x{base})").unwrap();
            }
            Inst::Li { rd, bits } => {
                register(*rd)?;
                writeln!(output, "li x{rd}, {}", *bits as i64).unwrap();
            }
            Inst::Branch {
                condition,
                rs1,
                rs2,
                target,
            } => {
                register(*rs1)?;
                register(*rs2)?;
                let inverse = match condition {
                    Condition::Eq => "bne",
                    Condition::Ne => "beq",
                    Condition::LtUnsigned => "bgeu",
                    Condition::GeUnsigned => "bltu",
                };
                let pair = fresh();
                let after = fresh();
                writeln!(output, "{inverse} x{rs1}, x{rs2}, {after}\n{pair}:\nauipc x{SCRATCH}, %pcrel_hi({target})\njalr x0, %pcrel_lo({pair})(x{SCRATCH})\n{after}:").unwrap();
            }
            Inst::Jump { rd, target } => {
                if !matches!(*rd, ZERO | RA) {
                    return Err(error(Span::default(), "jump destination must be x0 or x1"));
                }
                let pair = fresh();
                writeln!(output, "{pair}:\nauipc x{SCRATCH}, %pcrel_hi({target})\njalr x{rd}, %pcrel_lo({pair})(x{SCRATCH})").unwrap();
            }
            Inst::Address { rd, target } => {
                register(*rd)?;
                if *rd == ZERO {
                    return Err(error(
                        Span::default(),
                        "address destination must be nonzero",
                    ));
                }
                let pair = fresh();
                writeln!(output, "{pair}:\nauipc x{rd}, %pcrel_hi({target})\naddi x{rd}, x{rd}, %pcrel_lo({pair})").unwrap();
            }
            Inst::Jalr { rd, base, offset } => {
                register(*rd)?;
                register(*base)?;
                immediate(*offset)?;
                writeln!(output, "jalr x{rd}, {offset}(x{base})").unwrap();
            }
            Inst::Ecall => output.push_str("ecall\n"),
            Inst::Unimp => output.push_str(".4byte 0xc0001073\n"),
        }
    }
    for section in 0..3 {
        output.push_str([".section .rodata\n", ".data\n", ".bss\n"][section]);
        for object in &program.data {
            let class = if !object.mutable {
                0
            } else if object.bytes.is_empty() {
                2
            } else {
                1
            };
            if class != section {
                continue;
            }
            writeln!(
                output,
                ".balign {}\n.globl {}\n{}:",
                object.align, object.symbol, object.symbol
            )
            .unwrap();
            if section == 2 {
                writeln!(output, ".zero {}", object.zero_fill).unwrap();
                continue;
            }
            let relocations: std::collections::BTreeMap<_, _> = object
                .relocations
                .iter()
                .map(|r| (r.offset as usize, r))
                .collect();
            let mut offset = 0;
            while offset < object.bytes.len() {
                if let Some(relocation) = relocations.get(&offset) {
                    writeln!(
                        output,
                        ".quad {} {:+}",
                        relocation.target, relocation.addend
                    )
                    .unwrap();
                    offset += 8;
                } else {
                    writeln!(output, ".byte {}", object.bytes[offset]).unwrap();
                    offset += 1;
                }
            }
        }
    }
    Ok(output)
}

fn valid_symbol(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_' || c == b'.')
        && value != "."
        && bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'.')
}
