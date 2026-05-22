use super::{MapData, MapFormat, Symbol};
use anyhow::Result;
use regex::Regex;
use std::sync::OnceLock;

static SYMBOL_RE: OnceLock<Regex> = OnceLock::new();
static SECTION_RE: OnceLock<Regex> = OnceLock::new();

fn symbol_re() -> &'static Regex {
    SYMBOL_RE.get_or_init(|| {
        Regex::new(r"^\s+(0x[0-9a-fA-F]+)\s+([a-zA-Z_][a-zA-Z0-9_.$@]*)$").unwrap()
    })
}

fn section_re() -> &'static Regex {
    SECTION_RE.get_or_init(|| {
        Regex::new(r"^\s+(\.[a-zA-Z_][a-zA-Z0-9_.$@]*)\s+(0x[0-9a-fA-F]+)\s+(0x[0-9a-fA-F]+)\s+(.+)$").unwrap()
    })
}

pub fn parse(content: &str, format: MapFormat) -> Result<MapData> {
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut current_section = String::new();
    let mut current_obj: Option<String> = None;

    for line in content.lines() {
        // Track current section context: lines like " .text.main  0x... 0x... file.o"
        if let Some(caps) = section_re().captures(line) {
            current_section = caps[1].to_string();
            current_obj = Some(caps[4].trim().to_string());
            continue;
        }

        // Bare section address lines: "  .text  0x00001234  0x5678"  (no object file)
        // These set the section but don't update the object
        let trimmed = line.trim();
        if trimmed.starts_with('.') {
            let parts: Vec<&str> = trimmed.splitn(3, char::is_whitespace).collect();
            if parts.len() >= 2 && parts[1].starts_with("0x") {
                current_section = parts[0].to_string();
                current_obj = None;
            }
            continue;
        }

        // Symbol definition: "                0x000001234                symbol_name"
        if let Some(caps) = symbol_re().captures(line) {
            let addr_str = &caps[1];
            let name = caps[2].to_string();

            // Skip linker-generated symbols that are not real functions
            if name.starts_with("__") && (name.ends_with("_start") || name.ends_with("_end")) {
                continue;
            }

            let address = u64::from_str_radix(&addr_str[2..], 16).unwrap_or(0);

            symbols.push(Symbol {
                name,
                address,
                size: 0, // GNU ld doesn't give per-symbol sizes inline
                section: current_section.clone(),
                object_file: current_obj.clone(),
            });
        }
    }

    // Deduplicate symbols by name (GNU ld can repeat symbols at link time)
    symbols.sort_by(|a, b| a.name.cmp(&b.name).then(a.address.cmp(&b.address)));
    symbols.dedup_by(|a, b| a.name == b.name && a.address == b.address);

    Ok(MapData {
        format,
        symbols,
        max_stack: None,
    })
}
