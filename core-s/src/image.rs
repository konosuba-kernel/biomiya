//! Static ELF64 executable: RX text, R constants, RW initialized data and BSS.
use crate::{
    diagnostic::{Diagnostic, Result},
    encode,
    machine::{self, Program},
    source::Span,
};
use std::collections::BTreeMap;

pub const TEXT_BASE: u64 = 0x10000;
const PAGE: u64 = 4096;
const MAX_IMAGE: u64 = 64 * 1024 * 1024;
fn invalid(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("I_INTERNAL", Span::default(), message)
}
fn limit(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("I_LIMIT", Span::default(), message)
}
fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b)
        .ok_or_else(|| limit("image address overflow"))
}
fn aligned(n: u64, align: u64) -> Result<u64> {
    Ok(add(n, align - 1)? & !(align - 1))
}
fn bounded(n: u64) -> Result<u64> {
    if n > MAX_IMAGE {
        Err(limit("image exceeds 64 MiB"))
    } else {
        Ok(n)
    }
}

pub fn executable(program: &Program, entry: &str) -> Result<Vec<u8>> {
    machine::validate_data(&program.data)?;
    let text_size = encode::text_size(program)?;
    // Initialized objects are packed first; BSS follows all initialized RW data.
    let mut lengths = [0u64; 3];
    let mut locations = Vec::new();
    for (i, object) in program.data.iter().enumerate() {
        let section = if !object.mutable {
            0
        } else if object.bytes.is_empty() {
            2
        } else {
            1
        };
        let offset = aligned(lengths[section], object.align)?;
        lengths[section] = bounded(add(
            offset,
            if section == 2 {
                object.zero_fill
            } else {
                object.bytes.len() as u64
            },
        )?)?;
        locations.push((i, section, offset));
    }
    let ro_size = lengths[0].max(1);
    let rw_size = lengths[1].max(1);
    let bss_align = program
        .data
        .iter()
        .filter(|o| o.bytes.is_empty())
        .map(|o| o.align)
        .max()
        .unwrap_or(1);
    let bss_start = aligned(rw_size, bss_align)?;
    let rw_memory = if lengths[2] == 0 {
        rw_size
    } else {
        add(bss_start, lengths[2])?
    };
    let ro_base = aligned(add(TEXT_BASE, text_size)?, PAGE)?;
    let rw_base = aligned(add(ro_base, ro_size)?, PAGE)?;
    bounded(add(rw_base - TEXT_BASE, rw_memory)?)?;
    let mut symbols = BTreeMap::new();
    for &(i, section, offset) in &locations {
        let base = match section {
            0 => ro_base,
            1 => rw_base,
            _ => add(rw_base, bss_start)?,
        };
        symbols.insert(program.data[i].symbol.clone(), add(base, offset)?);
    }
    let encoded = encode::encode_with_symbols(program, TEXT_BASE, &symbols)?;
    let entry = *encoded
        .symbols
        .get(entry)
        .ok_or_else(|| invalid("missing ELF entry symbol"))?;
    if !(TEXT_BASE..TEXT_BASE + text_size).contains(&entry) {
        return Err(invalid("ELF entry outside text"));
    }
    let offsets = [
        PAGE,
        aligned(PAGE + text_size, PAGE)?,
        aligned(add(aligned(PAGE + text_size, PAGE)?, ro_size)?, PAGE)?,
    ];
    let addresses = [TEXT_BASE, ro_base, rw_base];
    let sizes = [text_size, ro_size, rw_size];
    let memories = [text_size, ro_size, rw_memory];
    let mut bytes = vec![0u8; add(offsets[2], rw_size)? as usize];
    bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    put16(&mut bytes, 16, 2);
    put16(&mut bytes, 18, 243);
    put32(&mut bytes, 20, 1);
    put64(&mut bytes, 24, entry);
    put64(&mut bytes, 32, 64);
    put16(&mut bytes, 52, 64);
    put16(&mut bytes, 54, 56);
    put16(&mut bytes, 56, 3);
    for n in 0..3 {
        let at = 64 + n * 56;
        put32(&mut bytes, at, 1);
        put32(&mut bytes, at + 4, [5, 4, 6][n]);
        put64(&mut bytes, at + 8, offsets[n]);
        put64(&mut bytes, at + 16, addresses[n]);
        put64(&mut bytes, at + 24, addresses[n]);
        put64(&mut bytes, at + 32, sizes[n]);
        put64(&mut bytes, at + 40, memories[n]);
        put64(&mut bytes, at + 48, PAGE);
    }
    bytes[PAGE as usize..(PAGE + text_size) as usize].copy_from_slice(&encoded.bytes);
    for (i, section, offset) in locations {
        let object = &program.data[i];
        if section == 2 {
            continue;
        }
        let start = (offsets[section + 1] + offset) as usize;
        bytes[start..start + object.bytes.len()].copy_from_slice(&object.bytes);
        for relocation in &object.relocations {
            let address = symbols
                .get(&relocation.target)
                .or_else(|| encoded.symbols.get(&relocation.target))
                .ok_or_else(|| invalid(format!("undefined ABS64 symbol {}", relocation.target)))?;
            let address = (*address as i128) + relocation.addend as i128;
            let address = u64::try_from(address).map_err(|_| limit("ABS64 addend overflow"))?;
            put64(&mut bytes, start + relocation.offset as usize, address);
        }
    }
    Ok(bytes)
}
