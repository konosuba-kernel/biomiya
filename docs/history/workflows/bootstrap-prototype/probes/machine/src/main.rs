use biomiya_core_s::{encode,image,backend,machine::*,model::*,source::Span,ast::BinaryOp};
use std::{fs,process::Command,os::unix::fs::PermissionsExt};
fn main(){
 let mut seed=0x97102173b3214a9du64;
 for n in 0..20000 {
  seed=seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
  let value=if n<64 {1u64<<n} else {seed}; let rd=(n%31+1) as u8;
  let out=encode::encode(&Program{instructions:vec![Inst::Li{rd,bits:value}]},0).unwrap();
  let mut regs=[0x2233445566778899u64;32];regs[0]=0;let initial=regs;
  for b in out.bytes.chunks_exact(4) {let w=u32::from_le_bytes(b.try_into().unwrap());assert_eq!(w&127,0x13);let d=((w>>7)&31)as usize;let s=((w>>15)&31)as usize;let imm=(w as i32>>20)as i64;regs[d]=match (w>>12)&7{0=>regs[s].wrapping_add(imm as u64),1=>regs[s]<<((w>>20)&63),_=>panic!("unexpected instruction")};regs[0]=0;}
  assert_eq!(regs[rd as usize],value); for r in 0..32 {if r!=rd as usize{assert_eq!(regs[r],initial[r]);}}
 }
 for pad in [508,509,510,511,512,513,514,1020,1021,1022,1023,1024,1025] {
  let mut ops=vec![Inst::Label("begin".into()),Inst::Address{rd:5,target:"end".into()}];ops.extend((0..pad).map(|_|Inst::Ecall));ops.push(Inst::Label("end".into()));ops.push(Inst::Address{rd:7,target:"begin".into()});
  let e=encode::encode(&Program{instructions:ops},0xffff_ffff_0000_0000).unwrap();
  for (off,target) in [(0usize,e.symbols["end"]),(8+pad*4,e.symbols["begin"])] {let a=u32::from_le_bytes(e.bytes[off..off+4].try_into().unwrap());let b=u32::from_le_bytes(e.bytes[off+4..off+8].try_into().unwrap());let hi=(a&0xfffff000)as i32 as i64;let lo=(b as i32>>20)as i64;assert_eq!((0xffff_ffff_0000_0000u64+off as u64).wrapping_add(hi as u64).wrapping_add(lo as u64),target);}
 }
 let mut types=TypeTable::default();types.entries.push(TypeInfo{kind:TypeKind::Int(Integer::Usize),layout:Layout{size:8,align:8,field_offsets:vec![],payload_offset:None}});
 let cases=[(BinaryOp::Div,u64::MAX,3,u64::MAX/3),(BinaryOp::Rem,u64::MAX,257,u64::MAX%257),(BinaryOp::WrappingAdd,u64::MAX,1,0),(BinaryOp::WrappingMul,0x8123456789abcdef,137,0x8123456789abcdefu64.wrapping_mul(137)),(BinaryOp::Shr,u64::MAX,63,1),(BinaryOp::Shl,1,63,1<<63)];
 let dir=std::env::temp_dir().join(format!("core-s-independent-images-{}",std::process::id()));fs::create_dir(&dir).unwrap();
 for (i,(op,a,b,expected)) in cases.into_iter().enumerate(){let c=Compilation{types:types.clone(),functions:vec![Function{name:"run".into(),symbol:"__core_s_f0".into(),signature:Signature{parameters:vec![],result:TypeId(0),unsafe_:false},parameter_slots:vec![],slots:vec![Slot{ty:TypeId(0),name:None,span:Span::default()}],blocks:vec![Block{instructions:vec![Instruction{kind:InstructionKind::Set{dst:SlotId(0),value:Rvalue::Binary{op,left:Operand::Immediate{ty:TypeId(0),bits:a},right:Operand::Immediate{ty:TypeId(0),bits:b}}},span:Span::default()}],terminator:Terminator::Return(Operand::Slot(SlotId(0))),span:Span::default()}],entry:BlockId(0),span:Span::default()}]};
 let mut p=backend::machine(&c).unwrap();p.instructions.extend([Inst::Label("_start".into()),Inst::Jump{rd:RA,target:"__core_s_f0".into()},Inst::Li{rd:5,bits:expected},Inst::Branch{condition:Condition::Ne,rs1:10,rs2:5,target:"__core_s_panic".into()},Inst::Li{rd:10,bits:0},Inst::Li{rd:17,bits:93},Inst::Ecall,Inst::Label("__core_s_panic".into()),Inst::Li{rd:10,bits:101},Inst::Li{rd:17,bits:93},Inst::Ecall]);
 let f=dir.join(format!("{i}.elf"));fs::write(&f,image::executable(&p,"_start").unwrap()).unwrap();fs::set_permissions(&f,fs::Permissions::from_mode(0o755)).unwrap();let o=Command::new("qemu-riscv64").arg(&f).output().unwrap();assert!(o.status.success(),"case {i}: {:?}",o);}
 fs::remove_dir_all(dir).unwrap();println!("passed: 20,000 Li patterns with register preservation; 26 PCREL carry/negative/base-high checks; six typed-IR direct ELF QEMU executions");
}
