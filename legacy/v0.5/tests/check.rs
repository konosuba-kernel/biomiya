use biomiya::{check, source::FileId, syntax};
#[test]
fn conformance_frontend() {
    let mut failures = vec![];
    for entry in std::fs::read_dir("tests/conformance").unwrap() {
        let p = entry.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) != Some("vt") {
            continue;
        }
        let name = p.file_name().unwrap().to_str().unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        let result = syntax::parse(FileId(0), &s).and_then(|f| check::check(&[f]));
        let reject = name.starts_with("reject_");
        if result.is_err() != reject {
            failures.push(format!("{name}: {:?}", result.err()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn diagnostic_categories() {
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string("tests/conformance/manifest.json").unwrap())
            .unwrap();
    let mut failures = vec![];
    for case in manifest["cases"].as_array().unwrap() {
        if case["kind"] != "reject" {
            continue;
        }
        let name = case["file"].as_str().unwrap();
        let source = std::fs::read_to_string(format!("tests/conformance/{name}")).unwrap();
        let error = syntax::parse(FileId(0), &source)
            .and_then(|f| check::check(&[f]))
            .err()
            .unwrap();
        if error.code != case["category"].as_str().unwrap() {
            failures.push(format!(
                "{name}: {} expected {}",
                error.code, case["category"]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
fn source_check(s: &str) -> biomiya::diagnostic::Result<biomiya::hir::Program> {
    syntax::parse(FileId(0), s).and_then(|f| check::check(&[f]))
}
#[test]
fn unused_generic_independent_checks() {
    for s in [
        "fn f<T>(v:T){let b:bool=1;}",
        "fn f<T>(v:T)->T{{let b:bool=1;v}}",
        "fn f<T>(v:T){v.missing({let b:bool=1;});}",
        "fn f<T>(v:T){unknown();}",
    ] {
        assert!(source_check(s).is_err(), "{s}")
    }
    for s in ["fn f<T>(v:T)->i32{v.missing()}", "fn f<T>(v:T)->T{v+1}"] {
        assert!(source_check(s).is_ok(), "{s}: {:?}", source_check(s).err())
    }
}
#[test]
fn borrowed_match_and_extended_constants() {
    for s in [
        "struct S{x:i32}fn f(s:&mut S){match s{S{x:v}=>{*v=3;}}}",
        "static A:Atomic<u32>=Atomic::new(3);",
        "static U:MaybeUninit<[3]u32>=MaybeUninit::uninit();",
        "static S:[]const u8=\"hello\";",
        "const X:i32={let a=2;let b=3;a+b};static_assert(X==5);",
        "const X:i32=match Some(true){Some(true)=>7,Some(false)=>8,None=>9};static_assert(X==7);",
        "const X:i32=(-1_i8) as i32;static_assert(X == -1);",
    ] {
        assert!(source_check(s).is_ok(), "{s}: {:?}", source_check(s).err())
    }
}
#[test]
fn dependent_constant_blocks() {
    for s in [
        "struct Buffer<T>{bytes:[{let n=sizeof(T);n}]u8} static_assert(sizeof(Buffer<u32>)==4);",
        "struct Buffer<T>{bytes:[{let (n,m)=(sizeof(T),2_usize);n+m}]u8} static_assert(sizeof(Buffer<u32>)==6);",
        "struct Buffer<T>{bytes:[{let pair=(sizeof(T),2_usize);pair.0+pair.1}]u8} static_assert(sizeof(Buffer<u32>)==6);",
        "struct Buffer<T>{bytes:[{let n=sizeof(T);static_assert(n==4_usize);n}]u8} static_assert(sizeof(Buffer<u32>)==4);",
        "struct Buffer<T>{bytes:[match sizeof(T){0_usize=>1_usize,n=>n}]u8} static_assert(sizeof(Buffer<u32>)==4);",
        "struct Buffer<T>{bytes:[match Some(sizeof(T)){Some(n)=>n,None=>0_usize}]u8} static_assert(sizeof(Buffer<u32>)==4);",
        "struct Buffer<T>{bytes:[if sizeof(T)==4_usize{4_usize}else{panic(\"wrong\")}]u8} static_assert(sizeof(Buffer<u32>)==4);",
        "struct Buffer<T>{bytes:[1+sizeof(T)]u8} static_assert(sizeof(Buffer<u32>)==5);",
    ] {
        assert!(source_check(s).is_ok(), "{s}: {:?}", source_check(s).err());
    }
    for s in [
        "struct Buffer<T>{bytes:[{let n=sizeof(T);static_assert(n==4_usize);n}]u8} static_assert(sizeof(Buffer<u64>)==8);",
        "struct Buffer<T>{bytes:[{let n=1/0;sizeof(T)}]u8} static_assert(sizeof(Buffer<u32>)==4);",
        "struct Buffer<T>{bytes:[match sizeof(T){0_usize=>1_usize}]u8}",
    ] {
        assert!(source_check(s).is_err(), "{s}")
    }
}
#[test]
fn finite_initializer_resources() {
    assert!(
        source_check("static Z:[18446744073709551615_usize]()=[();18446744073709551615_usize];")
            .is_ok()
    );
    let error = source_check("static X:[67108865_usize]u8=[0;67108865_usize];").unwrap_err();
    assert_eq!(error.code, "I_LIMIT");
}
#[test]
fn function_pointer_fields() {
    for s in [
        "fn source(){} fn test()->i32{let pair:(fn(),i32)=(source,1);(pair.0)();7}",
        "struct S{callback:fn()}fn source(){}fn test()->i32{let s=S{callback:source};s.callback();7}",
        "struct S{f:fn()->i32}impl S{fn f(&self)->i32{2}}fn source()->i32{1}fn test()->i32{let s=S{f:source};(s.f)()+s.f()}",
    ] {
        assert!(source_check(s).is_ok(), "{s}: {:?}", source_check(s).err())
    }
}
#[test]
fn defer_internal_transfers() {
    assert!(source_check("fn f(){defer {var i=0;loop {i+=1;if i<2{continue;}break;}}}").is_ok());
    assert!(source_check("fn f(){loop {defer {break;}break;}}").is_err());
    assert!(source_check("fn f(){loop {defer {continue;}break;}}").is_err());
}

#[test]
fn pointer_space_syntax() {
    for space in ["()", "kernel::Bogus", "kernel<u8>"] {
        let s = format!("fn f(){{unsafe{{let p=ptr::<u32, {space}>(0);}}}}");
        assert_eq!(source_check(&s).unwrap_err().code, "E_TYPE");
    }
}
#[test]
fn symbolic_local_constructor_match() {
    assert!(source_check("enum E{A(usize),B(usize)}struct Buffer<T>{bytes:[{let v=E::A(sizeof(T));match v{E::A(x)=>x,E::B(y)=>y}}]u8}static_assert(sizeof(Buffer<u32>)==4);").is_ok());
}

#[test]
fn block_expression_full_contexts() {
    for s in [
        "fn f()->i32{let v=if true{1}else{2}+3;v}",
        "fn f()->i32{(if true{1}else{2})+3}",
        "fn f()->i32{if true{1}else{2}+3}",
        "fn source()->i32{3}fn f()->i32{{source}()}",
    ] {
        assert!(source_check(s).is_ok(), "{s}: {:?}", source_check(s).err());
    }
}

#[test]
fn symbolic_choices_compose() {
    for s in [
        "struct B<T>{b:[{let r:Result<usize,usize>=Ok(sizeof(T));match r{Ok(n)=>n,Err(e)=>e}}]u8}static_assert(sizeof(B<u32>)==4);",
        "struct B<T>{b:[{let o:Option<usize>=None;match o{Some(n)=>n,None=>sizeof(T)}}]u8}static_assert(sizeof(B<u32>)==4);",
        "struct B<T>{b:[{let p=if sizeof(T)==4_usize{(4_usize,Some(2_usize))}else{(8_usize,None)};let(n,v)=p;let out=match v{Some(k)=>(n,k),None=>(n,0_usize)};out.0+out.1}]u8}static_assert(sizeof(B<u32>)==6);static_assert(sizeof(B<u64>)==8);",
        "struct B<T>{b:[{let a=if sizeof(T)==4_usize{[4_usize,2_usize]}else{[8_usize,0_usize]};a[0]+a[1]}]u8}static_assert(sizeof(B<u32>)==6);static_assert(sizeof(B<u64>)==8);",
        "enum E<T>{A(T),B(T)}struct B<T>{b:[{let v=E::A(sizeof(T));match v{E::A(n)=>n,E::B(n)=>n}}]u8}static_assert(sizeof(B<u32>)==4);",
    ] {
        assert!(source_check(s).is_ok(), "{s}: {:?}", source_check(s).err());
    }
}

#[test]
fn symbolic_unused_allowed_values() {
    for s in [
        "fn source(){}struct B<T>{b:[{let f=source;let a=Atomic::<u32>::new(0);let c=UnsafeCell::<usize>::new(sizeof(T));let u=MaybeUninit::<T>::uninit();let p:*u32=null;sizeof(T)}]u8}static_assert(sizeof(B<u32>)==4);",
        "struct B<T>{b:[{let a=Atomic::<u32>::new(1_u32/0_u32);sizeof(T)}]u8}static_assert(sizeof(B<u32>)==4);",
    ] {
        let r = source_check(s);
        if s.contains("1_u32/0_u32") {
            assert_eq!(r.unwrap_err().code, "E_CONST")
        } else {
            assert!(r.is_ok(), "{:?}", r.err())
        }
    }
}

#[test]
fn symbolic_builtin_identity() {
    for name in ["Some", "panic"] {
        let s = format!(
            "fn {name}(n:usize)->usize{{n}}struct B<T>{{b:[{{let unused={name}(sizeof(T));sizeof(T)}}]u8}}"
        );
        assert_eq!(source_check(&s).unwrap_err().code, "E_CONST");
    }
}

#[test]
fn constant_index_checks_dead_operands() {
    assert_eq!(
        source_check("fn ordinary()->usize{0}const X:i32=if true{1}else{[1,2][ordinary()]};")
            .unwrap_err()
            .code,
        "E_CONST"
    );
    assert_eq!(
        source_check(
            "struct S{x:i32,a:Atomic<u32>}const X:i32={let s=S{x:1,a:Atomic::<u32>::new(0)};s.x};"
        )
        .unwrap_err()
        .code,
        "E_CONST"
    );
}

#[test]
fn constant_prefix_queries_are_lazy_and_scoped() {
    for source in [
        "static_assert({let n=3_usize;let a=[7_u8;n];a.len==3_usize});fn test(){static_assert({let n=2_usize;let a=[7_u8;n];a.len==2_usize});}",
        "const C:usize={let n=3_usize;static_assert(n==3_usize);let a=[7_u8;n];a.len};static_assert(C==3_usize);",
        "const C:usize={let (n,m)=if true{(3_usize,4_usize)}else{(5_usize,6_usize)};static_assert(n==3_usize);let a=[0_u8;m];a.len};static_assert(C==4_usize);",
        "const C:usize=if true{4_usize}else{let n=1_usize/0_usize;static_assert(n==0_usize);n};static_assert(C==4_usize);",
    ] {
        assert!(
            source_check(source).is_ok(),
            "{source}: {:?}",
            source_check(source).err()
        );
    }
    assert_eq!(
        source_check("fn test(){let n=3_usize;let a=[7_u8;n];}")
            .unwrap_err()
            .code,
        "E_CONST"
    );
    assert_eq!(
        source_check("const N:usize={1_usize;2_usize};")
            .unwrap_err()
            .code,
        "E_CONST"
    );
}

#[test]
fn dependent_length_roots_replay_all_operand_forms() {
    for source in [
        "struct B<T>{x:[{let n:T=2 as T;sizeof([n+n]u8)}]u8}static_assert(sizeof(B<u8>)==4_usize);",
        "struct B<T>{x:[{let n:T=2 as T;sizeof([~n]u8)}]u8}static_assert(sizeof(B<u8>)==253_usize);",
        "struct B<T>{x:[{let n:T=(2_usize,3_usize);sizeof([n.0+n.1]u8)}]u8}static_assert(sizeof(B<(usize,usize)>)==5_usize);",
        "struct B<T>{x:[{let n:T=[2_usize,3_usize];sizeof([n[0]]u8)}]u8}static_assert(sizeof(B<[2]usize>)==2_usize);",
    ] {
        assert!(
            source_check(source).is_ok(),
            "{source}: {:?}",
            source_check(source).err()
        );
    }
    for source in [
        "struct B<T>{x:[{let n:T=2 as T;sizeof([true+n]u8)}]u8}",
        "fn ordinary<T>(n:T)->usize{4_usize}struct B<T>{x:[{let n:T=2 as T;sizeof([n+ordinary(n)]u8)}]u8}",
    ] {
        assert!(source_check(source).is_err(), "{source}");
    }
}

#[test]
fn atomic_compare_exchange_field_receiver_result_patterns() {
    let source = "struct S{value:Atomic<u8>} fn test()->u8{let s=S{value:Atomic::<u8>::new(17_u8)};match s.value.compare_exchange(18_u8,42_u8,SeqCst,Acquire){Err(old)=>old,Ok(old)=>old}}";
    let program = source_check(source).unwrap();
    let function = program.functions.iter().find(|f| f.name == "test").unwrap();
    assert!(matches!(
        program.types.kind(function.signature.result),
        biomiya::types::TypeKind::Int(biomiya::types::Integer::U8)
    ));
    assert!(source_check(include_str!("integration/atomic_widths.vt")).is_ok());
    let nested = "struct S{value:Atomic<u8>}struct Outer{inner:S}fn test()->u8{let outer=Outer{inner:S{value:Atomic::<u8>::new(17_u8)}};let r=&outer;match r.inner.value.compare_exchange(18_u8,42_u8,SeqCst,Acquire){Err(old)=>old,Ok(old)=>old}}";
    assert!(source_check(nested).is_ok());
    let method = "struct S{value:u8}impl S{fn get(&self)->u8{self.value}}struct Outer{inner:S}fn test()->u8{let outer=Outer{inner:S{value:17_u8}};let r=&outer;r.inner.get()}";
    assert!(source_check(method).is_ok());
    let invalid = source.replace("Err(old)=>old", "Err(old)=>old+1_u16");
    assert_eq!(source_check(&invalid).unwrap_err().code, "E_TYPE");
}
