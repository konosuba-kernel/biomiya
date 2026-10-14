#![cfg(feature = "pipeline")]

use biomiya_core_s::{model::*, pipeline::compile, source::FileId};

fn reject(source: &str, code: &str) {
    let failure = compile(FileId(17), source).unwrap_err();
    assert_eq!(failure.code, code, "{}", failure.message);
    assert_eq!(failure.span.file, FileId(17));
    assert!(failure.span.start <= failure.span.end);
    assert!(failure.span.end as usize <= source.len());
}

#[test]
fn real_source_resolves_forward_constants_layouts_and_calls() {
    let compiled = compile(
        FileId(3),
        r#"
        struct Header { bytes:[COUNT]u8 }
        const COUNT:usize = 4;
        fn run() -> u32 {
            let h:Header = Header { bytes:[7; COUNT] };
            return add(h.bytes[0] as u32, sizeof(Header) as u32);
        }
        fn add(a:u32, b:u32) -> u32 { return a + b; }
    "#,
    )
    .unwrap();
    assert_eq!(compiled.functions.len(), 2);
    assert_eq!(compiled.functions[0].name, "run");
    let header = compiled.types.names["Header"];
    assert_eq!(compiled.types.entries[header.0 as usize].layout.size, 4);
    assert!(
        compiled.functions[0]
            .blocks
            .iter()
            .flat_map(|b| &b.instructions)
            .any(|i| matches!(
                i.kind,
                InstructionKind::Call {
                    function: FunctionId(1),
                    ..
                }
            ))
    );
}

#[test]
fn source_snapshots_cleanup_and_raw_access_reach_the_checker() {
    compile(
        FileId(0),
        r#"
        struct Pair { a:u32, b:u32 }
        unsafe fn change(p:*kernel Pair) -> u32 { unsafe { (*p).a = 99; } return 2; }
        fn take(p:Pair, other:u32) -> u32 { return p.a + other; }
        fn run() -> u32 {
            var pair:Pair = Pair { a:40, b:0 };
            var result:u32 = 0;
            unsafe { result = take(pair, change(addr_of_mut(pair))); }
            defer { result = 0; }
            return result;
        }
    "#,
    )
    .unwrap();
}

#[test]
fn source_result_construction_propagation_and_match_compose() {
    compile(
        FileId(0),
        r#"
        fn fail() -> Result<u32,u32> { return Result::<u32,u32>::Err(23); }
        fn forward() -> Result<u32,u32> {
            let value:u32 = fail()?;
            return Result::<u32,u32>::Ok(value);
        }
        fn run() -> u32 {
            let value:Result<u32,u32> = forward();
            match value {
                Result::<u32,u32>::Ok(n:u32) => { return n; },
                Result::<u32,u32>::Err(e:u32) => { return e; },
            }
        }
    "#,
    )
    .unwrap();
}

#[test]
fn failures_preserve_stage_and_source_identity() {
    reject("fn run() -> u32 { return ; }", "E_PARSE");
    reject("fn run() -> u32 { return missing; }", "E_NAME");
    reject("fn run() -> u32 { return true; }", "E_TYPE");
    reject(
        "fn run() -> u32 { let x:u32 = 1; if 0 == x { return 0; } return 1; }",
        "E_TYPE",
    );
    reject("fn run() -> u32 { return 1; return missing; }", "E_NAME");
    reject(
        "struct Recursive { next:Recursive } fn run() -> u32 { return 0; }",
        "E_LAYOUT",
    );
    reject("const A:u32 = B; const B:u32 = A;", "E_CONST");
}

#[test]
fn unsupported_features_are_distinct_from_language_errors() {
    reject(
        "enum Unsupported { } fn run() -> u32 { return 0; }",
        "I_UNSUPPORTED",
    );
    reject("fn run() -> u64 { return 0; }", "I_UNSUPPORTED");
    reject(
        "fn run() -> u32 { defer { return 1; } return 0; }",
        "E_TYPE",
    );
    reject(
        "fn run() -> u32 { let x:u32 = 1; let p:*kernel const u32 = addr_of(x); return *p; }",
        "E_UNSAFE",
    );
}

#[test]
fn source_capacity_is_a_resource_diagnostic() {
    let source = " ".repeat(biomiya_core_s::pipeline::MAX_SOURCE_BYTES + 1);
    reject(&source, "I_LIMIT");
}
