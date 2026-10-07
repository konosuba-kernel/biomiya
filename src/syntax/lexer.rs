use crate::{
    diagnostic::{Diagnostic, Result},
    source::{FileId, Span},
};
#[derive(Clone, Debug)]
pub struct Token {
    pub text: String,
    pub bytes: Option<Vec<u8>>,
    pub span: Span,
}
fn xid(c: char, r: &[(u32, u32)]) -> bool {
    let n = c as u32;
    r.binary_search_by(|(a, b)| {
        if n < *a {
            std::cmp::Ordering::Greater
        } else if n > *b {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Equal
        }
    })
    .is_ok()
}
pub fn lex(file: FileId, s: &str) -> Result<Vec<Token>> {
    let mut out = vec![];
    let mut i = 0;
    while i < s.len() {
        let start = i;
        let c = s[i..].chars().next().unwrap();
        let err =
            |msg: &str| Diagnostic::new("E_LEX", Span::new(file, start, start + c.len_utf8()), msg);
        if matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c') {
            i += 1;
            continue;
        }
        if s[i..].starts_with("//") {
            i += 2;
            while i < s.len() && !matches!(s.as_bytes()[i], b'\n' | b'\r') {
                i += 1
            }
            continue;
        }
        if s[i..].starts_with("/*") {
            i += 2;
            let mut depth = 1;
            while i < s.len() && depth > 0 {
                if s[i..].starts_with("/*") {
                    depth += 1;
                    i += 2
                } else if s[i..].starts_with("*/") {
                    depth -= 1;
                    i += 2
                } else {
                    i += s[i..].chars().next().unwrap().len_utf8()
                }
            }
            if depth > 0 {
                return Err(err("unclosed block comment"));
            }
            continue;
        }
        let mut bytes = None;
        if c == '"' {
            i += 1;
            let mut b = vec![];
            let mut closed = false;
            while i < s.len() {
                let ch = s[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if ch == '"' {
                    closed = true;
                    break;
                }
                if ch == '\n' || ch == '\r' {
                    return Err(err("string crosses physical line"));
                }
                if ch == '\\' {
                    if i >= s.len() {
                        break;
                    }
                    let e = s.as_bytes()[i];
                    i += 1;
                    match e {
                        b'0' => b.push(0),
                        b'n' => b.push(10),
                        b'r' => b.push(13),
                        b't' => b.push(9),
                        b'\\' => b.push(92),
                        b'"' => b.push(34),
                        b'x' => {
                            if i + 2 > s.len() {
                                return Err(err("incomplete byte escape"));
                            }
                            let hex = s
                                .as_bytes()
                                .get(i..i + 2)
                                .ok_or_else(|| err("incomplete byte escape"))?;
                            if !hex.iter().all(u8::is_ascii_hexdigit) {
                                return Err(err("invalid byte escape"));
                            }
                            let v = u8::from_str_radix(std::str::from_utf8(hex).unwrap(), 16)
                                .map_err(|_| err("invalid byte escape"))?;
                            i += 2;
                            b.push(v)
                        }
                        _ => return Err(err("unknown escape")),
                    }
                } else {
                    let mut tmp = [0; 4];
                    b.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes())
                }
            }
            if !closed {
                return Err(err("unclosed string"));
            }
            bytes = Some(b)
        } else if c.is_ascii_digit() {
            i += 1;
            while i < s.len() {
                let ch = s[i..].chars().next().unwrap();
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    i += 1
                } else {
                    break;
                }
            }
            let text = &s[start..i];
            let mut number = text;
            for suffix in [
                "_usize", "_isize", "_u8", "_u16", "_u32", "_u64", "_i8", "_i16", "_i32", "_i64",
            ] {
                if let Some(n) = text.strip_suffix(suffix) {
                    number = n;
                    break;
                }
            }
            let (radix, digits) = if let Some(n) = number.strip_prefix("0x") {
                (16, n)
            } else if let Some(n) = number.strip_prefix("0b") {
                (2, n)
            } else {
                (10, number)
            };
            if digits.is_empty()
                || digits.starts_with('_')
                || digits.ends_with('_')
                || !digits.chars().all(|x| x == '_' || x.is_digit(radix))
            {
                return Err(err("invalid integer literal"));
            }
        } else if c == '_' || xid(c, super::unicode::XID_START) {
            i += c.len_utf8();
            while i < s.len() {
                let ch = s[i..].chars().next().unwrap();
                if xid(ch, super::unicode::XID_CONTINUE) {
                    i += ch.len_utf8()
                } else {
                    break;
                }
            }
        } else {
            let mut found = false;
            for op in [
                "<<=", ">>=", "+%=", "-%=", "*%=", "..=", "::", "->", "=>", "..", "+?", "-?", "*?",
                "+%", "-%", "*%", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "+=", "-=", "*=",
                "/=", "%=", "&=", "|=", "^=",
            ] {
                if s[i..].starts_with(op) {
                    i += op.len();
                    found = true;
                    break;
                }
            }
            if !found {
                if "()[]{};:,.@+-*/%&|^!~=<>?".contains(c) {
                    i += 1
                } else {
                    return Err(err("invalid source character"));
                }
            }
        }
        out.push(Token {
            text: s[start..i].into(),
            bytes,
            span: Span::new(file, start, i),
        });
    }
    out.push(Token {
        text: String::new(),
        bytes: None,
        span: Span::new(file, i, i),
    });
    Ok(out)
}
