#[path = "../../../../core-s/src/project.rs"]
mod project;
fn input() -> String { "format 1\r\nprofile linux-user\r\nentry root::app::run\r\nsource α/main.vt\r\noutput out/image\r\n".to_owned() }
#[test]
fn independent_format_and_boundaries() {
    let m = project::parse(&input()).unwrap();
    assert_eq!(m.sources, ["α/main.vt"]);
    assert_eq!(m.entry, "root::app::run");
    let p96 = "源".repeat(32);
    assert!(project::parse(&(input() + &format!("source {p96}\n"))).is_ok());
    assert!(project::parse(&(input() + &format!("source {p96}x\n"))).is_err());
    assert!(project::parse(&(input() + "assembly α/main.vt\n")).unwrap_err().contains("line 6"));
}
#[test]
fn independent_directive_cross_product() {
    for key in ["format", "profile", "entry", "source", "assembly", "output"] {
        for suffix in ["", " one two", " one # trailing"] {
            assert!(project::parse(&(input() + &format!("{key}{suffix}\n"))).is_err(), "{key}{suffix}");
        }
    }
    for bad in ["/root", "./root", "root/..", "root//file", "root/\u{7f}file", "root\u{2003}file", "root\\file"] {
        for key in ["source", "assembly"] {
            assert!(project::parse(&(input() + &format!("{key} {bad}\n"))).is_err(), "{key} {bad:?}");
        }
        assert!(project::parse(&input().replace("out/image", bad)).is_err());
    }
    assert!(project::parse(&input().replace("format 1", "format 01")).is_err());
    assert!(project::parse(&input().replace("profile linux-user", "profile sos")).unwrap_err().contains("I_UNSUPPORTED"));
}
