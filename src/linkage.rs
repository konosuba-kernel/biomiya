//! Byte-preserving external names and restoration of assembler-safe ELF aliases.
//!
//! Source attributes are byte strings. The NUL-prefixed hex form is an internal,
//! reversible identity for invalid UTF-8; source names themselves cannot contain NUL.
use crate::{
    diagnostic::{Diagnostic, Result},
    source::{FileId, Span},
};
use std::{borrow::Cow, collections::BTreeMap};
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NameMapping {
    pub alias: String,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LinkageNames {
    pub symbols: Vec<NameMapping>,
    pub sections: Vec<NameMapping>,
}
impl LinkageNames {
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty() && self.sections.is_empty()
    }
}
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
}
pub fn encode_name(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => format!("\0{}", hex(bytes)),
    }
}
pub fn name_bytes(name: &str) -> Cow<'_, [u8]> {
    if let Some(encoded) = name.strip_prefix('\0') {
        let decode = || {
            if !encoded.len().is_multiple_of(2) {
                return None;
            }
            encoded
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    let high = (pair[0] as char).to_digit(16)?;
                    let low = (pair[1] as char).to_digit(16)?;
                    Some((high * 16 + low) as u8)
                })
                .collect::<Option<Vec<u8>>>()
        };
        if let Some(bytes) = decode() {
            return Cow::Owned(bytes);
        }
    }
    Cow::Borrowed(name.as_bytes())
}
pub fn symbol_alias(name: &str) -> String {
    let bytes = name_bytes(name);
    if name.starts_with('\0')
        || bytes
            .iter()
            .any(|b| *b < 32 || *b == 127 || matches!(*b, b'\\' | b'"'))
    {
        format!("__biomiya_bytes_{}", hex(&bytes))
    } else {
        name.to_owned()
    }
}
pub fn asm_symbol(name: &str) -> String {
    format!("\"{}\"", symbol_alias(name))
}
/// Every explicit section is aliased, so a user section cannot collide with an alias.
pub fn section_alias(name: &str) -> String {
    format!(".biomiya.section.{}", hex(&name_bytes(name)))
}
fn malformed(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_LINK", Span::new(FileId(0), 0, 0), message)
}
fn at(bytes: &[u8], offset: usize, size: usize) -> Result<&[u8]> {
    bytes
        .get(
            offset
                ..offset
                    .checked_add(size)
                    .ok_or_else(|| malformed("ELF offset overflow"))?,
        )
        .ok_or_else(|| malformed("truncated ELF object"))
}
fn u16_at(bytes: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        at(bytes, offset, 2)?.try_into().unwrap(),
    ))
}
fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        at(bytes, offset, 4)?.try_into().unwrap(),
    ))
}
fn u64_at(bytes: &[u8], offset: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(
        at(bytes, offset, 8)?.try_into().unwrap(),
    ))
}
fn offset(value: u64) -> Result<usize> {
    usize::try_from(value).map_err(|_| malformed("ELF object exceeds host address space"))
}
fn string_at(table: &[u8], index: u32) -> Result<&[u8]> {
    let bytes = table
        .get(index as usize..)
        .ok_or_else(|| malformed("invalid ELF string index"))?;
    let len = bytes
        .iter()
        .position(|b| *b == 0)
        .ok_or_else(|| malformed("unterminated ELF name"))?;
    Ok(&bytes[..len])
}
#[derive(Clone)]
struct Section {
    header: usize,
    kind: u32,
    start: usize,
    size: usize,
    link: usize,
    entry_size: usize,
}
/// Restore aliases in an ELF64 little-endian RISC-V relocatable object. String
/// tables are extended at EOF; section/symbol indices and relocation data stay
/// unchanged. The caller must apply this before handing the object to a linker.
pub fn restore_object_names(input: &[u8], names: &LinkageNames) -> Result<Vec<u8>> {
    if names.is_empty() {
        return Ok(input.to_vec());
    }
    if at(input, 0, 6)? != b"\x7fELF\x02\x01"
        || u16_at(input, 16)? != 1
        || u16_at(input, 18)? != 243
    {
        return Err(malformed(
            "linkage restoration requires a RISC-V ELF64 little-endian relocatable object",
        ));
    }
    let headers = offset(u64_at(input, 40)?)?;
    at(input, headers, 64)?;
    if u16_at(input, 58)? != 64 {
        return Err(malformed("invalid ELF section header size"));
    }
    let count = match u16_at(input, 60)? {
        0 => offset(u64_at(input, headers + 32)?)?,
        n => n as usize,
    };
    let header_bytes = count
        .checked_mul(64)
        .ok_or_else(|| malformed("ELF section count overflow"))?;
    at(input, headers, header_bytes)?;
    let strings = match u16_at(input, 62)? {
        0xffff => u32_at(input, headers + 40)? as usize,
        n => n as usize,
    };
    let mut sections = Vec::with_capacity(count);
    for n in 0..count {
        let header = headers
            .checked_add(
                n.checked_mul(64)
                    .ok_or_else(|| malformed("ELF section count overflow"))?,
            )
            .ok_or_else(|| malformed("ELF section offset overflow"))?;
        at(input, header, 64)?;
        sections.push(Section {
            header,
            kind: u32_at(input, header + 4)?,
            start: offset(u64_at(input, header + 24)?)?,
            size: offset(u64_at(input, header + 32)?)?,
            link: u32_at(input, header + 40)? as usize,
            entry_size: offset(u64_at(input, header + 56)?)?,
        });
    }
    fn mappings(list: &[NameMapping]) -> Result<BTreeMap<&[u8], &[u8]>> {
        let mut map = BTreeMap::new();
        for mapping in list {
            if mapping.alias.is_empty()
                || mapping.alias.as_bytes().contains(&0)
                || mapping.bytes.is_empty()
                || mapping.bytes.contains(&0)
            {
                return Err(malformed("empty or NUL-containing linkage name"));
            }
            if let Some(previous) = map.insert(mapping.alias.as_bytes(), mapping.bytes.as_slice())
                && previous != mapping.bytes
            {
                return Err(malformed("conflicting linkage aliases"));
            }
        }
        Ok(map)
    }
    let symbols = mappings(&names.symbols)?;
    let section_names = mappings(&names.sections)?;
    let mut output = input.to_vec();
    let mut tables: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    let mut indices: BTreeMap<(usize, Vec<u8>), u32> = BTreeMap::new();
    fn replacement(
        table: usize,
        name: &[u8],
        input: &[u8],
        sections: &[Section],
        tables: &mut BTreeMap<usize, Vec<u8>>,
        indices: &mut BTreeMap<(usize, Vec<u8>), u32>,
    ) -> Result<u32> {
        let key = (table, name.to_vec());
        if let Some(index) = indices.get(&key) {
            return Ok(*index);
        }
        let section = sections
            .get(table)
            .ok_or_else(|| malformed("invalid ELF string table section"))?;
        if section.kind != 3 {
            return Err(malformed("ELF names require a string table"));
        }
        if let std::collections::btree_map::Entry::Vacant(entry) = tables.entry(table) {
            entry.insert(at(input, section.start, section.size)?.to_vec());
        }
        let contents = tables.get_mut(&table).unwrap();
        let index = u32::try_from(contents.len())
            .map_err(|_| malformed("ELF string table exceeds 32-bit indices"))?;
        contents.extend_from_slice(name);
        contents.push(0);
        indices.insert(key, index);
        Ok(index)
    }
    let section_strings = sections
        .get(strings)
        .ok_or_else(|| malformed("invalid ELF section-name table"))?;
    if section_strings.kind != 3 {
        return Err(malformed("invalid ELF section-name string table"));
    }
    let source_strings = at(input, section_strings.start, section_strings.size)?;
    for section in &sections {
        let name = string_at(source_strings, u32_at(input, section.header)?)?;
        if let Some(raw) = section_names.get(name) {
            let index = replacement(strings, raw, input, &sections, &mut tables, &mut indices)?;
            output[section.header..section.header + 4].copy_from_slice(&index.to_le_bytes());
        }
    }
    for section in &sections {
        if !matches!(section.kind, 2 | 11) {
            continue;
        }
        if section.entry_size != 24 || !section.size.is_multiple_of(24) {
            return Err(malformed("invalid ELF symbol table size"));
        }
        let table = sections
            .get(section.link)
            .ok_or_else(|| malformed("invalid ELF symbol string table"))?;
        if table.kind != 3 {
            return Err(malformed("invalid ELF symbol string table"));
        }
        at(input, section.start, section.size)?;
        let source = at(input, table.start, table.size)?;
        for n in 0..section.size / 24 {
            let position = section.start + n * 24;
            let name = string_at(source, u32_at(input, position)?)?;
            if let Some(raw) = symbols.get(name) {
                let index = replacement(
                    section.link,
                    raw,
                    input,
                    &sections,
                    &mut tables,
                    &mut indices,
                )?;
                output[position..position + 4].copy_from_slice(&index.to_le_bytes());
            }
        }
    }
    for (index, contents) in tables {
        let start = output.len() as u64;
        let size = contents.len() as u64;
        output.extend_from_slice(&contents);
        let header = sections[index].header;
        output[header + 24..header + 32].copy_from_slice(&start.to_le_bytes());
        output[header + 32..header + 40].copy_from_slice(&size.to_le_bytes());
    }
    Ok(output)
}
