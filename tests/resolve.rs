use biomiya::resolve::{Namespace, Resolver};
use biomiya::{source, syntax};
use source::{FileId, Span};
fn files(s: &[&str]) -> Vec<syntax::AstFile> {
    s.iter()
        .enumerate()
        .map(|(i, s)| syntax::parse(FileId(i as u32), s).unwrap())
        .collect()
}
fn names(s: &str) -> Vec<String> {
    s.split("::").map(str::to_string).collect()
}
#[test]
fn modules_forward_imports_visibility() {
    let f = files(&[
        "module a; import b::{T,f}; fn g(x:T)->i32{f(x)}",
        "module b; pub struct T{x:i32} pub fn f(x:T)->i32{x.x} fn private(){}",
    ]);
    let r = Resolver::new(&f).unwrap();
    assert!(
        r.lookup(&names("a"), &names("T"), Namespace::Type, Span::default())
            .unwrap()
            .is_some()
    );
    assert!(
        r.lookup(
            &names("a"),
            &names("b::private"),
            Namespace::Value,
            Span::default()
        )
        .is_err()
    );
    assert!(
        r.lookup(
            &names("b"),
            &names("a::f"),
            Namespace::Value,
            Span::default()
        )
        .is_err()
    );
}
#[test]
fn namespaces_and_module_merge() {
    let f = files(&[
        "module a; type X=i32; fn X()->i32{1}",
        "module a; const Y:i32=2;",
    ]);
    let r = Resolver::new(&f).unwrap();
    let x = names("X");
    assert_ne!(
        r.lookup(&names("a"), &x, Namespace::Type, Span::default())
            .unwrap(),
        r.lookup(&names("a"), &x, Namespace::Value, Span::default())
            .unwrap()
    );
    assert_eq!(r.declarations.len(), 3);
}
#[test]
fn duplicate_and_unknown_imports() {
    for s in [
        &["fn x(){} fn x(){}"] as &[&str],
        &["module a; type b=i32;", "module a::b;"],
        &["import core::missing;"],
        &["module a; fn f(){}", "module b; import a::f;"],
        &["import core::panic; import core::panic;"],
    ] {
        assert!(Resolver::new(&files(s)).is_err(), "{s:?}")
    }
}
#[test]
fn builtins_and_impl_context() {
    let r = Resolver::new(&files(&[
        "import core::panic; struct S<T>{x:T} impl<T> S<T>{fn get(&self)->T{self.x}}",
    ]))
    .unwrap();
    assert_eq!(
        r.builtin_path(&[], &names("panic"), Namespace::Value, Span::default())
            .unwrap(),
        Some(names("core::panic"))
    );
    let m = r
        .declarations
        .values()
        .find(|r| r.decl.name == "get")
        .unwrap();
    assert!(m.impl_owner.is_some());
    assert_eq!(m.impl_generics[0].name, "T");
    assert_eq!(r.methods.len(), 1);
}
#[test]
fn variants_and_cycles() {
    let r = Resolver::new(&files(&[
        "module a; import b; pub enum E{X,Y}",
        "module b; import a; fn f(){}",
    ]))
    .unwrap();
    assert!(
        r.lookup(
            &names("b"),
            &names("a::E::X"),
            Namespace::Value,
            Span::default()
        )
        .unwrap()
        .is_some()
    );
}
