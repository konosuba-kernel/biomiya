//! Direct, stack-slot RV64IMA emitter. Machine plans retain semantic type identity.
use std::collections::BTreeSet;

use crate::{
    abi::{self, CallPlan, Extension, Location, Passing, ReturnLoc},
    diagnostic::{Diagnostic, Result},
    ir::*,
    layout::{Layout, LayoutEngine, align_up},
    source::Span,
    types::*,
};
#[derive(Clone, Debug)]
pub struct FrameInfo {
    pub symbol: String,
    pub local_bytes: u64,
    pub outgoing_bytes: u64,
    pub alignment: u64,
    pub realignment_slack: u64,
    pub frame_bytes: u64,
    pub transient_bytes: u64,
}
#[derive(Clone, Debug)]
pub struct Emission {
    pub assembly: String,
    pub required_symbols: Vec<String>,
    pub frames: Vec<FrameInfo>,
    pub linkage: crate::linkage::LinkageNames,
}
pub fn emit(types: &mut TypeContext, module: &Module) -> Result<Emission> {
    let mut emitter = Emitter {
        types,
        text: String::from(".option norelax\n.option norvc\n"),
        required: BTreeSet::new(),
        frames: vec![],
        serial: 0,
    };
    emitter.line(".section .rodata,\"a\",@progbits\n.Lbiomiya_failure_message:\n.ascii \"runtime check failed\"");
    for data in &module.data {
        emitter.data(data)?;
    }
    for function in &module.functions {
        emitter.function(function)?;
    }
    Ok(Emission {
        assembly: emitter.text,
        required_symbols: emitter.required.into_iter().collect(),
        frames: emitter.frames,
        linkage: collect_linkage(module),
    })
}
struct Emitter<'a> {
    types: &'a mut TypeContext,
    text: String,
    required: BTreeSet<String>,
    frames: Vec<FrameInfo>,
    serial: u64,
}
struct Frame {
    offsets: Vec<u64>,
    layouts: Vec<Layout>,
    save: u64,
    sret: u64,
    scratch: u64,
    plan: CallPlan,
    info: FrameInfo,
}
impl Emitter<'_> {
    fn line(&mut self, s: impl AsRef<str>) {
        self.text.push_str(s.as_ref());
        self.text.push('\n');
    }
    fn fresh(&mut self) -> String {
        self.serial += 1;
        format!(".Lbiomiya_{}", self.serial)
    }
    fn error(&self, span: Span, s: impl Into<String>) -> Diagnostic {
        Diagnostic::new("I_UNSUPPORTED", span, s)
    }
    fn layout(&mut self, t: TyId) -> Result<Layout> {
        LayoutEngine::new(self.types).layout(t)
    }
    fn data(&mut self, d: &Data) -> Result<()> {
        self.line(format!(
            ".section {},\"{}\",@progbits",
            d.section
                .as_deref()
                .map(|s| quote(&crate::linkage::section_alias(s)))
                .unwrap_or_else(|| quote(if d.writable { ".data" } else { ".rodata" })),
            if d.writable { "aw" } else { "a" }
        ));
        self.line(format!(".balign {}", d.align));
        if d.exported {
            self.line(format!(".globl {}", quote(&d.symbol)))
        }
        self.line(format!("{}:", quote(&d.symbol)));
        let mut offset = 0;
        let mut relocs = d.relocations.clone();
        relocs.sort_by_key(|r| r.offset);
        for r in relocs {
            if r.offset < offset || r.offset + 8 > d.bytes.len() as u64 {
                return Err(self.error(
                    crate::source::Span::new(crate::source::FileId(0), 0, 0),
                    "invalid data relocation",
                ));
            }
            for b in &d.bytes[offset as usize..r.offset as usize] {
                self.line(format!(".byte {b}"))
            }
            self.line(format!(".quad {}{:+}", quote(&r.symbol), r.addend));
            offset = r.offset + 8;
        }
        for b in &d.bytes[offset as usize..] {
            self.line(format!(".byte {b}"))
        }
        Ok(())
    }
    fn frame(&mut self, f: &Function) -> Result<Frame> {
        let plan = abi::classify(self.types, &f.signature)?;
        let mut outgoing = 0;
        let mut transient = 0;
        for b in &f.blocks {
            for i in &b.instructions {
                if let InstructionKind::Asm {
                    inputs, outputs, ..
                } = &i.kind
                {
                    transient = transient
                        .max(align_up((inputs.len() + 2 * outputs.len()) as u64 * 8, 16)?.max(16));
                }
                if let InstructionKind::Call { signature, .. } = &i.kind {
                    outgoing = outgoing.max(abi::classify(self.types, signature)?.stack_size);
                }
            }
        }
        let save = outgoing;
        let sret = save + 16;
        let scratch = save + 24;
        let mut size = save + 32;
        let mut alignment = 16;
        let mut offsets = vec![];
        let mut layouts = vec![];
        for s in &f.slots {
            let l = self.layout(s.ty)?;
            alignment = alignment.max(l.align);
            size = align_up(size, l.align)?;
            offsets.push(size);
            size = size
                .checked_add(l.size.max(1))
                .ok_or_else(|| self.error(s.span, "stack frame exceeds address space"))?;
            layouts.push(l);
        }
        let local = size - (save + 32);
        size = align_up(size, 16)?;
        let slack = alignment - 16;
        let frame_bytes = size
            .checked_add(slack)
            .and_then(|n| n.checked_add(transient))
            .ok_or_else(|| self.error(f.span, "stack frame exceeds address space"))?;
        Ok(Frame {
            offsets,
            layouts,
            save,
            sret,
            scratch,
            plan,
            info: FrameInfo {
                symbol: f.symbol.clone(),
                local_bytes: local,
                outgoing_bytes: outgoing,
                alignment,
                realignment_slack: slack,
                frame_bytes,
                transient_bytes: transient,
            },
        })
    }
    fn address(&mut self, dst: &str, base: &str, offset: u64) {
        if offset <= 2047 {
            self.line(format!("addi {dst}, {base}, {offset}"));
        } else {
            self.line(format!("li {dst}, {offset}"));
            self.line(format!("add {dst}, {base}, {dst}"));
        }
    }
    fn mem(&mut self, op: &str, value: &str, base: &str, offset: u64) {
        if offset <= 2047 {
            self.line(format!("{op} {value}, {offset}({base})"))
        } else {
            let tmp = if value == "t6" || base == "t6" {
                "t4"
            } else {
                "t6"
            };
            self.address(tmp, base, offset);
            self.line(format!("{op} {value}, 0({tmp})"));
        }
    }
    fn normalize(&mut self, r: &str, t: TyId) {
        if let TypeKind::Int(i) = self.types.kind(t).clone() {
            let bits = i.bits();
            if bits < 64 {
                self.line(format!("slli {r}, {r}, {}", 64 - bits));
                self.line(format!(
                    "{} {r}, {r}, {}",
                    if i.signed() { "srai" } else { "srli" },
                    64 - bits
                ));
            }
        }
    }
    fn extension(&mut self, r: &str, e: Extension) {
        match e {
            Extension::None => {}
            Extension::SignTo32Then64 { bits } => {
                self.line(format!("slli {r}, {r}, {}", 64 - bits));
                self.line(format!("srai {r}, {r}, {}", 64 - bits));
            }
            Extension::ZeroTo32ThenSign64 { bits } => {
                if bits < 32 {
                    self.line(format!("slli {r}, {r}, {}", 64 - bits));
                    self.line(format!("srli {r}, {r}, {}", 64 - bits));
                }
                self.line(format!("sext.w {r}, {r}"));
            }
        }
    }
    fn load_bytes(&mut self, r: &str, base: &str, offset: u64, size: u64, align: u64) {
        let op = match size {
            1 => Some("lbu"),
            2 if align >= 2 => Some("lhu"),
            4 if align >= 4 => Some("lwu"),
            8 if align >= 8 => Some("ld"),
            _ => None,
        };
        if let Some(op) = op {
            self.mem(op, r, base, offset)
        } else {
            self.line(format!("li {r}, 0"));
            let temp = if r == "t3" { "t2" } else { "t3" };
            for n in 0..size {
                self.mem("lbu", temp, base, offset + n);
                if n > 0 {
                    self.line(format!("slli {temp}, {temp}, {}", n * 8))
                }
                self.line(format!("or {r}, {r}, {temp}"));
            }
        }
    }
    fn store_bytes(&mut self, r: &str, base: &str, offset: u64, size: u64, align: u64) {
        let op = match size {
            1 => Some("sb"),
            2 if align >= 2 => Some("sh"),
            4 if align >= 4 => Some("sw"),
            8 if align >= 8 => Some("sd"),
            _ => None,
        };
        if let Some(op) = op {
            self.mem(op, r, base, offset)
        } else {
            for n in 0..size {
                if n == 0 {
                    self.mem("sb", r, base, offset)
                } else {
                    self.line(format!("srli t3, {r}, {}", n * 8));
                    self.mem("sb", "t3", base, offset + n);
                }
            }
        }
    }
    fn operand(&mut self, f: &Function, fr: &Frame, o: &Operand, r: &str) -> Result<()> {
        match o {
            Operand::Integer { bits, ty } => {
                self.line(format!("li {r}, {}", *bits as i64));
                self.normalize(r, *ty)
            }
            Operand::Symbol { name, addend, .. } => {
                self.line(format!("la {r}, {}", quote(name)));
                if *addend != 0 {
                    self.line(format!("li t4, {addend}"));
                    self.line(format!("add {r}, {r}, t4"));
                }
            }
            Operand::Slot(id) => {
                let l = &fr.layouts[id.0 as usize];
                if l.size > 8 {
                    return Err(self.error(f.span, "aggregate operand requested as scalar"));
                }
                self.load_bytes(r, "sp", fr.offsets[id.0 as usize], l.size, l.align);
                self.normalize(r, f.slots[id.0 as usize].ty)
            }
        }
        Ok(())
    }
    fn operand_address(&mut self, f: &Function, fr: &Frame, o: &Operand, r: &str) -> Result<()> {
        match o {
            Operand::Slot(id) => {
                self.address(r, "sp", fr.offsets[id.0 as usize]);
                Ok(())
            }
            _ => Err(self.error(f.span, "aggregate value must reside in an IR slot")),
        }
    }
    fn place(&mut self, f: &Function, fr: &Frame, p: &Place, r: &str) -> Result<()> {
        match &p.base {
            PlaceBase::Slot(id) => self.address(r, "sp", fr.offsets[id.0 as usize]),
            PlaceBase::Global(s) => self.line(format!("la {r}, {}", quote(s))),
            PlaceBase::Pointer(o) => self.operand(f, fr, o, r)?,
        }
        if p.offset != 0 {
            self.line(format!("li t4, {}", p.offset));
            self.line(format!("add {r}, {r}, t4"));
        }
        Ok(())
    }
    fn copy(&mut self, size: u64) {
        if size == 0 {
            return;
        }
        let backward = self.fresh();
        let forward = self.fresh();
        let done = self.fresh();
        self.line(format!("beq t0, t1, {done}"));
        self.line(format!("bltu t1, t0, {backward}"));
        self.line(format!("li t2, {size}"));
        self.line(format!("{forward}:"));
        self.line("lbu t3, 0(t1)");
        self.line("sb t3, 0(t0)");
        self.line("addi t0, t0, 1");
        self.line("addi t1, t1, 1");
        self.line("addi t2, t2, -1");
        self.line(format!("bnez t2, {forward}"));
        self.line(format!("j {done}"));
        self.line(format!("{backward}:"));
        self.line(format!("li t2, {size}"));
        self.line("add t0, t0, t2");
        self.line("add t1, t1, t2");
        let loop_ = self.fresh();
        self.line(format!("{loop_}:"));
        self.line("addi t0, t0, -1");
        self.line("addi t1, t1, -1");
        self.line("lbu t3, 0(t1)");
        self.line("sb t3, 0(t0)");
        self.line("addi t2, t2, -1");
        self.line(format!("bnez t2, {loop_}"));
        self.line(format!("{done}:"));
    }
    fn function(&mut self, f: &Function) -> Result<()> {
        let fr = self.frame(f)?;
        self.line(format!(
            ".section {},\"ax\",@progbits",
            f.section
                .as_deref()
                .map(|s| quote(&crate::linkage::section_alias(s)))
                .unwrap_or_else(|| quote(".text"))
        ));
        self.line(".balign 4");
        if f.exported {
            self.line(format!(".globl {}", quote(&f.symbol)))
        }
        self.line(format!(
            ".type {}, @function\n{}:",
            quote(&f.symbol),
            quote(&f.symbol)
        ));
        self.line("mv t5, sp");
        self.line(format!(
            "li t6, {}",
            fr.info.frame_bytes - fr.info.realignment_slack - fr.info.transient_bytes
        ));
        self.line("sub sp, sp, t6");
        if fr.info.alignment > 16 {
            self.line(format!("li t6, -{}", fr.info.alignment));
            self.line("and sp, sp, t6");
        }
        self.mem("sd", "ra", "sp", fr.save);
        self.mem("sd", "s0", "sp", fr.save + 8);
        self.line("mv s0, t5");
        if fr.plan.hidden_result.is_some() {
            self.mem("sd", "a0", "sp", fr.sret);
        }
        for (n, slot) in f.parameters.iter().enumerate() {
            let arg = &fr.plan.args[n];
            let l = &fr.layouts[slot.0 as usize];
            let dest = fr.offsets[slot.0 as usize];
            match arg.passing {
                Passing::Ignore => {}
                Passing::IndirectSnapshot => {
                    self.incoming(&arg.pieces[0].location, "t1");
                    self.address("t0", "sp", dest);
                    self.copy(l.size)
                }
                Passing::Direct => {
                    for p in &arg.pieces {
                        self.incoming(&p.location, "t0");
                        self.store_bytes("t0", "sp", dest + p.value_offset, p.size, l.align.min(8));
                    }
                }
            }
        }
        self.jump(&self.block_label(f, f.entry));
        for (n, b) in f.blocks.iter().enumerate() {
            self.line(format!("{}:", self.block_label(f, BlockId(n as u32))));
            for i in &b.instructions {
                self.instruction(f, &fr, i)?;
            }
            self.terminator(f, &fr, &b.terminator)?;
        }
        self.line(format!(
            ".size {}, .-{}",
            quote(&f.symbol),
            quote(&f.symbol)
        ));
        self.frames.push(fr.info);
        Ok(())
    }
    fn incoming(&mut self, l: &Location, r: &str) {
        match l {
            Location::Register(n) => self.line(format!("mv {r}, a{n}")),
            Location::Stack { offset } => self.mem("ld", r, "s0", *offset),
        }
    }
    fn outgoing(&mut self, l: &Location, r: &str) {
        match l {
            Location::Register(n) => self.line(format!("mv a{n}, {r}")),
            Location::Stack { offset } => self.mem("sd", r, "sp", *offset),
        }
    }
    fn jump(&mut self, label: &str) {
        self.line(format!("la t6, {label}"));
        self.line("jalr zero, 0(t6)");
    }
    fn block_label(&self, f: &Function, b: BlockId) -> String {
        quote(&format!(
            ".L{}_b{}",
            crate::linkage::symbol_alias(&f.symbol),
            b.0
        ))
    }
    fn epilogue(&mut self, fr: &Frame) {
        self.line("mv t5, s0");
        self.mem("ld", "ra", "sp", fr.save);
        self.mem("ld", "s0", "sp", fr.save + 8);
        self.line("mv sp, t5");
        self.line("ret");
    }
    fn panic(&mut self) {
        self.required.insert("__biomiya_panic".into());
        self.line("la a0, .Lbiomiya_failure_message");
        self.line("li a1, 20");
        self.line("call __biomiya_panic");
        self.line("unimp");
    }
    fn arithmetic(
        &mut self,
        f: &Function,
        fr: &Frame,
        dst: SlotId,
        op: &str,
        left: &Operand,
        right: &Operand,
    ) -> Result<()> {
        let ty = left.ty(f);
        if matches!(op, "==" | "!=") && !scalar_type(self.types.kind(ty)) {
            return self.aggregate_equality(f, fr, dst, op, left, right);
        }
        let (bits, signed) = match self.types.kind(ty) {
            TypeKind::Int(i) => (i.bits(), i.signed()),
            TypeKind::Bool => (1, false),
            TypeKind::Pointer { .. } | TypeKind::Ref { .. } | TypeKind::Function(_) => (64, false),
            _ => return Err(self.error(f.span, "binary operation requires scalar operands")),
        };
        self.operand(f, fr, left, "t0")?;
        self.operand(f, fr, right, "t1")?;
        let option = op.ends_with('?');
        let wrapping = matches!(op, "+%" | "-%" | "*%");
        let base = if option || wrapping {
            &op[..op.len() - 1]
        } else {
            op
        };
        let fail = self.fresh();
        let done = self.fresh();
        let check = !wrapping;
        match base {
            "+" | "-" => {
                self.line(format!(
                    "{} t2, t0, t1",
                    if base == "+" { "add" } else { "sub" }
                ));
                if check {
                    if signed && bits == 64 {
                        if base == "+" {
                            self.line("xor t3, t0, t2");
                            self.line("xor t4, t1, t2");
                        } else {
                            self.line("xor t3, t0, t1");
                            self.line("xor t4, t0, t2");
                        }
                        self.line("and t3, t3, t4");
                        self.line(format!("bltz t3, {fail}"));
                    } else if !signed {
                        if base == "+" {
                            self.line(format!("bltu t2, t0, {fail}"))
                        } else {
                            self.line(format!("bltu t0, t1, {fail}"))
                        }
                    }
                    if bits < 64 {
                        self.line("mv t3, t2");
                        self.normalize("t3", ty);
                        self.line(format!("bne t2, t3, {fail}"));
                    }
                }
            }
            "*" => {
                self.line("mul t2, t0, t1");
                if check {
                    self.line(format!(
                        "{} t3, t0, t1",
                        if signed { "mulh" } else { "mulhu" }
                    ));
                    if signed {
                        self.line("srai t4, t2, 63");
                        self.line(format!("bne t3, t4, {fail}"));
                    } else {
                        self.line(format!("bnez t3, {fail}"));
                    }
                    if bits < 64 {
                        self.line("mv t3, t2");
                        self.normalize("t3", ty);
                        self.line(format!("bne t2, t3, {fail}"));
                    }
                }
            }
            "/" | "%" => {
                self.line(format!("beqz t1, {fail}"));
                if signed {
                    let safe = self.fresh();
                    self.line(format!(
                        "li t3, {}",
                        if bits == 64 {
                            i64::MIN
                        } else {
                            -(1_i64 << (bits - 1))
                        }
                    ));
                    self.line(format!("bne t0, t3, {safe}"));
                    self.line("li t3, -1");
                    self.line(format!("beq t1, t3, {fail}"));
                    self.line(format!("{safe}:"));
                }
                self.line(format!(
                    "{}{} t2, t0, t1",
                    if base == "/" { "div" } else { "rem" },
                    if signed { "" } else { "u" }
                ));
            }
            "<<" | ">>" => {
                self.line(format!("li t3, {bits}"));
                self.line(format!("bgeu t1, t3, {fail}"));
                self.line(format!(
                    "{} t2, t0, t1",
                    if base == "<<" {
                        "sll"
                    } else if signed {
                        "sra"
                    } else {
                        "srl"
                    }
                ));
            }
            "&" | "|" | "^" => self.line(format!(
                "{} t2, t0, t1",
                match base {
                    "&" => "and",
                    "|" => "or",
                    _ => "xor",
                }
            )),
            "==" | "!=" => {
                self.line("xor t2, t0, t1");
                self.line(format!(
                    "{} t2, t2",
                    if base == "==" { "seqz" } else { "snez" }
                ));
            }
            "<" | ">" | "<=" | ">=" => {
                let reverse = base == ">" || base == "<=";
                self.line(format!(
                    "slt{} t2, {}, {}",
                    if signed { "" } else { "u" },
                    if reverse { "t1" } else { "t0" },
                    if reverse { "t0" } else { "t1" }
                ));
                if base == "<=" || base == ">=" {
                    self.line("xori t2, t2, 1");
                }
            }
            _ => return Err(self.error(f.span, format!("unimplemented binary operator {op}"))),
        }
        self.normalize("t2", ty);
        let dstoff = fr.offsets[dst.0 as usize];
        let dl = &fr.layouts[dst.0 as usize];
        if option {
            self.mem("sw", "zero", "sp", dstoff);
            let offset = dl.payload_offset.ok_or_else(|| {
                self.error(f.span, "Option arithmetic destination must be Option")
            })?;
            let il = self.layout(ty)?;
            self.store_bytes("t2", "sp", dstoff + offset, il.size, il.align);
        } else {
            self.store_bytes("t2", "sp", dstoff, dl.size, dl.align);
        }
        self.line(format!("j {done}"));
        self.line(format!("{fail}:"));
        if option {
            self.line("li t2, 1");
            self.mem("sw", "t2", "sp", dstoff);
        } else {
            self.panic();
        }
        self.line(format!("{done}:"));
        Ok(())
    }
    fn instruction(&mut self, f: &Function, fr: &Frame, i: &Instruction) -> Result<()> {
        match &i.kind {
            InstructionKind::Set { dst, value } => {
                let off = fr.offsets[dst.0 as usize];
                let l = &fr.layouts[dst.0 as usize];
                if l.size == 0 {
                    return Ok(());
                }
                match value {
                    Rvalue::Use(o) => {
                        if l.size > 8 || !scalar_type(self.types.kind(f.slots[dst.0 as usize].ty)) {
                            self.operand_address(f, fr, o, "t1")?;
                            self.address("t0", "sp", off);
                            self.copy(l.size);
                        } else {
                            self.operand(f, fr, o, "t0")?;
                            self.store_bytes("t0", "sp", off, l.size, l.align);
                        }
                    }
                    Rvalue::Load(p) => {
                        self.place(f, fr, p, "t1")?;
                        if scalar_type(self.types.kind(p.ty)) {
                            self.load_bytes("t0", "t1", 0, l.size, p.align);
                            self.store_bytes("t0", "sp", off, l.size, l.align);
                        } else {
                            self.address("t0", "sp", off);
                            self.copy(l.size);
                        }
                    }
                    Rvalue::Address(p) => {
                        self.place(f, fr, p, "t0")?;
                        self.store_bytes("t0", "sp", off, 8, 8);
                    }
                    Rvalue::Cast { value } => {
                        self.operand(f, fr, value, "t0")?;
                        self.normalize("t0", f.slots[dst.0 as usize].ty);
                        self.store_bytes("t0", "sp", off, l.size, l.align);
                    }
                    Rvalue::Unary { op, value } => {
                        self.operand(f, fr, value, "t0")?;
                        match op.as_str() {
                            "!" => self.line("seqz t0, t0"),
                            "~" => self.line("not t0, t0"),
                            "-" => {
                                let ty = value.ty(f);
                                let (bits, signed) = match self.types.kind(ty) {
                                    TypeKind::Int(i) => (i.bits(), i.signed()),
                                    _ => {
                                        return Err(self.error(i.span, "negation requires integer"));
                                    }
                                };
                                if !signed {
                                    return Err(
                                        self.error(i.span, "unsigned negation is not defined")
                                    );
                                }
                                let good = self.fresh();
                                self.line(format!(
                                    "li t1, {}",
                                    if bits == 64 {
                                        i64::MIN
                                    } else {
                                        -(1_i64 << (bits - 1))
                                    }
                                ));
                                self.line(format!("bne t0, t1, {good}"));
                                self.panic();
                                self.line(format!("{good}:"));
                                self.line("neg t0, t0");
                            }
                            _ => return Err(self.error(i.span, "unknown unary operator")),
                        };
                        self.normalize("t0", f.slots[dst.0 as usize].ty);
                        self.store_bytes("t0", "sp", off, l.size, l.align);
                    }
                    Rvalue::Binary { op, left, right } => {
                        self.arithmetic(f, fr, *dst, op, left, right)?
                    }
                }
            }
            InstructionKind::Store { dst, value } => {
                let l = self.layout(dst.ty)?;
                if l.size == 0 {
                    return Ok(());
                }
                if scalar_type(self.types.kind(dst.ty)) {
                    self.operand(f, fr, value, "t0")?;
                    self.place(f, fr, dst, "t1")?;
                    self.store_bytes("t0", "t1", 0, l.size, dst.align);
                } else {
                    self.place(f, fr, dst, "t0")?;
                    self.operand_address(f, fr, value, "t1")?;
                    self.copy(l.size);
                }
            }
            InstructionKind::Copy { dst, src, size } => {
                self.place(f, fr, dst, "t0")?;
                self.place(f, fr, src, "t1")?;
                self.copy(*size);
            }
            InstructionKind::Zero { dst, size } => {
                if *size > 0 {
                    self.place(f, fr, dst, "t0")?;
                    self.line(format!("li t1, {size}"));
                    let loop_ = self.fresh();
                    self.line(format!("{loop_}:"));
                    self.line("sb zero, 0(t0)");
                    self.line("addi t0, t0, 1");
                    self.line("addi t1, t1, -1");
                    self.line(format!("bnez t1, {loop_}"));
                }
            }
            InstructionKind::Call {
                target,
                signature,
                arguments,
                result,
            } => self.call(f, fr, target, signature, arguments, *result)?,
            InstructionKind::Intrinsic {
                name,
                arguments,
                result,
                type_arguments,
            } => self.intrinsic(f, fr, name, arguments, *result, type_arguments, i.span)?,
            InstructionKind::Asm {
                template,
                inputs,
                outputs,
                ..
            } => self.asm(f, fr, template, inputs, outputs)?,
            InstructionKind::Event(_) => {}
        }
        Ok(())
    }
    fn call(
        &mut self,
        f: &Function,
        fr: &Frame,
        target: &CallTarget,
        sig: &FnType,
        args: &[Operand],
        result: Option<SlotId>,
    ) -> Result<()> {
        let p = abi::classify(self.types, sig)?;
        if args.len() != p.args.len() {
            return Err(self.error(f.span, "IR call argument count mismatch"));
        }
        if let CallTarget::Indirect(o) = target {
            self.operand(f, fr, o, "t0")?;
            self.mem("sd", "t0", "sp", fr.scratch);
        }
        if let Some(loc) = p.hidden_result {
            let id = result
                .ok_or_else(|| self.error(f.span, "indirect call result requires storage"))?;
            self.address("t0", "sp", fr.offsets[id.0 as usize]);
            self.outgoing(&loc, "t0");
        }
        for (arg, o) in p.args.iter().zip(args) {
            if arg.passing == Passing::Ignore {
                continue;
            }
            if arg.passing == Passing::IndirectSnapshot {
                self.operand_address(f, fr, o, "t0")?;
                self.outgoing(&arg.pieces[0].location, "t0");
            } else if scalar_type(self.types.kind(arg.ty)) {
                self.operand(f, fr, o, "t0")?;
                self.extension("t0", arg.extension);
                self.outgoing(&arg.pieces[0].location, "t0");
            } else {
                self.operand_address(f, fr, o, "t1")?;
                let align = self.layout(arg.ty)?.align;
                for part in &arg.pieces {
                    self.load_bytes("t0", "t1", part.value_offset, part.size, align);
                    self.outgoing(&part.location, "t0");
                }
            }
        }
        match target {
            CallTarget::Direct(s) => {
                self.line(format!("la t5, {}", quote(s)));
                self.line("jalr ra, 0(t5)");
            }
            CallTarget::Indirect(_) => {
                self.mem("ld", "t5", "sp", fr.scratch);
                self.line("jalr ra, 0(t5)");
            }
        }
        if let (Some(id), ReturnLoc::Direct { pieces, .. }) = (result, &p.result) {
            let l = &fr.layouts[id.0 as usize];
            for part in pieces {
                let n = match part.location {
                    Location::Register(n) => n,
                    _ => unreachable!(),
                };
                self.store_bytes(
                    &format!("a{n}"),
                    "sp",
                    fr.offsets[id.0 as usize] + part.value_offset,
                    part.size,
                    l.align.min(8),
                );
            }
        }
        Ok(())
    }
    fn terminator(&mut self, f: &Function, fr: &Frame, t: &Terminator) -> Result<()> {
        match t {
            Terminator::Goto(b) => self.jump(&self.block_label(f, *b)),
            Terminator::Branch { condition, yes, no } => {
                self.operand(f, fr, condition, "t0")?;
                let skip = self.fresh();
                self.line(format!("beqz t0, {skip}"));
                self.jump(&self.block_label(f, *yes));
                self.line(format!("{skip}:"));
                self.jump(&self.block_label(f, *no));
            }
            Terminator::Switch {
                value,
                cases,
                otherwise,
            } => {
                self.operand(f, fr, value, "t0")?;
                for (v, b) in cases {
                    self.line(format!("li t1, {}", *v as i64));
                    let skip = self.fresh();
                    self.line(format!("bne t0, t1, {skip}"));
                    self.jump(&self.block_label(f, *b));
                    self.line(format!("{skip}:"));
                }
                self.jump(&self.block_label(f, *otherwise));
            }
            Terminator::Return(o) => {
                match (&fr.plan.result, o) {
                    (ReturnLoc::Indirect { size, .. }, Some(o)) => {
                        self.operand_address(f, fr, o, "t1")?;
                        self.mem("ld", "t0", "sp", fr.sret);
                        self.copy(*size);
                    }
                    (ReturnLoc::Direct { pieces, extension }, Some(o)) => {
                        if scalar_type(self.types.kind(o.ty(f))) {
                            self.operand(f, fr, o, "a0")?;
                            self.extension("a0", *extension);
                        } else {
                            self.operand_address(f, fr, o, "t1")?;
                            let align = self.layout(o.ty(f))?.align;
                            for part in pieces {
                                let n = match part.location {
                                    Location::Register(n) => n,
                                    _ => unreachable!(),
                                };
                                self.load_bytes(
                                    &format!("a{n}"),
                                    "t1",
                                    part.value_offset,
                                    part.size,
                                    align,
                                );
                            }
                        }
                    }
                    (ReturnLoc::Void, _) => {}
                    (ReturnLoc::Never, _) => {
                        return Err(self.error(f.span, "Never function has normal return"));
                    }
                    _ => return Err(self.error(f.span, "missing return value")),
                }
                self.epilogue(fr);
            }
            Terminator::Panic(Some(o)) => {
                self.operand_address(f, fr, o, "t0")?;
                self.mem("ld", "a0", "t0", 0);
                self.mem("ld", "a1", "t0", 8);
                self.required.insert("__biomiya_panic".into());
                self.line("call __biomiya_panic");
                self.line("unimp");
            }
            Terminator::Panic(None) => self.panic(),
            Terminator::Unreachable => self.line("unimp"),
            Terminator::SpecialTransfer {
                frame,
                handler,
                irq,
            } => {
                self.operand(f, fr, frame, "a0")?;
                self.line(format!("la a1, {}", quote(handler)));
                self.line(format!("li a2, {}", u8::from(*irq)));
                self.required.insert("__biomiya_dispatch".into());
                self.line("call __biomiya_dispatch");
                self.line("unimp");
            }
        }
        Ok(())
    }
    fn asm(
        &mut self,
        f: &Function,
        fr: &Frame,
        template: &str,
        inputs: &[AsmInput],
        outputs: &[AsmOutput],
    ) -> Result<()> {
        let size = align_up((inputs.len() + 2 * outputs.len()) as u64 * 8, 16)?.max(16);
        self.line(format!("addi sp, sp, -{size}"));
        let shifted = Frame {
            offsets: fr.offsets.iter().map(|x| x + size).collect(),
            layouts: fr.layouts.clone(),
            save: fr.save + size,
            sret: fr.sret + size,
            scratch: fr.scratch + size,
            plan: fr.plan.clone(),
            info: fr.info.clone(),
        };
        for (n, input) in inputs.iter().enumerate() {
            self.operand(f, &shifted, &input.value, "t0")?;
            self.mem("sd", "t0", "sp", n as u64 * 8);
        }
        let addresses = inputs.len() as u64 * 8;
        let values = addresses + outputs.len() as u64 * 8;
        for (n, output) in outputs.iter().enumerate() {
            self.place(f, &shifted, &output.place, "t0")?;
            self.mem("sd", "t0", "sp", addresses + n as u64 * 8);
        }
        for (n, input) in inputs
            .iter()
            .enumerate()
            .filter(|(_, i)| i.register != "t6")
        {
            self.mem("ld", &input.register, "sp", n as u64 * 8);
        }
        for (n, input) in inputs
            .iter()
            .enumerate()
            .filter(|(_, i)| i.register == "t6")
        {
            self.mem("ld", &input.register, "sp", n as u64 * 8);
        }
        self.line(template);
        for (n, output) in outputs.iter().enumerate() {
            self.mem("sd", &output.register, "sp", values + n as u64 * 8);
        }
        for (n, output) in outputs.iter().enumerate() {
            let l = self.layout(output.place.ty)?;
            self.mem("ld", "t0", "sp", values + n as u64 * 8);
            self.mem("ld", "t1", "sp", addresses + n as u64 * 8);
            self.store_bytes("t0", "t1", 0, l.size, output.place.align);
        }
        self.line(format!("addi sp, sp, {size}"));
        Ok(())
    }
    // Keep source identity, type arguments and physical frame context explicit.
    #[allow(clippy::too_many_arguments)]
    fn intrinsic(
        &mut self,
        f: &Function,
        fr: &Frame,
        name: &str,
        args: &[Operand],
        result: Option<SlotId>,
        type_args: &[TyId],
        span: Span,
    ) -> Result<()> {
        match name {
            "arch::riscv::cpu_relax" => self.line("nop"),
            "arch::riscv::wfi" => self.line("wfi"),
            "arch::device_read_fence" | "arch::device_write_fence" => self.line("fence iorw, iorw"),
            "arch::riscv::fence_i" => self.line("fence.i"),
            "arch::riscv::sfence_vma_all" => self.line("sfence.vma zero, zero"),
            "arch::riscv::csr_read" | "arch::riscv::csr_write" => {
                let number = match args.first() {
                    Some(Operand::Integer { bits, .. }) => *bits,
                    _ => return Err(self.error(span, "CSR number must be compile-time integer")),
                };
                if ![
                    0x100, 0x104, 0x105, 0x140, 0x141, 0x142, 0x143, 0x144, 0x180,
                ]
                .contains(&number)
                {
                    return Err(self.error(span, "unsupported CSR number"));
                }
                if name.ends_with("read") {
                    self.line(format!("csrr t0, {number}"));
                    self.save_result(fr, result, "t0")?;
                } else {
                    self.operand(f, fr, &args[1], "t0")?;
                    self.line(format!("csrw {number}, t0"));
                }
            }
            "volatile_load" | "mmio_load" | "volatile_store" | "mmio_store" => {
                let ty = *type_args
                    .first()
                    .ok_or_else(|| self.error(span, "volatile intrinsic needs payload type"))?;
                let l = self.layout(ty)?;
                if !matches!(l.size, 1 | 2 | 4 | 8) {
                    return Err(self.error(span, "volatile/MMIO width must be 1, 2, 4 or 8"));
                }
                self.operand(f, fr, &args[0], "t1")?;
                if name.ends_with("load") {
                    self.load_bytes("t0", "t1", 0, l.size, l.align);
                    self.normalize("t0", ty);
                    self.save_result(fr, result, "t0")?;
                } else {
                    self.operand(f, fr, &args[1], "t0")?;
                    self.store_bytes("t0", "t1", 0, l.size, l.align);
                }
            }
            n if n.starts_with("atomic.") => {
                self.atomic(f, fr, n, args, result, type_args, span)?
            }
            "addr_of" | "addr_of_mut" => {
                self.operand(f, fr, &args[0], "t0")?;
                self.save_result(fr, result, "t0")?;
            }
            "addr" | "ptr" => {
                self.operand(f, fr, &args[0], "t0")?;
                self.save_result(fr, result, "t0")?;
            }
            "forget" => {}
            _ => return Err(self.error(span, format!("intrinsic {name} is not implemented"))),
        }
        Ok(())
    }
    fn save_result(&mut self, fr: &Frame, result: Option<SlotId>, r: &str) -> Result<()> {
        if let Some(id) = result {
            let l = &fr.layouts[id.0 as usize];
            self.store_bytes(r, "sp", fr.offsets[id.0 as usize], l.size, l.align);
        }
        Ok(())
    }
    // Atomic payload types and memory-order operands are separate semantic inputs.
    #[allow(clippy::too_many_arguments)]
    fn atomic(
        &mut self,
        f: &Function,
        fr: &Frame,
        name: &str,
        args: &[Operand],
        result: Option<SlotId>,
        type_args: &[TyId],
        span: Span,
    ) -> Result<()> {
        let ty = *type_args
            .first()
            .ok_or_else(|| self.error(span, "atomic intrinsic needs payload type"))?;
        let atomic = self.types.intern(TypeKind::Atomic(ty));
        let storage = self.layout(atomic)?;
        let payload = self.layout(ty)?;
        let width = if storage.size == 8 { "d" } else { "w" };
        let narrow = payload.size < 4;
        self.operand(f, fr, &args[0], "t0")?;
        if name == "atomic.load" {
            self.line("fence rw, rw");
            self.line(format!(
                "{} t2, 0(t0)",
                if width == "d" { "ld" } else { "lwu" }
            ));
            self.line("fence r, rw");
            self.normalize("t2", ty);
            return self.save_result(fr, result, "t2");
        }
        self.operand(f, fr, &args[1], "t1")?;
        if name == "atomic.store" {
            self.line("fence rw, w");
            self.line(format!(
                "{} t1, 0(t0)",
                if width == "d" { "sd" } else { "sw" }
            ));
            self.line("fence rw, rw");
            return Ok(());
        }
        if name == "atomic.compare_exchange" {
            self.operand(f, fr, &args[2], "t4")?;
            let retry = self.fresh();
            let failed = self.fresh();
            let done = self.fresh();
            self.line("fence rw, rw");
            self.line(format!("{retry}:"));
            self.line(format!("lr.{width}.aqrl t2, (t0)"));
            if width == "w" {
                self.line("slli t2, t2, 32");
                self.line("srli t2, t2, 32");
            }
            self.line(format!("bne t2, t1, {failed}"));
            self.line(format!("sc.{width}.rl t3, t4, (t0)"));
            self.line(format!("bnez t3, {retry}"));
            self.line("li t3, 0");
            self.line(format!("j {done}"));
            self.line(format!("{failed}:"));
            self.line("li t3, 1");
            self.line(format!("{done}:"));
            if let Some(id) = result {
                let l = &fr.layouts[id.0 as usize];
                self.mem("sw", "t3", "sp", fr.offsets[id.0 as usize]);
                self.store_bytes(
                    "t2",
                    "sp",
                    fr.offsets[id.0 as usize]
                        + l.payload_offset
                            .ok_or_else(|| self.error(span, "CAS result must be Result"))?,
                    payload.size,
                    payload.align,
                );
            }
            return Ok(());
        }
        let op = match name {
            "atomic.exchange" => "swap",
            "atomic.fetch_add" => "add",
            "atomic.fetch_sub" => {
                self.line("neg t1, t1");
                "add"
            }
            "atomic.fetch_and" => "and",
            "atomic.fetch_or" => "or",
            _ => return Err(self.error(span, "unsupported atomic operation")),
        };
        if narrow && matches!(op, "add" | "and" | "or") {
            let retry = self.fresh();
            self.line(format!("{retry}:"));
            self.line("lr.w.aqrl t2, (t0)");
            self.line(format!("{op} t3, t2, t1"));
            self.normalize("t3", ty);
            self.line("sc.w.rl t4, t3, (t0)");
            self.line(format!("bnez t4, {retry}"));
        } else {
            self.line(format!("amo{op}.{width}.aqrl t2, t1, (t0)"));
        }
        self.normalize("t2", ty);
        self.save_result(fr, result, "t2")
    }
}
fn scalar_type(k: &TypeKind) -> bool {
    matches!(
        k,
        TypeKind::Int(_)
            | TypeKind::Bool
            | TypeKind::Pointer { .. }
            | TypeKind::Ref { .. }
            | TypeKind::Function(_)
    )
}

fn quote(s: &str) -> String {
    crate::linkage::asm_symbol(s)
}
impl Emitter<'_> {
    fn aggregate_equality(
        &mut self,
        f: &Function,
        fr: &Frame,
        dst: SlotId,
        op: &str,
        left: &Operand,
        right: &Operand,
    ) -> Result<()> {
        let ty = left.ty(f);
        let fail = self.fresh();
        let done = self.fresh();
        if !matches!(self.types.kind(ty), TypeKind::Unit) {
            self.operand_address(f, fr, left, "t0")?;
            self.operand_address(f, fr, right, "t1")?;
            self.compare_fields(ty, 0, &fail, f.span, &mut 0)?;
        }
        self.line(format!("li t2, {}", u8::from(op == "==")));
        self.line(format!("j {done}"));
        self.line(format!("{fail}:"));
        self.line(format!("li t2, {}", u8::from(op == "!=")));
        self.line(format!("{done}:"));
        self.mem("sb", "t2", "sp", fr.offsets[dst.0 as usize]);
        Ok(())
    }
    fn mismatch(&mut self, fail: &str) {
        let equal = self.fresh();
        self.line(format!("beq t5, t3, {equal}"));
        self.jump(fail);
        self.line(format!("{equal}:"));
    }
    fn compare_fields(
        &mut self,
        ty: TyId,
        offset: u64,
        fail: &str,
        span: Span,
        count: &mut u64,
    ) -> Result<()> {
        use TypeKind::*;
        *count += 1;
        if *count > 100_000 {
            return Err(Diagnostic::new(
                "I_LIMIT",
                span,
                "aggregate equality expansion exceeds implementation capacity",
            ));
        }
        let k = self.types.kind(ty).clone();
        let l = self.layout(ty)?;
        match k {
            Unit => {}
            Int(_) | Bool | Pointer { .. } | Ref { .. } | Function(_) => {
                self.load_bytes("t2", "t0", offset, l.size, 1);
                self.line("mv t5, t2");
                self.load_bytes("t3", "t1", offset, l.size, 1);
                self.mismatch(fail);
            }
            Array(x, n) => {
                let step = self.layout(x)?.size;
                for i in 0..n {
                    self.compare_fields(x, offset + i * step, fail, span, count)?
                }
            }
            ArrayExpr(x, len) => {
                let n = LayoutEngine::new(self.types).evaluate_len(&len)?;
                let step = self.layout(x)?.size;
                for i in 0..n {
                    self.compare_fields(x, offset + i * step, fail, span, count)?
                }
            }
            Tuple(ts) => {
                for (t, field) in ts.into_iter().zip(l.fields) {
                    self.compare_fields(t, offset + field, fail, span, count)?
                }
            }
            Nominal(id, args) => {
                let d = self
                    .types
                    .definition(id)
                    .cloned()
                    .ok_or_else(|| self.error(span, "undefined equality type"))?;
                match d.kind {
                    DefKind::Struct(fs) => {
                        for (f, field) in fs.into_iter().zip(l.fields) {
                            let t = self.types.substitute(f.ty, &args);
                            self.compare_fields(t, offset + field, fail, span, count)?
                        }
                    }
                    DefKind::Enum(vs) => {
                        let variants = vs
                            .into_iter()
                            .map(|v| {
                                v.fields
                                    .into_iter()
                                    .map(|f| self.types.substitute(f.ty, &args))
                                    .collect()
                            })
                            .collect();
                        self.compare_enum(variants, &l, offset, fail, span, count)?
                    }
                }
            }
            Option(t) => self.compare_enum(vec![vec![t], vec![]], &l, offset, fail, span, count)?,
            Result(t, e) => {
                self.compare_enum(vec![vec![t], vec![e]], &l, offset, fail, span, count)?
            }
            _ => return Err(self.error(span, "type does not support aggregate equality")),
        }
        Ok(())
    }
    fn compare_enum(
        &mut self,
        variants: Vec<Vec<TyId>>,
        l: &Layout,
        offset: u64,
        fail: &str,
        span: Span,
        count: &mut u64,
    ) -> Result<()> {
        self.load_bytes("t2", "t0", offset, 4, 1);
        self.line("mv t5, t2");
        self.load_bytes("t3", "t1", offset, 4, 1);
        self.mismatch(fail);
        let done = self.fresh();
        let labels: Vec<_> = variants.iter().map(|_| self.fresh()).collect();
        for (n, label) in labels.iter().enumerate() {
            let next = self.fresh();
            self.line(format!("li t2, {n}"));
            self.line(format!("bne t5, t2, {next}"));
            self.jump(label);
            self.line(format!("{next}:"));
        }
        self.jump(fail);
        for ((ts, v), label) in variants.into_iter().zip(&l.variants).zip(labels) {
            self.line(format!("{label}:"));
            for (t, field) in ts.into_iter().zip(&v.fields) {
                self.compare_fields(
                    t,
                    offset + l.payload_offset.unwrap() + field,
                    fail,
                    span,
                    count,
                )?
            }
            self.jump(&done);
        }
        self.line(format!("{done}:"));
        Ok(())
    }
}
fn collect_linkage(module: &Module) -> crate::linkage::LinkageNames {
    use crate::linkage::{LinkageNames, NameMapping, name_bytes, section_alias, symbol_alias};
    let mut symbols = std::collections::BTreeMap::new();
    let mut sections = std::collections::BTreeMap::new();
    fn symbol(name: &str, map: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        let alias = symbol_alias(name);
        if alias != name {
            map.insert(alias, name_bytes(name).into_owned());
        }
    }
    fn operand(value: &Operand, map: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        if let Operand::Symbol { name, .. } = value {
            symbol(name, map)
        }
    }
    fn place(value: &Place, map: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        match &value.base {
            PlaceBase::Global(name) => symbol(name, map),
            PlaceBase::Pointer(value) => operand(value, map),
            _ => {}
        }
    }
    for name in &module.externals {
        symbol(name, &mut symbols)
    }
    for data in &module.data {
        symbol(&data.symbol, &mut symbols);
        if let Some(name) = &data.section {
            sections.insert(section_alias(name), name_bytes(name).into_owned());
        }
        for relocation in &data.relocations {
            symbol(&relocation.symbol, &mut symbols)
        }
    }
    for function in &module.functions {
        symbol(&function.symbol, &mut symbols);
        if let Some(name) = &function.section {
            sections.insert(section_alias(name), name_bytes(name).into_owned());
        }
        for block in &function.blocks {
            for instruction in &block.instructions {
                match &instruction.kind {
                    InstructionKind::Set { value, .. } => match value {
                        Rvalue::Use(value)
                        | Rvalue::Unary { value, .. }
                        | Rvalue::Cast { value } => operand(value, &mut symbols),
                        Rvalue::Load(value) | Rvalue::Address(value) => place(value, &mut symbols),
                        Rvalue::Binary { left, right, .. } => {
                            operand(left, &mut symbols);
                            operand(right, &mut symbols)
                        }
                    },
                    InstructionKind::Store { dst, value } => {
                        place(dst, &mut symbols);
                        operand(value, &mut symbols)
                    }
                    InstructionKind::Copy { dst, src, .. } => {
                        place(dst, &mut symbols);
                        place(src, &mut symbols)
                    }
                    InstructionKind::Zero { dst, .. } => place(dst, &mut symbols),
                    InstructionKind::Call {
                        target, arguments, ..
                    } => {
                        match target {
                            CallTarget::Direct(name) => symbol(name, &mut symbols),
                            CallTarget::Indirect(value) => operand(value, &mut symbols),
                        }
                        for value in arguments {
                            operand(value, &mut symbols)
                        }
                    }
                    InstructionKind::Intrinsic { arguments, .. } => {
                        for value in arguments {
                            operand(value, &mut symbols)
                        }
                    }
                    InstructionKind::Asm {
                        inputs, outputs, ..
                    } => {
                        for input in inputs {
                            operand(&input.value, &mut symbols)
                        }
                        for output in outputs {
                            place(&output.place, &mut symbols)
                        }
                    }
                    InstructionKind::Event(_) => {}
                }
            }
            match &block.terminator {
                Terminator::Branch { condition, .. } => operand(condition, &mut symbols),
                Terminator::Switch { value, .. } => operand(value, &mut symbols),
                Terminator::Return(Some(value)) | Terminator::Panic(Some(value)) => {
                    operand(value, &mut symbols)
                }
                Terminator::SpecialTransfer { frame, handler, .. } => {
                    operand(frame, &mut symbols);
                    symbol(handler, &mut symbols)
                }
                _ => {}
            }
        }
    }
    LinkageNames {
        symbols: symbols
            .into_iter()
            .map(|(alias, bytes)| NameMapping { alias, bytes })
            .collect(),
        sections: sections
            .into_iter()
            .map(|(alias, bytes)| NameMapping { alias, bytes })
            .collect(),
    }
}
