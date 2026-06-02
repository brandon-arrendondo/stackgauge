use crate::su::{FrameType, SuEntry};
use anyhow::{Context, Result};
use gimli::RunTimeEndian;
use object::{Object, ObjectSection};
use std::collections::HashMap;
use std::path::Path;

/// Parse an ELF file's DWARF `.debug_info` and `.text` sections to extract
/// per-function stack frame sizes for ARM Thumb-2 targets.
///
/// Frame size = bytes pushed by PUSH instructions + bytes allocated by SUB SP.
/// Names are demangled with `rustc-demangle`.
pub fn parse_elf_frames(path: &Path) -> Result<Vec<SuEntry>> {
    let data =
        std::fs::read(path).with_context(|| format!("failed to read ELF: {}", path.display()))?;

    let obj = object::File::parse(data.as_slice())
        .with_context(|| format!("failed to parse ELF: {}", path.display()))?;

    let endian = if obj.is_little_endian() {
        RunTimeEndian::Little
    } else {
        RunTimeEndian::Big
    };

    // Build address → (mangled_name, func_len) map from .debug_info
    let name_map = build_name_map(&obj, endian)?;

    // Load .text section bytes for instruction scanning
    let text_section = obj.section_by_name(".text");
    let text_base = text_section.as_ref().map(|s| s.address()).unwrap_or(0);
    let text_bytes: Vec<u8> = text_section
        .and_then(|s| s.data().ok())
        .map(|d| d.to_vec())
        .unwrap_or_default();

    let mut entries = Vec::new();

    for (addr, (raw_name, func_len)) in &name_map {
        if *func_len == 0 {
            continue;
        }

        // Mask the Thumb bit (bit 0) that some toolchains set in symbol addresses
        let code_addr = addr & !1;
        if text_bytes.is_empty() || code_addr < text_base {
            continue;
        }
        let offset = (code_addr - text_base) as usize;
        if offset >= text_bytes.len() {
            continue;
        }
        let end = (offset + *func_len as usize).min(text_bytes.len());
        let func_bytes = &text_bytes[offset..end];

        let frame_size = parse_thumb2_frame_size(func_bytes);
        if frame_size == 0 {
            continue;
        }

        let demangled = rustc_demangle::demangle(raw_name).to_string();
        entries.push(SuEntry {
            source_file: String::new(),
            line: 0,
            col: 0,
            function_name: demangled,
            frame_size,
            frame_type: FrameType::Static,
        });
    }

    entries.sort_by_key(|b| std::cmp::Reverse(b.frame_size));

    Ok(entries)
}

/// Build a map from function start address → (linkage_name, length) using `.debug_info`.
fn build_name_map(
    obj: &object::File<'_>,
    endian: RunTimeEndian,
) -> Result<HashMap<u64, (String, u64)>> {
    let load_section = |id: gimli::SectionId| -> std::result::Result<
        gimli::EndianSlice<'_, RunTimeEndian>,
        gimli::Error,
    > {
        let data = match obj.section_by_name(id.name()) {
            Some(s) => s.data().unwrap_or(&[]),
            None => &[],
        };
        Ok(gimli::EndianSlice::new(data, endian))
    };

    let dwarf = gimli::Dwarf::load(load_section)
        .with_context(|| "failed to load DWARF from .debug_info")?;

    let mut name_map: HashMap<u64, (String, u64)> = HashMap::new();

    let mut iter = dwarf.units();
    while let Some(header) = iter.next()? {
        let unit = dwarf.unit(header)?;
        let mut entries = unit.entries();
        while let Some((_, entry)) = entries.next_dfs()? {
            if entry.tag() != gimli::DW_TAG_subprogram {
                continue;
            }

            let low_pc = match entry.attr_value(gimli::DW_AT_low_pc)? {
                Some(gimli::AttributeValue::Addr(a)) => a,
                _ => continue,
            };

            // DW_AT_high_pc: DWARF 4+ may encode as absolute address or length
            let func_len = match entry.attr_value(gimli::DW_AT_high_pc)? {
                Some(gimli::AttributeValue::Addr(high)) if high > low_pc => high - low_pc,
                Some(gimli::AttributeValue::Udata(len)) => len,
                Some(gimli::AttributeValue::Data1(len)) => len as u64,
                Some(gimli::AttributeValue::Data2(len)) => len as u64,
                Some(gimli::AttributeValue::Data4(len)) => len as u64,
                Some(gimli::AttributeValue::Data8(len)) => len,
                _ => 0,
            };

            // Prefer DW_AT_linkage_name (mangled), fall back to DW_AT_name
            let name_attr = entry
                .attr_value(gimli::DW_AT_linkage_name)?
                .or_else(|| entry.attr_value(gimli::DW_AT_name).ok().flatten());

            if let Some(attr_val) = name_attr {
                if let Ok(s) = dwarf.attr_string(&unit, attr_val) {
                    if let Ok(name_str) = s.to_string() {
                        name_map.insert(low_pc, (name_str.to_string(), func_len));
                    }
                }
            }
        }
    }

    Ok(name_map)
}

/// Compute the stack frame size for an ARM Thumb-2 function by scanning its bytes.
///
/// Sums bytes consumed by:
/// - PUSH {rlist} / PUSH {rlist, LR}: T1 encodings 0xB4xx / 0xB5xx
/// - SUB SP, SP, #imm7×4: T1 encoding 0xB080–0xB0FF
///
/// 32-bit Thumb-2 words (first halfword bits [15:11] ≥ 0x1D) are skipped.
fn parse_thumb2_frame_size(bytes: &[u8]) -> u64 {
    let mut total: u64 = 0;
    let mut i = 0;

    while i + 1 < bytes.len() {
        let hw = (bytes[i] as u16) | ((bytes[i + 1] as u16) << 8);

        // 32-bit instruction: first halfword bits [15:11] = 11101 / 11110 / 11111
        if (hw >> 11) >= 0x1D {
            i += 4;
            continue;
        }

        if (hw & 0xFF00) == 0xB400 {
            // PUSH {rlist}: popcount of low 8 bits × 4 bytes each
            total += (hw & 0xFF).count_ones() as u64 * 4;
        } else if (hw & 0xFF00) == 0xB500 {
            // PUSH {rlist, LR}: same as above plus 4 for LR
            total += ((hw & 0xFF).count_ones() + 1) as u64 * 4;
        } else if (hw & 0xFF80) == 0xB080 {
            // SUB SP, SP, #imm7×4
            let imm7 = (hw & 0x7F) as u64;
            total += imm7 * 4;
        }

        i += 2;
    }

    total
}

#[cfg(test)]
mod tests {
    use super::{parse_elf_frames, parse_thumb2_frame_size};
    use std::path::Path;

    #[test]
    fn test_missing_elf_returns_error() {
        let result = parse_elf_frames(Path::new("/nonexistent/firmware.elf"));
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_elf_returns_error() {
        let result = parse_elf_frames(Path::new("Cargo.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_push_r7_lr_gives_8_bytes() {
        // PUSH {r7, lr}: halfword 0xB580 → [0x80, 0xB5] in LE
        // popcount(0x80) + 1 (LR) = 1 + 1 = 2 registers × 4 = 8 bytes
        let bytes = [0x80u8, 0xB5];
        assert_eq!(parse_thumb2_frame_size(&bytes), 8);
    }

    #[test]
    fn test_sub_sp_448_bytes() {
        // SUB SP, SP, #448 = imm7=112=0x70 → halfword 0xB0F0 → [0xF0, 0xB0]
        let bytes = [0xF0u8, 0xB0];
        assert_eq!(parse_thumb2_frame_size(&bytes), 448);
    }

    #[test]
    fn test_push_and_sub_sp_combined() {
        // Typical Cortex-M23 prologue:
        //   PUSH {r7, lr}    → 8 bytes
        //   ADD r7, sp, #0   → ignored (0xAF00)
        //   SUB SP, #448     → 448 bytes
        // Total = 456 bytes
        let bytes = [
            0x80, 0xB5, // PUSH {r7, lr}
            0x00, 0xAF, // ADD r7, sp, #0  (not a frame instruction)
            0xF0, 0xB0, // SUB SP, #448
        ];
        assert_eq!(parse_thumb2_frame_size(&bytes), 456);
    }

    #[test]
    fn test_32bit_instruction_skipped() {
        // 32-bit instruction (first halfword 0xF800, bits [15:11] = 11111 ≥ 0x1D)
        // followed by PUSH {r7, lr} = 8 bytes
        let bytes = [
            0x00, 0xF8, 0x00, 0x00, // 32-bit word — must be skipped
            0x80, 0xB5, // PUSH {r7, lr}
        ];
        assert_eq!(parse_thumb2_frame_size(&bytes), 8);
    }

    #[test]
    fn test_push_without_lr() {
        // PUSH {r4, r5, r6, r7}: halfword 0xB4F0 → [0xF0, 0xB4]
        // popcount(0xF0) = 4 registers × 4 = 16 bytes
        let bytes = [0xF0u8, 0xB4];
        assert_eq!(parse_thumb2_frame_size(&bytes), 16);
    }

    #[test]
    fn test_empty_function_gives_zero() {
        assert_eq!(parse_thumb2_frame_size(&[]), 0);
    }
}
