use crate::su::{FrameType, SuEntry};
use anyhow::{Context, Result};
use gimli::RunTimeEndian;
use object::{Architecture, Object, ObjectSection, SectionKind};
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

// Bound aggregate allocated output from compressed DWARF sections.
const MAX_DWARF_BYTES: u64 = 64 * 1024 * 1024;

/// Parse an ELF file's DWARF `.debug_info` and code sections to extract
/// per-function stack frame sizes.
///
/// Supports ARM Thumb-2 and Xtensa LX (ESP32) targets. ISA is detected from
/// the ELF e_machine field. Names are demangled with `rustc-demangle`.
pub fn parse_elf_frames(path: &Path) -> Result<Vec<SuEntry>> {
    let data =
        std::fs::read(path).with_context(|| format!("failed to read ELF: {}", path.display()))?;

    let obj = object::File::parse(data.as_slice())
        .with_context(|| format!("failed to parse ELF: {}", path.display()))?;

    anyhow::ensure!(
        obj.format() == object::BinaryFormat::Elf
            && matches!(obj.architecture(), Architecture::Arm | Architecture::Xtensa),
        "unsupported ELF architecture {:?}; supported ISAs: ARM Thumb-2 and Xtensa LX",
        obj.architecture()
    );

    let endian = if obj.is_little_endian() {
        RunTimeEndian::Little
    } else {
        RunTimeEndian::Big
    };

    let is_xtensa = obj.architecture() == Architecture::Xtensa;

    // Build address → (mangled_name, func_len) map from .debug_info
    let name_map = build_name_map(&obj, endian)?;

    // Load all executable sections so we can resolve any function address.
    // ESP32 splits code across .flash.text / .iram0.text / etc.; ARM typically
    // puts everything in .text, but multi-section scanning works for both.
    let code_sections: Vec<(u64, Vec<u8>)> = obj
        .sections()
        .filter(|s| matches!(s.kind(), SectionKind::Text) && s.size() > 0)
        .filter_map(|s| s.data().ok().map(|d| (s.address(), d.to_vec())))
        .collect();

    // Fallback: if no sections matched above, try the named .text section.
    let fallback: Vec<(u64, Vec<u8>)> = if code_sections.is_empty() {
        obj.section_by_name(".text")
            .and_then(|s| s.data().ok().map(|d| (s.address(), d.to_vec())))
            .into_iter()
            .collect()
    } else {
        vec![]
    };
    let sections: &[(u64, Vec<u8>)] = if code_sections.is_empty() {
        &fallback
    } else {
        &code_sections
    };

    let mut entries = Vec::new();

    for (addr, (raw_name, func_len)) in &name_map {
        if *func_len == 0 {
            continue;
        }

        // Mask the Thumb bit (bit 0) that ARM toolchains set in symbol addresses.
        let code_addr = addr & !1;

        let Some((sec_base, sec_bytes)) = find_section(sections, code_addr) else {
            continue;
        };

        let offset = (code_addr - sec_base) as usize;
        let end = (offset + *func_len as usize).min(sec_bytes.len());
        if offset >= sec_bytes.len() {
            continue;
        }
        let func_bytes = &sec_bytes[offset..end];

        let frame_size = if is_xtensa {
            parse_xtensa_entry_frame_size(func_bytes)
        } else {
            parse_thumb2_frame_size(func_bytes)
        };

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

/// Find the code section that contains `addr`, returning (section_base, section_bytes).
fn find_section(sections: &[(u64, Vec<u8>)], addr: u64) -> Option<(u64, &[u8])> {
    sections
        .iter()
        .find(|(base, data)| addr >= *base && addr < base + data.len() as u64)
        .map(|(base, data)| (*base, data.as_slice()))
}

/// Validate both the ELF size claim and the entire zlib stream.
fn decompress_zlib(input: &[u8], expected: u64) -> Result<Vec<u8>> {
    let size = usize::try_from(expected).context("DWARF section size does not fit memory")?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(size)
        .context("cannot allocate DWARF section")?;
    output.resize(size, 0);
    let mut decoder = flate2::Decompress::new(true);
    let mut status = decoder.decompress(input, &mut output, flate2::FlushDecompress::Finish)?;
    if status != flate2::Status::StreamEnd {
        // A stack sentinel lets the decoder finish after filling the output,
        // or detect excess output, without growing the allocated buffer.
        let consumed = decoder.total_in() as usize;
        status = decoder.decompress(
            &input[consumed..],
            &mut [0u8; 1],
            flate2::FlushDecompress::Finish,
        )?;
    }
    anyhow::ensure!(
        status == flate2::Status::StreamEnd
            && decoder.total_in() == input.len() as u64
            && decoder.total_out() == expected,
        "zlib stream is incomplete, has trailing bytes, or disagrees with the ELF size"
    );
    Ok(output)
}

/// Build a map from function start address → (linkage_name, length) using `.debug_info`.
fn build_name_map(
    obj: &object::File<'_>,
    endian: RunTimeEndian,
) -> Result<HashMap<u64, (String, u64)>> {
    let mut remaining = MAX_DWARF_BYTES;
    let sections = gimli::DwarfSections::load(|id: gimli::SectionId| -> Result<Cow<'_, [u8]>> {
        let Some(section) = obj.section_by_name(id.name()) else {
            return Ok(Cow::Borrowed(&[]));
        };
        let compressed = section
            .compressed_data()
            .with_context(|| format!("incomplete ELF analysis: cannot read {}", id.name()))?;
        if compressed.format == object::CompressionFormat::None {
            return Ok(Cow::Borrowed(compressed.data));
        }
        anyhow::ensure!(
            compressed.uncompressed_size <= remaining,
            "incomplete ELF analysis: {} exceeds the total decompressed DWARF limit of {} bytes",
            id.name(),
            MAX_DWARF_BYTES
        );
        // ruzstd's internal window buffer is independent of the ELF size
        // header and can allocate up to 128 MiB even for tiny output.
        anyhow::ensure!(
            compressed.format == object::CompressionFormat::Zlib,
            "incomplete ELF analysis: unsupported or unbounded compression {:?} in {}",
            compressed.format,
            id.name()
        );
        remaining -= compressed.uncompressed_size;
        decompress_zlib(compressed.data, compressed.uncompressed_size)
            .map(Cow::Owned)
            .with_context(|| format!("incomplete ELF analysis: cannot decompress {}", id.name()))
    })?;
    let dwarf = sections.borrow(|data| gimli::EndianSlice::new(data, endian));

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

/// Extract the Xtensa LX stack frame size from the function's first ENTRY instruction.
///
/// Xtensa windowed-call ABI: every non-leaf function starts with:
///   ENTRY a1, imm12*8
/// 24-bit little-endian encoding:
///   byte 0 = 0x36 (op0=6 CALL, op1=3 ENTRY)
///   byte 1 = (imm12[3:0] << 4) | s   where s=1 for a1 (sp)
///   byte 2 = imm12[11:4]
///   frame  = imm12 * 8
///
/// Returns 0 for leaf functions that omit ENTRY.
fn parse_xtensa_entry_frame_size(bytes: &[u8]) -> u64 {
    if bytes.len() < 3 {
        return 0;
    }
    let b0 = bytes[0];
    let b1 = bytes[1];
    let b2 = bytes[2];
    // Require ENTRY using a1 (sp): byte0=0x36, low nibble of byte1=0x1
    if b0 != 0x36 || (b1 & 0x0F) != 0x01 {
        return 0;
    }
    let imm12 = ((b2 as u64) << 4) | ((b1 as u64) >> 4);
    imm12 * 8
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
    use super::{parse_elf_frames, parse_thumb2_frame_size, parse_xtensa_entry_frame_size};
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

    // ── Xtensa ENTRY tests ──────────────────────────────────────────────────

    #[test]
    fn test_xtensa_entry_32_bytes() {
        // ENTRY a1, 32 → bytes 36 41 00 (imm12=4, frame=4*8=32)
        let bytes = [0x36u8, 0x41, 0x00];
        assert_eq!(parse_xtensa_entry_frame_size(&bytes), 32);
    }

    #[test]
    fn test_xtensa_entry_64_bytes() {
        // ENTRY a1, 64 → bytes 36 81 00 (imm12=8, frame=64)
        let bytes = [0x36u8, 0x81, 0x00];
        assert_eq!(parse_xtensa_entry_frame_size(&bytes), 64);
    }

    #[test]
    fn test_xtensa_entry_384_bytes() {
        // ENTRY a1, 0x180=384 → bytes 36 01 03 (imm12=0x30=48, frame=48*8=384)
        let bytes = [0x36u8, 0x01, 0x03];
        assert_eq!(parse_xtensa_entry_frame_size(&bytes), 384);
    }

    #[test]
    fn test_xtensa_entry_48_bytes() {
        // ENTRY a1, 48 → bytes 36 61 00 (imm12=6, frame=48)
        let bytes = [0x36u8, 0x61, 0x00];
        assert_eq!(parse_xtensa_entry_frame_size(&bytes), 48);
    }

    #[test]
    fn test_xtensa_leaf_no_entry() {
        // Leaf function — first byte is not 0x36, frame = 0
        let bytes = [0x0cu8, 0x08, 0x00];
        assert_eq!(parse_xtensa_entry_frame_size(&bytes), 0);
    }

    #[test]
    fn test_xtensa_entry_wrong_register() {
        // byte1 low nibble != 1 (not a1) — not a frame-allocating ENTRY for sp
        let bytes = [0x36u8, 0x42, 0x00];
        assert_eq!(parse_xtensa_entry_frame_size(&bytes), 0);
    }

    #[test]
    fn test_xtensa_entry_too_short() {
        assert_eq!(parse_xtensa_entry_frame_size(&[0x36, 0x41]), 0);
    }
}
