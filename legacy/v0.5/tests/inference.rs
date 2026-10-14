use biomiya::{check, source::FileId, syntax};
fn check_source(source: &str) -> biomiya::diagnostic::Result<biomiya::hir::Program> {
    syntax::parse(FileId(0), source).and_then(|ast| check::check(&[ast]))
}
#[test]
fn structural_constraints_precede_literal_defaults() {
    for source in [
        "fn test(){let p=if true{(1_u8,7)}else{(2,3_u8)};let q:(u8,u8)=p;}",
        "fn test(){let a:[0]u32=[];let p=if true{([],1_u8)}else{(a,2)};let q:([0]u32,u8)=p;}",
        "fn test(){let x=match true{true=>Some((1_u8,7)),false=>Some((2,3_u8))};let y:Option<(u8,u8)>=x;}",
        "fn test(){let x=loop{if true{break (1_u8,7);}break (2,3_u8);};let y:(u8,u8)=x;}",
        "fn id<T>(x:T,y:T)->T{x}fn test(){let x=id((1_u8,7),(2,3_u8));let y:(u8,u8)=x;}",
        "struct S{len:u8}fn test(s:S){let x=s.len+1;let y:u8=x;}",
        "struct S{}impl S{fn pair<T>(&self,x:T,y:T)->T{x}}fn test(){let s=S{};let x=s.pair((1_u8,7),(2,3_u8));let y:(u8,u8)=x;}",
        "fn test(){var a=[0];a[4]={return;};}",
        "fn test(){let x=if true{(1,{let n=1;n})}else{(2_u8,3_i32)};let y:(u8,i32)=x;}",
    ] {
        assert!(
            check_source(source).is_ok(),
            "{source}: {:?}",
            check_source(source).err()
        );
    }
}
#[test]
fn inferred_constraints_are_strict_and_prior_locals_are_hard() {
    for source in [
        "fn test(){let x=1;let y:u8=x;}",
        "fn test(){var x=1;let p=if true{&mut x}else{&x};}",
        "fn id<T>(x:T,y:T)->T{x}fn test(){var x=1;var y=2;let p=id(&mut x,&y);}",
    ] {
        assert_eq!(check_source(source).unwrap_err().code, "E_TYPE", "{source}");
    }
    for source in [
        "fn test(){var x=1;let p:&i32=if true{&mut x}else{&x};}",
        "fn id<T>(x:T,y:T)->T{x}fn test(){var x=1;var y=2;let p=id::<&i32>(&mut x,&y);}",
    ] {
        assert!(
            check_source(source).is_ok(),
            "{source}: {:?}",
            check_source(source).err()
        );
    }
}

#[test]
fn constant_branch_prefixes_keep_independent_bindings_and_soft_tail_peers() {
    for source in [
        "const C:usize=if true{let n=3_usize;let a=[7_u8;n];a.len}else{0_usize};static_assert(C==3_usize);",
        "const C:usize=match true{true=>{let n=3_usize;let a=[7_u8;n];a.len},false=>0_usize};static_assert(C==3_usize);",
        "const C:usize={let p=if true{let n=3_usize;let a=[7_u8;n];(1_u8,7)}else{(2,3_u8)};p.0 as usize+p.1 as usize};static_assert(C==8_usize);",
    ] {
        assert!(
            check_source(source).is_ok(),
            "{source}: {:?}",
            check_source(source).err()
        );
    }
}
