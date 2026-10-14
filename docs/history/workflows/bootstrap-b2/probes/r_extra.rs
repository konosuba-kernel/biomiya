#[test]
fn review_partial_output_cleanup_and_existing_symlink_protection() {
    let t = Scratch::new();
    prepare(&t, &manifest(&["first.vt", "missing.vt"]), &[("first.vt", b"fn first()->u32{return 1;}")]);
    assert_eq!(execute(&t, copy_image(), &["project"]).status.code(), Some(12));
    assert!(!t.file("copied.vt").exists());
    #[cfg(unix)] {
        std::os::unix::fs::symlink("absent-target", t.file("copied.vt")).unwrap();
        assert_eq!(execute(&t, copy_image(), &["project"]).status.code(), Some(12));
        assert_eq!(fs::read_link(t.file("copied.vt")).unwrap(), PathBuf::from("absent-target"));
        assert!(!t.file("absent-target").exists());
    }
}
#[test]
fn review_output_close_error_is_reported_and_new_file_removed() {
    let shim = "__b2_syscall6:\naddi x5,x0,57\nbne x10,x5,__b2_real\naddi x5,x0,3\nbne x11,x5,__b2_real\naddress x5,__review_count\nlbu x6,0(x5)\naddi x6,x6,1\nsb x6,0(x5)\naddi x5,x0,2\nbne x6,x5,__b2_real\naddi x17,x0,57\naddi x10,x11,0\necall\naddi x10,x0,-5\njalr x0,0(x1)\n".to_owned() + &provider().replace("__b2_syscall6:", "__b2_real:");
    let mut p = program(None, "app::start", &shim);
    p.data.push(DataObject { symbol:"__review_count".into(), mutable:true, align:1, bytes:vec![0], zero_fill:0, relocations:vec![] });
    let t=Scratch::new();prepare(&t,&manifest(&["first.vt"]),&[("first.vt",b"// written before final close\n")]);
    assert_eq!(execute(&t,&image::executable(&p,"_start").unwrap(),&["project"]).status.code(),Some(13));
    assert!(!t.file("copied.vt").exists());
}
#[test]
fn review_manifest_order_limits_and_nonascii_comment_string() {
    let t=Scratch::new();
    let mut m=manifest(&["one.vt","two.vt"]);
    m.push_str("assembly provider.s0\n");
    prepare(&t,&m,&[("one.vt","/* 嵌套 /* 注释 */ */\n".as_bytes()),("two.vt","static s:[]const u8=\"字节\\xFF\";\n".as_bytes())]);
    assert_eq!(execute(&t,copy_image(),&["project"]).status.code(),Some(0));
    assert_eq!(fs::read(t.file("copied.vt")).unwrap(),[fs::read(t.file("one.vt")).unwrap(),fs::read(t.file("two.vt")).unwrap()].concat());
    fs::remove_file(t.file("copied.vt")).unwrap();
    fs::write(t.file("project"),manifest(&["one.vt"])+"assembly one.vt\n").unwrap();
    assert_eq!(execute(&t,copy_image(),&["project"]).status.code(),Some(20));
    assert!(!t.file("copied.vt").exists());
}
