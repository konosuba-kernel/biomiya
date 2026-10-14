use biomiya_core_s::{encode, machine::*, s0, source::FileId};

#[test]
fn fixed_grammar_encodes_all_operations_and_localizes_labels() {
    let source = "# platform adapter\nentry:\n.Lagain:\nadd x1,x2,x3\nsub x1,x2,x3\nmul x1,x2,x3\nmulhu x1,x2,x3\ndivu x1,x2,x3\nremu x1,x2,x3\nand x1,x2,x3\nor x1,x2,x3\nxor x1,x2,x3\nsltu x1,x2,x3\nsll x1,x2,x3\nsrl x1,x2,x3\naddi x1,x2,-0x800\naddiw x1,x2,2047\nxori x1,x2,-1\nsltiu x1,x2,0\nslli x1,x2,63\nsrli x1,x2,0\nlbu x1,0(x2)\nlwu x1,-2048(x2)\nld x1,2047(x2)\nsb x1,0(x2)\nsw x1,0(x2)\nsd x1,0(x2)\nbeq x1,x2,.Lagain\nbne x1,x2,entry\nbltu x1,x2,entry\nbgeu x1,x2,entry\naddress x5,entry\ncall entry # comment\njump .Lagain\njalr x0,0(x1)\necall\nunimp\n";
    let program = s0::parse(FileId(7), source).unwrap();
    assert_eq!(program.instructions.len(), 36);
    assert_eq!(program.instructions[1], Inst::Label(".Lfile7_again".into()));
    assert!(program.instructions.contains(&Inst::Jump {
        rd: ZERO,
        target: ".Lfile7_again".into()
    }));
    assert!(program.data.is_empty());
    encode::encode(&program, 0x10000).unwrap();
    let other = s0::parse(FileId(8), ".Lagain:\nunimp").unwrap();
    assert_ne!(program.instructions[1], other.instructions[0]);
}

#[test]
fn malformed_and_unsupported_inputs_have_file_and_line_spans() {
    for line in [
        "add x32,x1,x2",
        "add a0,x1,x2",
        "addi x1,x2,2048",
        "addi x1,x2,-2049",
        "slli x1,x2,64",
        "srli x1,x2,-1",
        "ld x1,0xg(x2)",
        "ld x1,(x2)",
        "ld x1,0(x2",
        "call x y",
        "add x1,x2",
        "ecall x0",
        "address x0,entry",
        "entry: ecall",
        "é:",
        "call foo+8",
        "addi x1,x2,0xAB",
        "addi x1,x2,+1",
        "jalr x0,9999(x1)",
        "call",
        "x:\nx:",
    ] {
        let source = format!("# first\n{line}\n");
        let error = s0::parse(FileId(9), &source).unwrap_err();
        assert_eq!(error.span.file, FileId(9), "{line}");
        assert!(error.span.start >= 8, "{line}");
        assert!(error.span.end <= source.len() as u32);
    }
    for line in [
        "li x1,0",
        "la x1,foo",
        ".word 1",
        ".include foo",
        "csrrw x1,0,x2",
        "amoadd.d x1,x2,(x3)",
    ] {
        assert_eq!(
            s0::parse(FileId(0), line).unwrap_err().code,
            "I_UNSUPPORTED",
            "{line}"
        );
    }
}
