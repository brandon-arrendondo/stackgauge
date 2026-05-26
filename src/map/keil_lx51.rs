use super::{CallChainEntry, CallNode, MapData, MapFormat, MaxStackInfo, Symbol};
use anyhow::Result;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

static FUNC_ENTRY_RE: OnceLock<Regex> = OnceLock::new();
static FUNC_TRUNC_RE: OnceLock<Regex> = OnceLock::new();
static CONTINUATION_RE: OnceLock<Regex> = OnceLock::new();
static CALLEE_RE: OnceLock<Regex> = OnceLock::new();
static PAGE_HEADER_RE: OnceLock<Regex> = OnceLock::new();

fn func_entry_re() -> &'static Regex {
    FUNC_ENTRY_RE.get_or_init(|| {
        // FUNC_NAME/MODULE    XXXXH YYYYH   or   FUNC_NAME/MODULE    ----- -----
        Regex::new(r"^([A-Z0-9_?][A-Z0-9_?./]*)\s+([0-9A-F]{4}H|-----)\s+([0-9A-F]{4}H|-----)\s*$").unwrap()
    })
}

fn func_trunc_re() -> &'static Regex {
    FUNC_TRUNC_RE.get_or_init(|| {
        // Truncated name ends with +, then addresses
        Regex::new(r"^([A-Z0-9_?][A-Z0-9_?./]*)\+\s+([0-9A-F]{4}H|-----)\s+([0-9A-F]{4}H|-----)\s*$").unwrap()
    })
}

fn continuation_re() -> &'static Regex {
    CONTINUATION_RE.get_or_init(|| {
        Regex::new(r"^\.\.\. ([A-Z0-9_?./]*)$").unwrap()
    })
}

fn callee_re() -> &'static Regex {
    CALLEE_RE.get_or_init(|| {
        Regex::new(r"^\s+\+-->\s+([A-Z0-9_?][A-Z0-9_?./]*)$").unwrap()
    })
}

fn page_header_re() -> &'static Regex {
    PAGE_HEADER_RE.get_or_init(|| {
        Regex::new(r"^LX51 LINKER/LOCATER").unwrap()
    })
}

fn parse_addr(s: &str) -> Option<u64> {
    if s == "-----" {
        return None;
    }
    u64::from_str_radix(s.trim_end_matches('H'), 16).ok()
}

fn frame_size(start: Option<u64>, stop: Option<u64>) -> u64 {
    match (start, stop) {
        (Some(a), Some(b)) if b >= a => b - a + 1,
        _ => 0,
    }
}

#[derive(Debug)]
struct OverlayEntry {
    name: String,
    xdata_start: Option<u64>,
    frame_bytes: u64,
    callees: Vec<String>,
}

pub fn parse(content: &str, format: MapFormat) -> Result<MapData> {
    let entries = parse_overlay_map(content);

    // Build call graph
    let mut call_graph: HashMap<String, CallNode> = HashMap::new();
    let mut symbols: Vec<Symbol> = Vec::new();

    for entry in &entries {
        let node = call_graph
            .entry(entry.name.clone())
            .or_insert_with(|| CallNode {
                frame_size: entry.frame_bytes,
                callees: Vec::new(),
            });
        node.frame_size = entry.frame_bytes;
        for callee in &entry.callees {
            if !node.callees.contains(callee) {
                node.callees.push(callee.clone());
            }
        }

        // Expose as a symbol for the function table (use xdata_start as address)
        symbols.push(Symbol {
            name: entry.name.clone(),
            address: entry.xdata_start.unwrap_or(0),
            size: entry.frame_bytes,
            section: "XDATA".to_string(),
            object_file: None,
        });
    }

    // Ensure every callee that appears but wasn't declared as a header gets a node
    let callee_names: Vec<String> = call_graph
        .values()
        .flat_map(|n| n.callees.clone())
        .collect();
    for callee in callee_names {
        call_graph.entry(callee).or_insert_with(|| CallNode {
            frame_size: 0,
            callees: Vec::new(),
        });
    }

    let max_stack = compute_max_stack(&call_graph);

    Ok(MapData {
        format,
        symbols,
        max_stack,
    })
}

fn parse_overlay_map(content: &str) -> Vec<OverlayEntry> {
    let mut entries: Vec<OverlayEntry> = Vec::new();
    let mut in_overlay = false;
    let mut past_separator = false;

    // Pending state for building current entry
    let mut current: Option<OverlayEntry> = None;
    // Pending truncated name waiting for continuation
    let mut pending_trunc: Option<(String, Option<u64>, u64)> = None;

    for line in content.lines() {
        // Detect overlay map start
        if !in_overlay {
            if line.starts_with("OVERLAY MAP OF MODULE:") {
                in_overlay = true;
            }
            continue;
        }

        // Skip page-break headers
        if page_header_re().is_match(line) {
            continue;
        }

        // Wait for the separator "===" line that ends the column headers
        if !past_separator {
            if line.trim_start().starts_with("====") {
                past_separator = true;
            }
            continue;
        }

        let trimmed = line.trim();

        // Empty lines — no special action needed
        if trimmed.is_empty() {
            continue;
        }

        // Handle continuation for truncated names: "... REST"
        if let Some(caps) = continuation_re().captures(trimmed) {
            if let Some((prefix, start, frame)) = pending_trunc.take() {
                let full_name = format!("{}{}", prefix, &caps[1]);
                push_current(&mut entries, current.take());
                current = Some(OverlayEntry {
                    name: full_name,
                    xdata_start: start,
                    frame_bytes: frame,
                    callees: Vec::new(),
                });
            }
            continue;
        }

        // Callee line: "  +--> CALLEE/MODULE"
        if let Some(caps) = callee_re().captures(line) {
            let callee = caps[1].to_string();
            if let Some(ref mut entry) = current {
                if !entry.callees.contains(&callee) {
                    entry.callees.push(callee);
                }
            }
            continue;
        }

        // Truncated function header: "NAME+  XXXXH YYYYH"
        if let Some(caps) = func_trunc_re().captures(trimmed) {
            let prefix = caps[1].to_string();
            let start = parse_addr(&caps[2]);
            let stop = parse_addr(&caps[3]);
            let frame = frame_size(start, stop);
            push_current(&mut entries, current.take());
            pending_trunc = Some((prefix, start, frame));
            continue;
        }

        // Normal function header: "NAME  XXXXH YYYYH"
        if let Some(caps) = func_entry_re().captures(trimmed) {
            let name = caps[1].to_string();
            let start = parse_addr(&caps[2]);
            let stop = parse_addr(&caps[3]);
            let frame = frame_size(start, stop);
            push_current(&mut entries, current.take());
            current = Some(OverlayEntry {
                name,
                xdata_start: start,
                frame_bytes: frame,
                callees: Vec::new(),
            });
            continue;
        }
    }

    push_current(&mut entries, current);
    entries
}

fn push_current(entries: &mut Vec<OverlayEntry>, entry: Option<OverlayEntry>) {
    if let Some(e) = entry {
        entries.push(e);
    }
}

fn compute_max_stack(graph: &HashMap<String, CallNode>) -> Option<MaxStackInfo> {
    if graph.is_empty() {
        return None;
    }

    // Roots: nodes not appearing as a callee of anyone else
    let all_callees: HashSet<&str> = graph
        .values()
        .flat_map(|n| n.callees.iter().map(|s| s.as_str()))
        .collect();

    let roots: Vec<&str> = graph
        .keys()
        .filter(|k| !all_callees.contains(k.as_str()))
        .map(|s| s.as_str())
        .collect();

    let mut best: Vec<CallChainEntry> = Vec::new();
    let mut best_bytes: u64 = 0;

    for root in roots {
        let mut visited = HashSet::new();
        let (chain, bytes) = dfs_max_xdata(root, graph, &mut visited);
        if bytes > best_bytes || (bytes == best_bytes && chain.len() > best.len()) {
            best = chain;
            best_bytes = bytes;
        }
    }

    if best.is_empty() {
        return None;
    }

    Some(MaxStackInfo {
        bytes: best_bytes,
        unknown_factors: Vec::new(),
        chain: best,
    })
}

fn dfs_max_xdata(
    name: &str,
    graph: &HashMap<String, CallNode>,
    visited: &mut HashSet<String>,
) -> (Vec<CallChainEntry>, u64) {
    if !visited.insert(name.to_string()) {
        return (Vec::new(), 0);
    }

    let frame = graph.get(name).map(|n| n.frame_size).unwrap_or(0);

    let mut best_suffix: Vec<CallChainEntry> = Vec::new();
    let mut best_suffix_bytes: u64 = 0;

    if let Some(node) = graph.get(name) {
        for callee in &node.callees {
            let (suffix, suffix_bytes) = dfs_max_xdata(callee, graph, visited);
            if suffix_bytes > best_suffix_bytes
                || (suffix_bytes == best_suffix_bytes && suffix.len() > best_suffix.len())
            {
                best_suffix = suffix;
                best_suffix_bytes = suffix_bytes;
            }
        }
    }

    visited.remove(name);

    let mut chain = vec![CallChainEntry {
        name: name.to_string(),
        frame_size: frame,
    }];
    chain.extend(best_suffix);

    (chain, frame + best_suffix_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r"
OVERLAY MAP OF MODULE:   .\Objects\test (?C_STARTUP)

FUNCTION/MODULE                               XDATA_GROUP
--> CALLED FUNCTION/MODULE                    START  STOP
=========================================================
?C_C51STARTUP                                 ----- -----
  +--> MAIN/MAIN

MAIN/MAIN                                     0010H 0011H
  +--> FOO/MOD_A
  +--> BAR/MOD_B

FOO/MOD_A                                     0012H 0014H
  +--> LEAF/MOD_A

LEAF/MOD_A                                    0015H 0015H

BAR/MOD_B                                     0012H 0012H
";

    #[test]
    fn test_basic_parse() {
        let entries = parse_overlay_map(SAMPLE);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"MAIN/MAIN"));
        assert!(names.contains(&"FOO/MOD_A"));
        assert!(names.contains(&"BAR/MOD_B"));
        assert!(names.contains(&"LEAF/MOD_A"));
    }

    #[test]
    fn test_frame_sizes() {
        let entries = parse_overlay_map(SAMPLE);
        let main = entries.iter().find(|e| e.name == "MAIN/MAIN").unwrap();
        assert_eq!(main.frame_bytes, 2); // 0x11 - 0x10 + 1

        let foo = entries.iter().find(|e| e.name == "FOO/MOD_A").unwrap();
        assert_eq!(foo.frame_bytes, 3); // 0x14 - 0x12 + 1

        let leaf = entries.iter().find(|e| e.name == "LEAF/MOD_A").unwrap();
        assert_eq!(leaf.frame_bytes, 1); // 0x15 - 0x15 + 1
    }

    #[test]
    fn test_max_stack() {
        let result = parse(SAMPLE, MapFormat::KeilC51).unwrap();
        let ms = result.max_stack.unwrap();
        // Deepest path: ?C_C51STARTUP -> MAIN -> FOO -> LEAF = 0+2+3+1 = 6
        assert_eq!(ms.bytes, 6);
        assert_eq!(ms.chain.len(), 4);
    }
}
