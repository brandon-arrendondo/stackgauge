/// Parser for lld's map format.
///
/// lld map files start with a header line:
/// ```text
///      VMA      LMA     Size Align Out     In      Symbol
/// ```
///
/// Lines are indented to indicate depth:
///   - depth 0 (least indented after columns 0-4): output section name
///   - depth 1 (more indented): input file / object
///   - depth 2 (most indented): symbol assignment
///
/// For Rust + LTO builds the symbol table is almost always empty (no
/// per-function symbols survive LTO in the map).  This parser collects
/// whatever is present without erroring, so it can be composed with ELF
/// DWARF parsing.
use super::{MapData, MapFormat, Symbol};
use anyhow::Result;

pub fn parse(content: &str, format: MapFormat) -> Result<MapData> {
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut current_section = String::new();

    // Determine the column at which each depth's name field starts.
    // We detect this dynamically from the header line:
    //   "     VMA      LMA     Size Align Out     In      Symbol"
    // The positions of "Out", "In", and "Symbol" give us the three
    // column boundaries.  We fall back to hard-coded defaults if the
    // header is absent.
    let mut col_out: usize = 0; // output section column (depth 0)
    let mut col_in: usize = 0; // input file column (depth 1)
    let mut col_sym: usize = 0; // symbol column (depth 2)

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Detect header line and extract column positions
        if trimmed.starts_with("VMA") && trimmed.contains("LMA") && trimmed.contains("Out") {
            col_out = line.find("Out").unwrap_or(28);
            col_in = line.find("In").unwrap_or(36);
            col_sym = line.find("Symbol").unwrap_or(44);
            continue;
        }

        // All data lines have at least 4 numeric columns: VMA LMA Size Align
        // followed by the name field at some column.
        // Collect up to 5 whitespace-separated tokens (where multi-space runs
        // count as one separator).
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();

        if tokens.len() < 5 {
            continue;
        }

        let is_numeric = |s: &str| {
            let s = s.strip_prefix("0x").unwrap_or(s);
            !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit())
        };

        if !is_numeric(tokens[0]) || !is_numeric(tokens[2]) {
            continue;
        }

        let vma = parse_u64(tokens[0]);
        let size = parse_u64(tokens[2]);
        // Reconstruct the full name field from the original line: everything
        // after skipping 4 whitespace-separated tokens.
        let name_rest = name_rest_from_line(line, 4).unwrap_or(tokens[4]).trim();

        // Find column position of the name field in the original line.
        // We locate the start of the 5th whitespace-separated run in the line.
        let name_col = find_nth_token_col(line, 5).unwrap_or(col_out);

        // Classify by column
        if col_sym > 0 && name_col >= col_sym {
            // Symbol line — first word is the name (rest may be "= value")
            let name_field = name_rest.split_whitespace().next().unwrap_or("").trim();
            // Skip if the very next non-space character is '=' (linker assign)
            let after_name = name_rest[name_field.len()..].trim_start();
            if after_name.starts_with('=') || name_field.is_empty() {
                continue;
            }
            symbols.push(Symbol {
                name: name_field.to_string(),
                address: vma,
                size,
                section: current_section.clone(),
                object_file: None,
            });
        } else if col_in > 0 && name_col >= col_in {
            // Input file line — skip, just context
        } else {
            // Output section line
            let section_name = name_rest.split_whitespace().next().unwrap_or(name_rest);
            current_section = section_name.to_string();
        }
    }

    Ok(MapData {
        format,
        symbols,
        max_stack: None,
    })
}

/// Return the slice of `line` starting at the first non-whitespace character
/// after the first `skip` whitespace-separated tokens have been consumed.
fn name_rest_from_line(line: &str, skip: usize) -> Option<&str> {
    let mut remaining = line;
    for _ in 0..skip {
        // skip leading whitespace
        remaining = remaining.trim_start();
        // skip the token itself
        let end = remaining
            .find(|c: char| c.is_whitespace())
            .unwrap_or(remaining.len());
        remaining = &remaining[end..];
    }
    remaining = remaining.trim_start();
    if remaining.is_empty() {
        None
    } else {
        Some(remaining)
    }
}

/// Return the byte-column (0-based) where the N-th whitespace-separated
/// token starts in `line` (N is 1-based).
fn find_nth_token_col(line: &str, n: usize) -> Option<usize> {
    let mut token_count = 0;
    let mut in_token = false;
    for (i, ch) in line.char_indices() {
        if ch.is_whitespace() {
            in_token = false;
        } else {
            if !in_token {
                token_count += 1;
                if token_count == n {
                    return Some(i);
                }
            }
            in_token = true;
        }
    }
    None
}

fn parse_u64(s: &str) -> u64 {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).unwrap_or(0)
    } else {
        s.parse::<u64>().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::map::MapFormat;

    const SAMPLE: &str = "\
     VMA      LMA     Size Align Out     In      Symbol
   10000    10000     1000     4 .text
   10000    10000      100    16         /path/to/foo.o:(.text)
   10000    10000      100    16                 some_function
   10100    10100      900    16         /path/to/bar.o:(.text)
   10100    10100      900    16                 another_fn
   20000    20000        4     4 .data
   20000    20000        4     4         /path/to/foo.o:(.data)
   20000    20000        4     4                 MY_GLOBAL = 42
";

    #[test]
    fn test_parses_symbols_from_lld_map() {
        let result = parse(SAMPLE, MapFormat::RustLld).unwrap();
        // MY_GLOBAL has '=' so it should be skipped
        // some_function and another_fn should be collected
        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"some_function"),
            "expected some_function, got: {:?}",
            names
        );
        assert!(
            names.contains(&"another_fn"),
            "expected another_fn, got: {:?}",
            names
        );
        // Linker-assigned value should be excluded
        assert!(
            !names.contains(&"MY_GLOBAL"),
            "MY_GLOBAL should be excluded"
        );
    }

    #[test]
    fn test_empty_map_returns_no_symbols() {
        let result = parse("", MapFormat::RustLld).unwrap();
        assert!(result.symbols.is_empty());
    }

    #[test]
    fn test_header_only_returns_no_symbols() {
        let header = "     VMA      LMA     Size Align Out     In      Symbol\n";
        let result = parse(header, MapFormat::RustLld).unwrap();
        assert!(result.symbols.is_empty());
    }

    #[test]
    fn test_section_assigned_to_symbols() {
        let result = parse(SAMPLE, MapFormat::RustLld).unwrap();
        // Symbols inside .text should have section = ".text"
        for sym in &result.symbols {
            if sym.name == "some_function" || sym.name == "another_fn" {
                assert_eq!(sym.section, ".text", "section mismatch for {}", sym.name);
            }
        }
    }
}
