//! Bounded, line-oriented project manifest. Paths are resolved by the caller.
use std::collections::BTreeSet;

pub const MAX_MANIFEST: usize = 16 * 1024;
pub const MAX_PATH: usize = 96;
pub const MAX_SOURCES: usize = 32;
pub const MAX_ASSEMBLIES: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub entry: String,
    pub sources: Vec<String>,
    pub assemblies: Vec<String>,
    pub output: String,
}

fn path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PATH
        && !value.contains('\\')
        && !value.chars().any(|c| c.is_whitespace() || c.is_control())
        && value
            .split('/')
            .all(|part| !matches!(part, "" | "." | ".."))
}

pub fn parse(text: &str) -> Result<Manifest, String> {
    let error = |line: usize, message: &str| format!("manifest line {line}: {message}");
    if text.len() > MAX_MANIFEST {
        return Err(error(1, "manifest exceeds 16384 bytes"));
    }
    let mut seen = BTreeSet::new();
    let mut inputs = BTreeSet::new();
    let mut result = Manifest {
        entry: String::new(),
        sources: Vec::new(),
        assemblies: Vec::new(),
        output: String::new(),
    };
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let key = words.next().unwrap();
        let value = words
            .next()
            .ok_or_else(|| error(number, "expected one argument"))?;
        if words.next().is_some() {
            return Err(error(number, "expected one argument"));
        }
        if !matches!(
            key,
            "format" | "profile" | "entry" | "source" | "assembly" | "output"
        ) {
            return Err(error(number, "unknown directive"));
        }
        if !matches!(key, "source" | "assembly") && !seen.insert(key) {
            return Err(error(number, "duplicate directive"));
        }
        match key {
            "format" if value != "1" => {
                return Err(error(number, "unsupported format (expected 1)"))
            }
            "profile" if value != "linux-user" => {
                return Err(error(number, "I_UNSUPPORTED: profile"))
            }
            "entry" => result.entry = value.to_owned(),
            "source" | "assembly" | "output" => {
                if !path(value) {
                    return Err(error(
                        number,
                        "invalid relative path or path exceeds 96 bytes",
                    ));
                }
                if key == "output" {
                    result.output = value.to_owned();
                    continue;
                }
                if !inputs.insert(value) {
                    return Err(error(number, "duplicate input path"));
                }
                let (list, limit) = if key == "source" {
                    (&mut result.sources, MAX_SOURCES)
                } else {
                    (&mut result.assemblies, MAX_ASSEMBLIES)
                };
                if list.len() == limit {
                    return Err(error(number, "too many input paths"));
                }
                list.push(value.to_owned());
            }
            _ => {}
        }
    }
    let end = text.lines().count() + 1;
    for key in ["format", "profile", "entry", "output"] {
        if !seen.contains(key) {
            return Err(error(end, &format!("missing {key}")));
        }
    }
    if result.sources.is_empty() {
        return Err(error(end, "at least one source is required"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest(extra: &str) -> String {
        format!("format 1\nprofile linux-user\nentry app::run\nsource main.vt\noutput build/app\n{extra}")
    }
    #[test]
    fn preserves_input_order_and_unicode_paths() {
        let m = parse(&manifest(
            "# comment\nsource 源码/二.vt\nassembly runtime/linux.s0\n",
        ))
        .unwrap();
        assert_eq!(m.sources, ["main.vt", "源码/二.vt"]);
        assert_eq!(m.assemblies, ["runtime/linux.s0"]);
        assert_eq!(m.entry, "app::run");
        assert_eq!(m.output, "build/app");
    }
    #[test]
    fn rejects_ambiguous_and_escaping_paths() {
        for p in [
            "/etc/passwd",
            "a//b",
            "a/./b",
            "a/../b",
            "a/",
            "../a",
            "a\\b",
            "a b",
            "a\0b",
        ] {
            assert!(parse(&manifest(&format!("source {p}\n"))).is_err(), "{p:?}");
        }
        assert!(parse(&manifest(&format!("source {}\n", "a".repeat(96)))).is_ok());
        assert!(parse(&manifest(&format!("source {}\n", "a".repeat(97)))).is_err());
        assert!(parse(&manifest(&format!("source {}\n", "源".repeat(33)))).is_err());
    }
    #[test]
    fn rejects_duplicates_missing_fields_and_unknowns() {
        for extra in [
            "source main.vt",
            "assembly main.vt",
            "entry foo",
            "format 1",
            "profile linux-user",
            "output foo",
            "unknown bar",
            "source",
            "source a b",
        ] {
            let e = parse(&manifest(extra)).unwrap_err();
            assert!(e.contains("line 6"), "{extra}: {e}");
        }
        for field in ["format", "profile", "entry", "source", "output"] {
            let text = manifest("")
                .lines()
                .filter(|l| !l.starts_with(field))
                .collect::<Vec<_>>()
                .join("\n");
            assert!(parse(&text).is_err(), "{field}");
        }
        assert!(parse(&manifest("").replace("linux-user", "sos"))
            .unwrap_err()
            .contains("I_UNSUPPORTED"));
    }
    #[test]
    fn bounds_manifest_and_input_counts() {
        let mut text = manifest("");
        for i in 1..32 {
            text.push_str(&format!("source s{i}.vt\n"));
        }
        for i in 0..8 {
            text.push_str(&format!("assembly a{i}.s0\n"));
        }
        assert!(parse(&text).is_ok());
        assert!(parse(&(text.clone() + "source overflow.vt\n")).is_err());
        assert!(parse(&(text + "assembly overflow.s0\n")).is_err());
        let base = manifest("") + "#";
        let exact = base.clone() + &"x".repeat(MAX_MANIFEST - base.len());
        assert!(parse(&exact).is_ok());
        assert!(parse(&(exact + "x")).is_err());
    }
}
