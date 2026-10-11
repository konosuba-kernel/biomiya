use biomiya_core_s::{frontend,check,backend,image,s0,source::FileId,model::Compilation,diagnostic::Result,machine::*};
fn compile(files:&[&str])->Result<Compilation>{let p=files.iter().enumerate().map(|(i,s)|frontend::parse(FileId(i as u32),s)).collect::<Result<Vec<_>>>()?;check::check(&frontend::resolve_files(p)?)}
#[test]
fn permissions_and_public_recursive_types(){
 for files in [
  vec!["module lib; struct Hidden{n:u32} pub struct Outer{pub data:[]const Hidden}"],
  vec!["module lib; static secret:u32=2;","module app;fn run()->u32{return root::lib::secret;}"],
  vec!["fn run()->u32{let s:[]const u8=\"a\";unsafe{let m:[]u8=core::slice_from_raw_parts_mut(s.data,1);m[0]=2;}return 0;}"],
  vec!["static mut A:[1]u8=[0];fn run()->u32{let p:*u8=core::addr_of_mut(A[0]);return 0;}"],
  vec!["extern abi(C) fn f(x:[1]u8)->u32;"],
  vec!["extern abi(C) fn 外部()->u32;"],
 ]{assert!(compile(&files).is_err(),"accepted forbidden program {files:?}");}
 compile(&["module lib; pub struct Cell{pub next:*const Cell} pub static C:Cell=Cell{next:core::addr_of(C)};","module app;import lib;fn run()->u32{let p:*const lib::Cell=core::addr_of(lib::C);return 0;}"]).unwrap();
}
fn run(source:&str)->i32{
 let c=compile(&[source]).unwrap(); let mut p=s0::parse(FileId(99),"_start:\ncall __core_s_f0\naddi x17,x0,93\necall\n__core_s_panic:\naddi x10,x0,101\naddi x17,x0,93\necall\n").unwrap();let body=backend::machine(&c).unwrap();p.instructions.extend(body.instructions);p.data=body.data;
 let bytes=image::executable(&p,"_start").unwrap();let path=std::env::temp_dir().join(format!("b2-review-{}-{}.elf",std::process::id(),source.len()));std::fs::write(&path,bytes).unwrap(); use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&path,std::fs::Permissions::from_mode(0o700)).unwrap();
 let o=std::process::Command::new("timeout").args(["5s","qemu-riscv64"]).arg(&path).output().unwrap();std::fs::remove_file(path).unwrap();o.status.code().unwrap_or(-1)
}
#[test]
fn actual_source_slice_snapshot_static_and_bounds(){
 assert_eq!(run("static mut B:[2]u8=[0;2];static P:*[2]u8=core::addr_of_mut(B);fn run()->u32{unsafe{let s:[]u8=core::slice_of_mut(*P);let t:[]u8=s;s[1]=17;return t[1] as u32;}}"),17);
 assert_eq!(run("fn run()->u32{let s:[]const u8=\"\";unsafe{return s[0] as u32;}}"),101);
 assert_eq!(run("fn run()->u32{var a:[2]u8=[1,2];let s:[]u8=core::slice_of_mut(a);defer{unsafe{s[1]=9;}}unsafe{return s[1] as u32;}}"),2);
}
#[test]
fn independently_malformed_data_and_s0_spans(){
 let mut p=Program{instructions:vec![Inst::Label("start".into()),Inst::Unimp],data:vec![DataObject{symbol:"data".into(),mutable:false,align:8,bytes:vec![0;16],zero_fill:0,relocations:vec![DataRelocation{offset:0,target:"start".into(),addend:-0x10001}]}]};
 assert!(image::executable(&p,"start").unwrap_err().message.contains("overflow"));
 p.data[0].relocations[0].addend=0;p.data[0].relocations.push(DataRelocation{offset:7,target:"start".into(),addend:0});assert!(image::executable(&p,"start").is_err());
 p.data[0].relocations.clear();p.data[0].symbol="start".into();assert!(image::executable(&p,"start").is_err());
 let src="# α\r\n\n   ld x1, 2048(x2) # bad\r\n";let e=s0::parse(FileId(42),src).unwrap_err();assert_eq!(e.span.file,FileId(42));assert_eq!(&src[e.span.start as usize..e.span.end as usize],"   ld x1, 2048(x2) # bad");
}
