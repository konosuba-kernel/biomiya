#![cfg(all(feature = "frontend", feature = "check"))]
use biomiya_core_s::{check, diagnostic::Result, frontend, model::*, source::FileId};
fn compile(files: &[&str]) -> Result<Compilation> {
    let programs = files
        .iter()
        .enumerate()
        .map(|(i, s)| frontend::parse(FileId(i as u32 + 1), s))
        .collect::<Result<Vec<_>>>()?;
    check::check(&frontend::resolve_files(programs)?)
}
fn reject(source: &str, code: &str) {
    let d = compile(&[source]).unwrap_err();
    assert_eq!(d.code, code, "{}", d.message);
}
#[test]
fn modules_merge_and_private_boundaries() {
    let c = compile(&[
        "module lib; pub struct Item {pub n:u32} pub fn value(x:Item)->u32{return x.n;}",
        "module app; import lib; pub fn run()->u32{return lib::value(lib::Item{n:7});}",
        "module lib; fn hidden()->u32{return value(Item{n:2});}",
    ])
    .unwrap();
    assert_eq!(c.functions[1].name, "app::run");
    assert_eq!(
        compile(&[
            "module lib; fn hidden()->u32{return 1;}",
            "module app; import lib; fn run()->u32{return lib::hidden();}"
        ])
        .unwrap_err()
        .code,
        "E_NAME"
    );
    assert_eq!(
        compile(&[
            "fn hidden()->u32{return 1;}",
            "module app; fn run()->u32{return hidden();}"
        ])
        .unwrap_err()
        .code,
        "E_NAME"
    );
    assert_eq!(
        compile(&[
            "pub fn visible()->u32{return 1;}",
            "module app; fn run()->u32{return root::visible();}"
        ])
        .unwrap()
        .functions
        .len(),
        2
    );
    reject(
        "struct Hidden {} pub fn leak(x:*const Hidden)->u32{return 0;}",
        "E_NAME",
    );
    reject("pub struct Mixed { n:u32 }", "I_UNSUPPORTED");
    reject("module core; fn run()->u32{return 0;}", "E_NAME");
    assert_eq!(
        compile(&[
            "module lib; pub fn f()->u32{return 0;}",
            "module app; import lib; fn lib()->u32{return 0;}"
        ])
        .unwrap_err()
        .code,
        "E_NAME"
    );
}
#[test]
fn data_strings_bss_and_root_relocations() {
    let c=compile(&[r#"
 struct Link {next:*const Link}
 static HEAD:Link=Link{next:core::addr_of(HEAD)};
 static TEXT:[]const u8="hi\0\xFF";
 static mut BUFFER:[8]u8=[0;8];
 static ROOT:*[8]u8=core::addr_of_mut(BUFFER);
 fn run()->u32 {let empty:[]const u8="";let p:*[8]u8=core::addr_of_mut(BUFFER);return empty.len as u32;}
 "#]).unwrap();
    assert!(
        c.data
            .iter()
            .any(|d| d.zero_fill == 8 && d.bytes.is_empty())
    );
    assert!(c.data.iter().any(|d| d.bytes == vec![104, 105, 0, 255]));
    assert!(
        c.data
            .iter()
            .any(|d| d.relocations.iter().any(|r| r.target == "__core_s_g0"))
    );
    assert!(
        c.data
            .iter()
            .any(|d| d.symbol.starts_with("__core_s_string") && d.bytes == [0])
    );
    reject("static mut V:u32=0;fn run()->u32{return V;}", "E_UNSAFE");
    reject(
        "static V:u32=0;fn run()->u32{unsafe {V=1;}return 0;}",
        "E_TYPE",
    );
    reject(
        "static V:u32=0;static P:*u32=core::addr_of_mut(V);",
        "E_TYPE",
    );
    reject("static V:u8=256;", "E_TYPE");
    reject("static V:u32=1_u8;", "E_TYPE");
    reject("static V:u32=1;static W:u32=V;", "E_CONST");
    reject("static V:u32=1_u32+2_u32;", "I_UNSUPPORTED");
}
#[test]
fn slices_pointer_builtins_and_permissions() {
    let c=compile(&[r#"
 fn run()->u32 {
 var bytes:[2]u8=[3,4];let s:[]u8=core::slice_of_mut(bytes);let c:[]const u8=core::slice_const(s);
 let p:*const u8=core::ptr_const(s.data);let a:usize=core::addr(p);
 unsafe {let q:*u8=core::ptr::<u8>(a);let r:*u8=core::ptr_add(q,1);let b:*u8=core::ptr_add_bytes(r,0);let raw:[]u8=core::slice_from_raw_parts_mut(b,1);raw[0]=9;
 let ro:[]const u8=core::slice_from_raw_parts(p,2);return ro[1] as u32;}
 }
 "#]).unwrap();
    assert!(
        c.types
            .entries
            .iter()
            .any(|t| matches!(t.kind, TypeKind::Slice { .. })
                && t.layout.size == 16
                && t.layout.align == 8)
    );
    assert!(
        c.functions[0]
            .blocks
            .iter()
            .flat_map(|b| &b.instructions)
            .any(|i| matches!(i.kind, InstructionKind::BoundsDynamic { .. }))
    );
    reject(
        "fn run()->u32{let s:[]const u8=\"a\";return s[0] as u32;}",
        "E_UNSAFE",
    );
    reject(
        "fn run()->u32{let s:[]const u8=\"a\";unsafe{s[0]=2;}return 0;}",
        "E_TYPE",
    );
    reject(
        "fn run()->u32{var s:[]const u8=\"a\";s.len=2;return 0;}",
        "E_TYPE",
    );
    reject(
        "fn run()->u32{let p:*u8=core::ptr::<u8>(1);return 0;}",
        "E_UNSAFE",
    );
    reject(
        "fn run()->u32{let a:[1]u8=[0];let s:[]u8=core::slice_of_mut(a);return 0;}",
        "E_TYPE",
    );
    reject("fn run()->u32{let s:[]const u8=\"\\q\";return 0;}", "E_LEX");
}
#[test]
fn externs_are_scalar_unsafe_placeholders() {
    let c=compile(&["extern abi(C) fn os(n:usize,p:*const u8)->usize;fn run()->u32{unsafe{return os(1,\"x\".data) as u32;}}"]).unwrap();
    assert_eq!(c.externs[&FunctionId(0)], "os");
    assert!(c.functions[0].blocks.is_empty());
    reject(
        "extern abi(C) fn os()->u32;fn run()->u32{return os();}",
        "E_UNSAFE",
    );
    reject("extern abi(C) fn os(s:[]const u8)->u32;", "E_TYPE");
    reject("extern abi(C) fn os()->[2]u8;", "E_TYPE");
}

#[test]
fn merged_imports_prefix_modules_and_shadowing() {
    compile(&[
        "module memory::page; pub fn get()->u32{return 3;}",
        "module app; import memory;",
        "module app; fn run()->u32{return memory::page::get();}",
    ])
    .unwrap();
    assert_eq!(
        compile(&[
            "module a; fn run()->u32{return 0;}",
            "module a; fn run()->u32{return 1;}"
        ])
        .unwrap_err()
        .code,
        "E_NAME"
    );
    assert_eq!(
        compile(&["fn a()->u32{return 0;}", "module a::child;"])
            .unwrap_err()
            .code,
        "E_NAME"
    );
    reject(
        "struct Thing {} fn run()->u32{let Thing:u32=0;let x:Thing=Thing{};return 0;}",
        "E_TYPE",
    );
    compile(&["fn addr(x:u32)->u32{return x;}fn run()->u32{let x:u8=0;let p:*const u8=core::addr_of(x);let bits:usize=core::addr(p);return addr(7);}"]).unwrap();
}
#[test]
fn statics_validate_arrays_records_constants_and_permissions() {
    let c=compile(&["const N:u32=7;struct Pair{a:u32,b:[2]u8}static P:Pair=Pair{b:[1,2],a:N};static mut V:u32=0;fn run()->u32{unsafe {V=P.a;return V;}}"]).unwrap();
    let p = c.data.iter().find(|d| d.symbol == "__core_s_g0").unwrap();
    assert_eq!(&p.bytes[..6], &[7, 0, 0, 0, 1, 2]);
    reject("static A:[2]u8=[1];", "E_TYPE");
    reject("static A:[0]u8=[256;0];", "E_TYPE");
    reject("struct P{a:u32}static A:P=P{};", "E_TYPE");
    reject(
        "static A:[1]u8=[0];static P:*const u8=core::addr_of(A[0]);",
        "E_CONST",
    );
    reject(
        "static mut A:[1]u8=[0];fn run()->u32{let s:[]const u8=core::slice_of(A);return 0;}",
        "E_UNSAFE",
    );
    reject("extern abi(C) fn f(x:())->u32;", "E_TYPE");
}
#[test]
fn deferred_slice_checks_and_pointer_operations_remap() {
    let c=compile(&[r#"fn run()->u32{var a:[1]u8=[0];let s:[]u8=core::slice_of_mut(a);defer {unsafe {s[0]=2;let p:*u8=core::ptr_add(s.data,0);*p=3;}}return 0;}"#]).unwrap();
    for f in &c.functions {
        for block in &f.blocks {
            for i in &block.instructions {
                if let InstructionKind::BoundsDynamic { index, length } = &i.kind {
                    for op in [index, length] {
                        if let Operand::Slot(s) = op {
                            assert!((s.0 as usize) < f.slots.len());
                        }
                    }
                }
            }
        }
    }
}
