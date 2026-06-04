use super::{MapData, MapFormat, Symbol};
use anyhow::Result;
use regex::Regex;
use std::sync::OnceLock;

static SYMBOL_RE: OnceLock<Regex> = OnceLock::new();
static SECTION_RE: OnceLock<Regex> = OnceLock::new();
static SECTION_BODY_RE: OnceLock<Regex> = OnceLock::new();

fn symbol_re() -> &'static Regex {
    SYMBOL_RE
        .get_or_init(|| Regex::new(r"^\s+(0x[0-9a-fA-F]+)\s+([a-zA-Z_][a-zA-Z0-9_.$@]*)$").unwrap())
}

fn section_re() -> &'static Regex {
    SECTION_RE.get_or_init(|| {
        Regex::new(
            r"^\s+(\.[a-zA-Z_][a-zA-Z0-9_.$@]*)\s+(0x[0-9a-fA-F]+)\s+(0x[0-9a-fA-F]+)\s+(.+)$",
        )
        .unwrap()
    })
}

// ESP-IDF / multi-line format: "   0xADDR   0xSIZE   object_file"
// Appears on the line immediately after a bare section-name line.
fn section_body_re() -> &'static Regex {
    SECTION_BODY_RE
        .get_or_init(|| Regex::new(r"^\s+(0x[0-9a-fA-F]+)\s+(0x[0-9a-fA-F]+)\s+(\S+.*)$").unwrap())
}

pub fn parse(content: &str, format: MapFormat) -> Result<MapData> {
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut current_section = String::new();
    let mut current_obj: Option<String> = None;
    // Holds a section name seen alone on its own line (ESP-IDF two-line format).
    let mut pending_section: Option<String> = None;

    for line in content.lines() {
        // One-line section: " .text.main  0x...  0x...  file.o"
        if let Some(caps) = section_re().captures(line) {
            current_section = caps[1].to_string();
            let obj_str = caps[4].trim().to_string();
            current_obj = Some(obj_str.clone());
            pending_section = None;

            // Synthesize a symbol for static functions that won't get an explicit
            // symbol line (same logic as the two-line path below).
            if let Some(func_name) = current_section.strip_prefix(".text.") {
                if !func_name.is_empty()
                    && func_name.chars().all(|c| c.is_alphanumeric() || c == '_')
                {
                    let address = u64::from_str_radix(&caps[2][2..], 16).unwrap_or(0);
                    if address > 0 {
                        symbols.push(Symbol {
                            name: func_name.to_string(),
                            address,
                            size: 0,
                            section: current_section.clone(),
                            object_file: Some(obj_str),
                        });
                    }
                }
            }
            continue;
        }

        let trimmed = line.trim();

        // Lines starting with '*' are linker wildcards / fill directives — skip.
        if trimmed.starts_with('*') {
            pending_section = None;
            continue;
        }

        if trimmed.starts_with('.') {
            let parts: Vec<&str> = trimmed.split_ascii_whitespace().collect();
            if parts.len() >= 2 && parts[1].starts_with("0x") {
                // Bare one-line section with address but no object file.
                current_section = parts[0].to_string();
                current_obj = None;
                pending_section = None;
            } else if parts.len() == 1 {
                // Section name alone on its line (ESP-IDF two-line format).
                // The next body line carries address, size, and object file.
                pending_section = Some(parts[0].to_string());
            } else {
                pending_section = None;
            }
            continue;
        }

        // Two-line section body: "   0xADDR   0xSIZE   object_file"
        // Only consume this as a section header when we have a pending name.
        if let Some(ref sec_name) = pending_section.clone() {
            if let Some(caps) = section_body_re().captures(line) {
                let obj_str = caps[3].trim().to_string();
                // Reject "size before relaxing" annotation lines.
                if !obj_str.starts_with('(') {
                    current_section = sec_name.clone();
                    current_obj = Some(obj_str.clone());
                    pending_section = None;

                    // Synthesize a symbol for static functions: GCC emits a
                    // .text.funcname section per function but only writes an
                    // explicit symbol line for globally-visible functions.
                    // Extract the name from ".text.funcname" so static functions
                    // are visible in the analysis cross-reference.
                    if let Some(func_name) = sec_name.strip_prefix(".text.") {
                        if !func_name.is_empty()
                            && func_name.chars().all(|c| c.is_alphanumeric() || c == '_')
                        {
                            let addr_str = &caps[1];
                            let address = u64::from_str_radix(&addr_str[2..], 16).unwrap_or(0);
                            if address > 0 {
                                symbols.push(Symbol {
                                    name: func_name.to_string(),
                                    address,
                                    size: 0,
                                    section: sec_name.clone(),
                                    object_file: Some(obj_str),
                                });
                            }
                        }
                    }
                    continue;
                }
            }
            // Any non-body line cancels the pending section.
            pending_section = None;
        }

        // Symbol definition: "                0x000001234                symbol_name"
        if let Some(caps) = symbol_re().captures(line) {
            let addr_str = &caps[1];
            let name = caps[2].to_string();

            // Skip linker-generated boundary symbols.
            if name.starts_with("__") && (name.ends_with("_start") || name.ends_with("_end")) {
                continue;
            }

            let address = u64::from_str_radix(&addr_str[2..], 16).unwrap_or(0);

            symbols.push(Symbol {
                name,
                address,
                size: 0,
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

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::map::{MapFormat, Symbol};

    fn sym<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
        symbols
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("symbol '{name}' not found"))
    }

    #[test]
    fn test_symbol_count() {
        let content = std::fs::read_to_string("tests/fixtures/sample.map").unwrap();
        let result = parse(&content, MapFormat::GnuLd).unwrap();
        assert_eq!(result.symbols.len(), 5);
    }

    #[test]
    fn test_symbol_names() {
        let content = std::fs::read_to_string("tests/fixtures/sample.map").unwrap();
        let result = parse(&content, MapFormat::GnuLd).unwrap();
        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        for expected in &[
            "_Vectors",
            "Reset_Handler",
            "main",
            "process_data",
            "helper",
        ] {
            assert!(names.contains(expected), "missing symbol '{expected}'");
        }
    }

    #[test]
    fn test_section_tracking() {
        let content = std::fs::read_to_string("tests/fixtures/sample.map").unwrap();
        let result = parse(&content, MapFormat::GnuLd).unwrap();
        let s = &result.symbols;
        assert_eq!(sym(s, "_Vectors").section, ".isr_vector");
        assert_eq!(sym(s, "Reset_Handler").section, ".text.Reset_Handler");
        assert_eq!(sym(s, "main").section, ".text.main");
        assert_eq!(sym(s, "process_data").section, ".text.process_data");
        assert_eq!(sym(s, "helper").section, ".text.helper");
    }

    #[test]
    fn test_object_file_tracking() {
        let content = std::fs::read_to_string("tests/fixtures/sample.map").unwrap();
        let result = parse(&content, MapFormat::GnuLd).unwrap();
        let s = &result.symbols;
        assert_eq!(sym(s, "_Vectors").object_file.as_deref(), Some("startup.o"));
        assert_eq!(
            sym(s, "Reset_Handler").object_file.as_deref(),
            Some("startup.o")
        );
        assert_eq!(sym(s, "main").object_file.as_deref(), Some("main.o"));
        assert_eq!(
            sym(s, "process_data").object_file.as_deref(),
            Some("utils.o")
        );
        assert_eq!(sym(s, "helper").object_file.as_deref(), Some("utils.o"));
    }

    #[test]
    fn test_address_parsing() {
        let content = std::fs::read_to_string("tests/fixtures/sample.map").unwrap();
        let result = parse(&content, MapFormat::GnuLd).unwrap();
        let s = &result.symbols;
        assert_eq!(sym(s, "_Vectors").address, 0x0000000008000000);
        assert_eq!(sym(s, "Reset_Handler").address, 0x0000000008000188);
        assert_eq!(sym(s, "main").address, 0x00000000080001d0);
        assert_eq!(sym(s, "process_data").address, 0x000000000800022c);
        assert_eq!(sym(s, "helper").address, 0x00000000080002b4);
    }

    #[test]
    fn test_dedup() {
        let content = r#" .text.foo      0x0000000000001000       0x10 foo.o
                0x0000000000001000                foo
                0x0000000000001000                foo
"#;
        let result = parse(content, MapFormat::GnuLd).unwrap();
        assert_eq!(
            result.symbols.len(),
            1,
            "duplicate (name, addr) should collapse to one"
        );
    }

    #[test]
    fn test_linker_symbol_filtering() {
        let content = r#" .bss           0x0000000020000000       0x200 app.o
                0x0000000020000000                __bss_start
                0x0000000020000200                __bss_end
                0x0000000020000000                real_var
"#;
        let result = parse(content, MapFormat::GnuLd).unwrap();
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "real_var");
    }

    #[test]
    fn test_no_max_stack() {
        let content = std::fs::read_to_string("tests/fixtures/sample.map").unwrap();
        let result = parse(&content, MapFormat::GnuLd).unwrap();
        assert!(result.max_stack.is_none());
    }

    #[test]
    fn test_bare_section_clears_obj() {
        let content = r#" .text.first    0x0000000000001000       0x10 first.o
                0x0000000000001000                from_first
.text           0x0000000000002000      0x100
                0x0000000000002000                from_bare
"#;
        let result = parse(content, MapFormat::GnuLd).unwrap();
        let s = &result.symbols;
        assert_eq!(sym(s, "from_first").object_file.as_deref(), Some("first.o"));
        let bare = sym(s, "from_bare");
        assert_eq!(bare.section, ".text");
        assert!(bare.object_file.is_none());
    }

    #[test]
    fn test_esp_idf_two_line_section_format() {
        // ESP-IDF map: section name on its own line, address/size/obj on the next.
        let content = r#" .text.app_main
                0x42007b1c       0xb2 esp-idf/src/libsrc.a(main.c.obj)
                                 0xc6 (size before relaxing)
                0x42007b1c                app_main
 .text.PID_init
                0x42007bd0       0x12 esp-idf/src/libsrc.a(cmd_pump.c.obj)
                0x42007bd0                PID_init
"#;
        let result = parse(content, MapFormat::EspIdf).unwrap();
        let s = &result.symbols;
        assert_eq!(s.len(), 2);
        let main = sym(s, "app_main");
        assert_eq!(main.address, 0x42007b1c);
        assert_eq!(main.section, ".text.app_main");
        assert_eq!(
            main.object_file.as_deref(),
            Some("esp-idf/src/libsrc.a(main.c.obj)")
        );
        let pid = sym(s, "PID_init");
        assert_eq!(pid.address, 0x42007bd0);
        assert_eq!(pid.section, ".text.PID_init");
        assert_eq!(
            pid.object_file.as_deref(),
            Some("esp-idf/src/libsrc.a(cmd_pump.c.obj)")
        );
    }

    #[test]
    fn test_esp_idf_fill_and_wildcard_lines_skipped() {
        // *fill* and *(...) lines must not disrupt section/symbol tracking.
        // Both foo (static, synthesized) and bar (global, explicit) should appear.
        let content = r#" .text.foo
                0x42000100       0x10 libfoo.a(foo.c.obj)
 *fill*         0x42000110        0x2
 .text.bar
                0x42000112        0x8 libbar.a(bar.c.obj)
                0x42000112                bar
"#;
        let result = parse(content, MapFormat::EspIdf).unwrap();
        let s = &result.symbols;
        assert_eq!(s.len(), 2);
        assert_eq!(sym(s, "foo").section, ".text.foo");
        assert_eq!(sym(s, "foo").address, 0x42000100);
        assert_eq!(sym(s, "bar").section, ".text.bar");
    }
}
